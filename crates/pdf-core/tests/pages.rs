mod common;
use common::write_sample;
use pdf_core::{AnnotOp, Changed, NewAnnot, PageOp, PdfDoc};

fn doc(pages: usize) -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", pages, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

/// Número de cada página pelo texto ("Page N"), na ordem atual.
fn order(d: &PdfDoc) -> Vec<u32> {
    (0..d.page_count().unwrap())
        .map(|p| {
            let t = d.page_text(p).unwrap();
            let n = t
                .split_whitespace()
                .skip_while(|w| *w != "Page")
                .nth(1)
                .unwrap();
            n.parse().unwrap()
        })
        .collect()
}

#[test]
fn rotate_delete_move_insert_and_undo_each() {
    let (_d, _p, mut d) = doc(4);
    let start = order(&d);
    assert_eq!(start, [1, 2, 3, 4]);
    d.apply_pages(PageOp::Rotate {
        pages: vec![1],
        quarter_turns: 1,
    })
    .unwrap();
    assert_eq!(d.page_rotation(1).unwrap(), 90);
    d.apply_pages(PageOp::Delete { pages: vec![0] }).unwrap();
    assert_eq!(order(&d), [2, 3, 4]);
    d.apply_pages(PageOp::Move {
        pages: vec![2],
        to: 0,
    })
    .unwrap();
    assert_eq!(order(&d), [4, 2, 3]);
    d.apply_pages(PageOp::InsertBlank {
        at: 1,
        size: (300.0, 400.0),
    })
    .unwrap();
    assert_eq!(d.page_count().unwrap(), 4);
    assert_eq!(d.page_size(1).unwrap().width, 300.0);
    for _ in 0..4 {
        assert!(matches!(d.undo().unwrap(), Changed::Document));
    }
    assert_eq!(order(&d), start);
    assert_eq!(d.page_rotation(1).unwrap(), 0);
    for _ in 0..4 {
        d.redo().unwrap();
    }
    assert_eq!(d.page_count().unwrap(), 4);
    assert_eq!(
        d.page_rotation(2).unwrap(),
        90,
        "a página 2 original, agora na posição 2"
    );
}

#[test]
fn moving_scattered_pages_keeps_their_order() {
    let (_d, _p, mut d) = doc(5);
    d.apply_pages(PageOp::Move {
        pages: vec![0, 2],
        to: 5,
    })
    .unwrap();
    assert_eq!(order(&d), [2, 4, 5, 1, 3]);
    d.apply_pages(PageOp::Move {
        pages: vec![3, 4],
        to: 1,
    })
    .unwrap();
    assert_eq!(order(&d), [2, 1, 3, 4, 5]);
}

#[test]
fn cannot_delete_every_page() {
    let (_d, _p, mut d) = doc(2);
    assert!(d.apply_pages(PageOp::Delete { pages: vec![0, 1] }).is_err());
    assert_eq!(d.page_count().unwrap(), 2);
    assert!(!d.is_dirty());
}

#[test]
fn insert_from_file_brings_annotations_and_range() {
    let (dir, _p, mut d) = doc(2);
    let other = write_sample(&dir, "b.pdf", 3, 0);
    let mut o = PdfDoc::open(&other, None).unwrap();
    o.apply(AnnotOp::Create(NewAnnot::Note {
        page: 1,
        at: (50.0, 50.0),
        text: "vem junto".into(),
    }))
    .unwrap();
    o.save(&other).unwrap();
    d.apply_pages(PageOp::InsertFrom {
        at: 1,
        path: other,
        password: None,
        range: Some((1, 2)),
    })
    .unwrap();
    assert_eq!(order(&d), [1, 2, 3, 2]);
    assert_eq!(d.annotations(1).unwrap()[0].contents, "vem junto");
}

#[test]
fn insert_from_protected_pdf_needs_the_password() {
    let (dir, _p, mut d) = doc(1);
    let locked = common::encrypted_sample(&dir);
    let op = |pw: Option<&str>| PageOp::InsertFrom {
        at: 1,
        path: locked.clone(),
        password: pw.map(str::to_owned),
        range: None,
    };
    assert!(d.apply_pages(op(Some("errada"))).is_err());
    assert_eq!(d.page_count().unwrap(), 1);
    d.apply_pages(op(Some("segredo"))).unwrap();
    assert_eq!(d.page_count().unwrap(), 2);
}

