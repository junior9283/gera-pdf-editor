mod common;
use common::write_sample;
use pdf_core::{sample::pdf_from_objects, DestOp, PdfDoc};

/// Árvore `/Names /Dests` com `/Kids` (e um ciclo) mais o dicionário antigo `/Dests`.
fn mixed(dir: &tempfile::TempDir) -> PdfDoc {
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Names<</Dests 5 0 R>>/Dests 8 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Kids[6 0 R 7 0 R]>>".to_string(),
        "<</Names[(cap1)[3 0 R/XYZ 100 700 null]]>>".to_string(),
        "<</Names[(cap2)<</D[4 0 R/Fit]>>]/Kids[5 0 R]>>".to_string(),
        "<</antigo[3 0 R/Fit]>>".to_string(),
    ];
    let path = dir.path().join("dest.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    PdfDoc::open(&path, None).unwrap()
}

fn names(d: &PdfDoc) -> Vec<(String, Option<i32>)> {
    d.destinations()
        .into_iter()
        .map(|x| (x.name, x.page))
        .collect()
}

#[test]
fn lists_name_tree_with_kids_and_old_dests_dict() {
    let dir = tempfile::tempdir().unwrap();
    let d = mixed(&dir);
    assert_eq!(
        names(&d),
        [
            ("antigo".to_string(), Some(0)),
            ("cap1".to_string(), Some(0)),
            ("cap2".to_string(), Some(1))
        ]
    );
    let cap1 = d
        .destinations()
        .into_iter()
        .find(|x| x.name == "cap1")
        .unwrap();
    let (x, y) = cap1.at.unwrap();
    assert!((x - 100.0).abs() < 0.5 && (y - 92.0).abs() < 0.5, "{x} {y}");
}

#[test]
fn add_rename_remove_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&write_sample(&dir, "a.pdf", 3, 0), None).unwrap();
    let name = d
        .edit_destination(DestOp::Add {
            name: "intro".into(),
            page: 1,
            at: Some((0.0, 0.0)),
        })
        .unwrap();
    assert_eq!(name.as_deref(), Some("intro"));
    d.edit_destination(DestOp::Rename {
        name: "intro".into(),
        to: "inicio".into(),
    })
    .unwrap();
    d.edit_destination(DestOp::Set {
        name: "inicio".into(),
        page: 2,
        at: None,
    })
    .unwrap();
    let out = dir.path().join("b.pdf");
    d.save(&out).unwrap();
    let mut back = PdfDoc::open(&out, None).unwrap();
    assert!(names(&back).contains(&("inicio".to_string(), Some(2))));
    back.edit_destination(DestOp::Remove {
        name: "inicio".into(),
    })
    .unwrap();
    assert!(!names(&back).iter().any(|(n, _)| n == "inicio"));
    back.undo().unwrap();
    assert!(
        names(&back).iter().any(|(n, _)| n == "inicio"),
        "desfazer volta"
    );
}

#[test]
fn repeated_name_gets_a_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    let add = |d: &mut PdfDoc| {
        d.edit_destination(DestOp::Add {
            name: "a".into(),
            page: 0,
            at: None,
        })
        .unwrap()
    };
    assert_eq!(add(&mut d).as_deref(), Some("a"));
    assert_eq!(add(&mut d).as_deref(), Some("a (2)"));
}

#[test]
fn editing_an_old_dests_entry_moves_it_to_the_name_tree() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = mixed(&dir);
    d.edit_destination(DestOp::Rename {
        name: "antigo".into(),
        to: "novo".into(),
    })
    .unwrap();
    let out = dir.path().join("b.pdf");
    d.save(&out).unwrap();
    let back = PdfDoc::open(&out, None).unwrap();
    let n = names(&back);
    assert!(n.contains(&("novo".to_string(), Some(0))), "{n:?}");
    assert!(!n.iter().any(|(x, _)| x == "antigo"), "{n:?}");
    assert_eq!(n.len(), 3, "cap1 e cap2 continuam: {n:?}");
}

#[test]
fn removing_a_missing_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = mixed(&dir);
    let missing = d.edit_destination(DestOp::Remove {
        name: "nao-existe".into(),
    });
    assert!(missing.is_err());
    assert!(!d.is_dirty());
}

#[test]
fn many_names_in_both_places_are_read_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let n = 40_000;
    let tree: String = (0..n).map(|k| format!("(t{k:06})[3 0 R/Fit]")).collect();
    let old: String = (0..n).map(|k| format!("/o{k:06}[3 0 R/Fit]")).collect();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Names<</Dests 4 0 R>>/Dests 5 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        format!("<</Names[{tree}]>>"),
        format!("<<{old}>>"),
    ];
    let path = dir.path().join("muitos.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let t = std::time::Instant::now();
    let list = d.destinations();
    assert!(list.len() >= 40_000);
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
}
