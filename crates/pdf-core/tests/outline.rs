mod common;
use common::write_sample;
use pdf_core::{sample::pdf_from_objects, OutlineNode, OutlineOp, PdfDoc};

/// 3 páginas e o índice de amostra: "Capitulo 1..3", cada um para a sua página.
fn doc() -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&write_sample(&dir, "a.pdf", 3, 0), None).unwrap();
    (dir, d)
}

fn titles(nodes: &[OutlineNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|n| format!("{}{}", "  ".repeat(n.depth), n.title))
        .collect()
}

fn tree(d: &PdfDoc) -> Vec<OutlineNode> {
    d.outline_tree().unwrap()
}

fn id_of(d: &PdfDoc, title: &str) -> i32 {
    tree(d).into_iter().find(|n| n.title == title).unwrap().id
}

#[test]
fn reads_titles_destinations_and_style() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Outlines 5 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Type/Outlines/First 6 0 R/Last 6 0 R/Count 2>>".to_string(),
        "<</Title(Pai)/Parent 5 0 R/First 7 0 R/Last 7 0 R/Count 1/C[1 0 0]/F 3/Dest[3 0 R/XYZ 100 700 null]>>".to_string(),
        "<</Title(Filho)/Parent 6 0 R/A<</S/GoTo/D[4 0 R/Fit]>>>>".to_string(),
    ];
    let path = dir.path().join("o.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let t = tree(&d);
    assert_eq!(titles(&t), ["Pai", "  Filho"]);
    let pai = &t[0];
    assert_eq!(
        (pai.page, pai.open, pai.bold, pai.italic),
        (Some(0), true, true, true)
    );
    assert_eq!(pai.color, Some([1.0, 0.0, 0.0]));
    let (x, y) = pai.at.unwrap();
    assert!(
        (x - 100.0).abs() < 0.5 && (y - 92.0).abs() < 0.5,
        "coordenadas do MuPDF: {x} {y}"
    );
    assert_eq!((t[1].page, t[1].parent), (Some(1), Some(pai.id)));
}