#[test]
fn replace_swaps_pages_from_another_file() {
    let (dir, _p, mut d) = doc(3);
    let other = write_sample(&dir, "b.pdf", 3, 0);
    d.apply_pages(PageOp::Replace {
        pages: vec![0, 2],
        path: other,
        password: None,
        from: 1,
    })
    .unwrap();
    assert_eq!(order(&d), [2, 2, 3]);
}

#[test]
fn crop_sets_the_crop_box_and_undo_restores() {
    let (_d, _p, mut d) = doc(1);
    let before = d.page_size(0).unwrap();
    d.apply_pages(PageOp::Crop {
        pages: vec![0],
        margins: [10.0, 20.0, 30.0, 40.0],
    })
    .unwrap();
    let after = d.page_size(0).unwrap();
    assert!(
        (after.width - (before.width - 70.0)).abs() < 0.5,
        "{after:?}"
    );
    assert!(
        (after.height - (before.height - 30.0)).abs() < 0.5,
        "{after:?}"
    );
    d.undo().unwrap();
    assert_eq!(d.page_size(0).unwrap(), before);
}

#[test]
fn hidden_annotations_survive_page_undo() {
    let (_d, _p, mut d) = doc(3);
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 2,
        at: (50.0, 50.0),
        text: "x".into(),
    }))
    .unwrap();
    let id = d.annotations(2).unwrap()[0].id;
    d.apply(AnnotOp::Delete { page: 2, id }).unwrap();
    d.apply_pages(PageOp::Delete { pages: vec![0] }).unwrap();
    assert!(
        d.annotations(1).unwrap().is_empty(),
        "apagada continua apagada"
    );
    d.undo().unwrap(); // volta a página
    assert!(d.annotations(2).unwrap().is_empty(), "e não ressuscita");
    d.undo().unwrap(); // desfaz o apagar
    assert_eq!(d.annotations(2).unwrap().len(), 1);
}

#[test]
fn undo_mixes_page_and_annotation_steps() {
    let (_d, _p, mut d) = doc(2);
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 1,
        at: (50.0, 50.0),
        text: "a".into(),
    }))
    .unwrap();
    d.apply_pages(PageOp::Move {
        pages: vec![1],
        to: 0,
    })
    .unwrap();
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 1,
        at: (60.0, 60.0),
        text: "b".into(),
    }))
    .unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].contents, "a");
    assert!(matches!(d.undo().unwrap(), Changed::Pages(_)));
    assert!(d.annotations(1).unwrap().is_empty());
    assert!(matches!(d.undo().unwrap(), Changed::Document));
    assert_eq!(d.annotations(1).unwrap()[0].contents, "a");
    d.undo().unwrap();
    assert!(d.annotations(1).unwrap().is_empty());
    assert!(!d.is_dirty());
}

#[test]
fn extract_writes_one_or_many_files_and_keeps_the_document() {
    let (dir, _p, mut d) = doc(4);
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 3,
        at: (50.0, 50.0),
        text: "junto".into(),
    }))
    .unwrap();
    let one = d
        .extract(&[1, 3], &dir.path().join("ext.pdf"), false)
        .unwrap();
    assert_eq!(one.len(), 1);
    let ext = PdfDoc::open(&one[0], None).unwrap();
    assert_eq!(order(&ext), [2, 4]);
    assert_eq!(
        ext.annotations(1).unwrap()[0].contents,
        "junto",
        "comentários vão junto"
    );
    let gone = d.annotations(3).unwrap()[0].id;
    d.apply(AnnotOp::Delete { page: 3, id: gone }).unwrap();
    let again = d.extract(&[3], &dir.path().join("sem.pdf"), false).unwrap();
    assert_eq!(
        common::count_raw_annotations(&again[0]),
        0,
        "a apagada não vai junto"
    );
    let many = d
        .extract(&[0, 2], &dir.path().join("sep.pdf"), true)
        .unwrap();
    assert_eq!(many.len(), 2);
    assert_eq!(order(&PdfDoc::open(&many[1], None).unwrap()), [3]);
    assert_eq!(d.page_count().unwrap(), 4);
}