#[test]
fn add_rename_delete_with_children() {
    let (_d, mut d) = doc();
    let c1 = id_of(&d, "Capitulo 1");
    let new = d
        .edit_outline(OutlineOp::Add {
            parent: Some(c1),
            after: None,
            title: "Seção 1.1".into(),
            page: 0,
            at: Some((72.0, 100.0)),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        titles(&tree(&d)),
        ["Capitulo 1", "  Seção 1.1", "Capitulo 2", "Capitulo 3"]
    );
    d.edit_outline(OutlineOp::Rename {
        id: new,
        title: "Introdução".into(),
    })
    .unwrap();
    let node = tree(&d).into_iter().find(|n| n.id == new).unwrap();
    assert_eq!((node.title.as_str(), node.page), ("Introdução", Some(0)));
    let (x, y) = node.at.unwrap();
    assert!((x - 72.0).abs() < 0.5 && (y - 100.0).abs() < 0.5, "{x} {y}");
    // excluir o pai leva o filho
    d.edit_outline(OutlineOp::Delete { id: c1 }).unwrap();
    assert_eq!(titles(&tree(&d)), ["Capitulo 2", "Capitulo 3"]);
    // no topo, depois de outro
    let c2 = id_of(&d, "Capitulo 2");
    d.edit_outline(OutlineOp::Add {
        parent: None,
        after: Some(c2),
        title: "Meio".into(),
        page: 1,
        at: None,
    })
    .unwrap();
    assert_eq!(titles(&tree(&d)), ["Capitulo 2", "Meio", "Capitulo 3"]);
}

#[test]
fn move_reorders_and_nests_and_refuses_cycles() {
    let (_d, mut d) = doc();
    let (c1, c2, c3) = (
        id_of(&d, "Capitulo 1"),
        id_of(&d, "Capitulo 2"),
        id_of(&d, "Capitulo 3"),
    );
    d.edit_outline(OutlineOp::Move {
        id: c3,
        parent: None,
        after: None,
    })
    .unwrap();
    assert_eq!(
        titles(&tree(&d)),
        ["Capitulo 3", "Capitulo 1", "Capitulo 2"]
    );
    d.edit_outline(OutlineOp::Move {
        id: c1,
        parent: Some(c3),
        after: None,
    })
    .unwrap();
    d.edit_outline(OutlineOp::Move {
        id: c2,
        parent: Some(c3),
        after: Some(c1),
    })
    .unwrap();
    assert_eq!(
        titles(&tree(&d)),
        ["Capitulo 3", "  Capitulo 1", "  Capitulo 2"]
    );
    // para dentro de si mesmo ou de um descendente: recusado, nada muda
    assert!(d
        .edit_outline(OutlineOp::Move {
            id: c3,
            parent: Some(c1),
            after: None
        })
        .is_err());
    assert!(d
        .edit_outline(OutlineOp::Move {
            id: c3,
            parent: Some(c3),
            after: None
        })
        .is_err());
    assert_eq!(
        titles(&tree(&d)),
        ["Capitulo 3", "  Capitulo 1", "  Capitulo 2"]
    );
}

#[test]
fn style_destination_and_open_state() {
    let (_d, mut d) = doc();
    let c1 = id_of(&d, "Capitulo 1");
    d.edit_outline(OutlineOp::Style {
        id: c1,
        color: Some([0.0, 0.0, 1.0]),
        bold: true,
        italic: false,
    })
    .unwrap();
    d.edit_outline(OutlineOp::SetDest {
        id: c1,
        page: 2,
        at: None,
    })
    .unwrap();
    let n = tree(&d).into_iter().find(|n| n.id == c1).unwrap();
    assert_eq!(
        (n.color, n.bold, n.italic, n.page),
        (Some([0.0, 0.0, 1.0]), true, false, Some(2))
    );
    let c2 = id_of(&d, "Capitulo 2");
    d.edit_outline(OutlineOp::Move {
        id: c2,
        parent: Some(c1),
        after: None,
    })
    .unwrap();
    d.edit_outline(OutlineOp::SetOpen {
        id: c1,
        open: false,
    })
    .unwrap();
    assert!(!tree(&d).into_iter().find(|n| n.id == c1).unwrap().open);
}

#[test]
fn counts_are_rewritten() {
    let (dir, mut d) = doc();
    let (c1, c2) = (id_of(&d, "Capitulo 1"), id_of(&d, "Capitulo 2"));
    d.edit_outline(OutlineOp::Move {
        id: c2,
        parent: Some(c1),
        after: None,
    })
    .unwrap();
    d.edit_outline(OutlineOp::SetOpen { id: c1, open: true })
        .unwrap();
    let out = dir.path().join("c.pdf");
    d.save(&out).unwrap();
    // outro leitor (o MuPDF) vê a mesma árvore
    let r = PdfDoc::open(&out, None).unwrap();
    let flat: Vec<_> = r
        .outline()
        .unwrap()
        .into_iter()
        .map(|o| (o.title, o.depth))
        .collect();
    assert_eq!(
        flat,
        [
            ("Capitulo 1".to_string(), 0),
            ("Capitulo 2".to_string(), 1),
            ("Capitulo 3".to_string(), 0)
        ]
    );
    let counts = r.outline_counts();
    assert_eq!(
        counts,
        vec![
            ("Capitulo 1".to_string(), 1),
            ("Capitulo 2".to_string(), 0),
            ("Capitulo 3".to_string(), 0)
        ],
        "Count = filhos visíveis"
    );
}

#[test]
fn broken_tree_with_a_cycle_is_read_and_fixed() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Outlines 4 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Type/Outlines/First 5 0 R/Last 6 0 R/Count 99>>".to_string(),
        // 5 → 6 → 5 … (ciclo), e 6 com /First apontando para o pai
        "<</Title(A)/Next 6 0 R>>".to_string(),
        "<</Title(B)/Next 5 0 R/First 4 0 R>>".to_string(),
    ];
    let path = dir.path().join("ciclo.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let t = tree(&d);
    assert_eq!(titles(&t), ["A", "B"], "cada nó uma vez");
    let a = t[0].id;
    d.edit_outline(OutlineOp::Rename {
        id: a,
        title: "A2".into(),
    })
    .unwrap();
    assert_eq!(titles(&tree(&d)), ["A2", "B"]);
}

#[test]
fn each_op_undoes_in_one_step() {
    let (_d, mut d) = doc();
    let before = titles(&tree(&d));
    let c1 = id_of(&d, "Capitulo 1");
    d.edit_outline(OutlineOp::Delete { id: c1 }).unwrap();
    d.undo().unwrap();
    assert_eq!(titles(&tree(&d)), before);
    d.edit_outline(OutlineOp::Add {
        parent: None,
        after: None,
        title: "Novo".into(),
        page: 0,
        at: None,
    })
    .unwrap();
    d.undo().unwrap();
    assert_eq!(titles(&tree(&d)), before);
}

#[test]
fn first_bookmark_in_a_document_without_outline() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
    ];
    let path = dir.path().join("sem.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(tree(&d).is_empty());
    d.edit_outline(OutlineOp::Add {
        parent: None,
        after: None,
        title: "Primeiro".into(),
        page: 0,
        at: None,
    })
    .unwrap();
    assert_eq!(titles(&tree(&d)), ["Primeiro"]);
    assert_eq!(d.outline().unwrap().len(), 1, "o MuPDF também vê");
}