#[test]
fn page_op_is_saved() {
    let (_d, path, mut d) = doc(3);
    d.apply_pages(PageOp::Delete { pages: vec![1] }).unwrap();
    d.save(&path).unwrap();
    assert_eq!(order(&PdfDoc::open(&path, None).unwrap()), [1, 3]);
    assert!(!d.is_dirty());
}

#[test]
fn crop_on_a_rotated_page_cuts_what_is_seen() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let before = d.page_size(0).unwrap();
    d.apply_pages(PageOp::Crop {
        pages: vec![0],
        margins: [100.0, 0.0, 0.0, 0.0],
    })
    .unwrap();
    let after = d.page_size(0).unwrap();
    assert!(
        (after.width - before.width).abs() < 0.5,
        "{before:?} → {after:?}"
    );
    assert!(
        (after.height - (before.height - 100.0)).abs() < 0.5,
        "{before:?} → {after:?}"
    );
}

#[test]
fn deleted_pages_leave_the_file_and_their_bookmarks_point_nowhere() {
    let (_d, path, mut d) = doc(3);
    d.apply_pages(PageOp::Delete { pages: vec![0] }).unwrap();
    let outline = d.outline().unwrap();
    assert_eq!(
        outline[0].page, None,
        "o marcador da página excluída não leva a outra"
    );
    assert_eq!(outline[1].page, Some(0));
    d.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.windows(12).any(|w| w == b"Page 1 Hello"),
        "o conteúdo da página excluída não pode ficar no arquivo"
    );
}

/// Quantas fontes (objetos /BaseFont) o arquivo tem.
fn fonts_in(path: &std::path::Path) -> usize {
    let bytes = std::fs::read(path).unwrap();
    bytes.windows(9).filter(|w| w == b"/BaseFont").count()
}

#[test]
fn inserting_pages_shares_their_resources() {
    let (dir, path, mut d) = doc(1);
    let other = write_sample(&dir, "b.pdf", 6, 0);
    let mut o = PdfDoc::open(&other, None).unwrap();
    o.apply_pages(PageOp::Rotate {
        pages: vec![0],
        quarter_turns: 0,
    })
    .ok();
    d.apply_pages(PageOp::InsertFrom {
        at: 1,
        path: other,
        password: None,
        range: None,
    })
    .unwrap();
    d.save(&path).unwrap();
    assert!(
        fonts_in(&path) <= 2,
        "a fonte compartilhada não pode ser copiada por página: {}",
        fonts_in(&path)
    );
}

#[test]
fn replace_keeps_the_comments_of_the_original_page() {
    let (dir, _p, mut d) = doc(2);
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (50.0, 50.0),
        text: "meu".into(),
    }))
    .unwrap();
    let other = write_sample(&dir, "b.pdf", 3, 0);
    d.apply_pages(PageOp::Replace {
        pages: vec![0],
        path: other,
        password: None,
        from: 2,
    })
    .unwrap();
    assert_eq!(order(&d), [3, 2]);
    assert_eq!(
        d.annotations(0).unwrap()[0].contents,
        "meu",
        "comentário do usuário fica"
    );
    assert_eq!(
        d.outline().unwrap()[0].page,
        Some(0),
        "o marcador continua levando à página"
    );
}

#[test]
fn extract_never_overwrites_the_open_file_or_existing_parts() {
    let (dir, path, d) = doc(3);
    assert!(
        d.extract(&[0], &path, false).is_err(),
        "não grava por cima do documento aberto"
    );
    let base = dir.path().join("x.pdf");
    std::fs::write(dir.path().join("x_1.pdf"), b"meu arquivo").unwrap();
    let files = d.extract(&[0], &base, true).unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("x_1.pdf")).unwrap(),
        b"meu arquivo"
    );
    assert_ne!(files[0], dir.path().join("x_1.pdf"));
}
