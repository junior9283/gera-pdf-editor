mod common;
use common::write_sample;
use pdf_core::{AnnotInfo, AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 2, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn note() -> NewAnnot {
    NewAnnot::Note {
        page: 0,
        at: (100.0, 100.0),
        text: "olá".into(),
    }
}

fn ink() -> NewAnnot {
    NewAnnot::Ink {
        page: 0,
        strokes: vec![vec![(100.0, 100.0), (200.0, 150.0)]],
    }
}

fn only(d: &PdfDoc) -> AnnotInfo {
    let mut v = d.annotations(0).unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

#[test]
fn create_undo_redo_keeps_the_same_id() {
    let (_d, _p, mut d) = doc();
    assert_eq!(d.apply(AnnotOp::Create(note())).unwrap(), vec![0]);
    let id = only(&d).id;
    assert!(d.is_dirty() && d.can_undo());
    d.undo().unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    assert!(!d.is_dirty() && d.can_redo());
    d.redo().unwrap();
    assert_eq!(only(&d).id, id, "refazer mostra a mesma anotação");
}

#[test]
fn moving_a_note_and_undoing() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let a = only(&d);
    d.apply(AnnotOp::Move {
        page: 0,
        id: a.id,
        dx: 30.0,
        dy: -10.0,
    })
    .unwrap();
    let moved = only(&d);
    assert!(
        (moved.rect.x0 - (a.rect.x0 + 30.0)).abs() < 0.5,
        "{moved:?}"
    );
    assert!(
        (moved.rect.y0 - (a.rect.y0 - 10.0)).abs() < 0.5,
        "{moved:?}"
    );
    d.undo().unwrap();
    assert!((only(&d).rect.x0 - a.rect.x0).abs() < 0.5);
}

#[test]
fn moving_ink_moves_its_strokes() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(ink())).unwrap();
    let a = only(&d);
    d.apply(AnnotOp::Move {
        page: 0,
        id: a.id,
        dx: 50.0,
        dy: 20.0,
    })
    .unwrap();
    let b = only(&d);
    assert!((b.rect.x0 - (a.rect.x0 + 50.0)).abs() < 1.0, "{b:?}");
    assert!((b.rect.y1 - (a.rect.y1 + 20.0)).abs() < 1.0, "{b:?}");
}

#[test]
fn resizing_ink_scales_its_strokes() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(ink())).unwrap();
    let a = only(&d);
    let bigger = PdfRect::new(
        a.rect.x0,
        a.rect.y0,
        a.rect.x0 + a.rect.width() * 2.0,
        a.rect.y0 + a.rect.height() * 2.0,
    );
    d.apply(AnnotOp::Resize {
        page: 0,
        id: a.id,
        rect: bigger,
    })
    .unwrap();
    let r = only(&d).rect;
    assert!((r.width() - a.rect.width() * 2.0).abs() < 3.0, "{r:?}");
    d.undo().unwrap();
    assert!((only(&d).rect.width() - a.rect.width()).abs() < 0.5);
}

#[test]
fn style_and_contents_are_undoable() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let a = only(&d);
    d.apply(AnnotOp::Style {
        page: 0,
        id: a.id,
        color: Some([0.0, 0.0, 1.0]),
        opacity: 0.5,
        width: None,
    })
    .unwrap();
    d.apply(AnnotOp::SetContents {
        page: 0,
        id: a.id,
        text: "novo".into(),
    })
    .unwrap();
    let b = only(&d);
    assert_eq!(b.contents, "novo");
    assert!((b.opacity - 0.5).abs() < 0.01);
    assert_eq!(b.color.map(|c| c[2] > 0.9), Some(true));
    d.undo().unwrap();
    d.undo().unwrap();
    let c = only(&d);
    assert_eq!(c.contents, "olá");
    assert!((c.opacity - a.opacity).abs() < 0.01);
    assert_eq!(c.color, a.color);
}

#[test]
fn deleting_an_external_annotation_and_undoing_restores_it_exactly() {
    let (_dir, path, mut d) = doc();
    d.save_with(&[note()], &path).unwrap(); // vira "externa": já está no arquivo
    let mut d = PdfDoc::open(&path, None).unwrap();
    let before = only(&d);
    d.apply(AnnotOp::Delete {
        page: 0,
        id: before.id,
    })
    .unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.undo().unwrap();
    assert_eq!(only(&d), before, "volta idêntica");
}

#[test]
fn saving_removes_hidden_and_starts_a_clean_history() {
    let (_d, path, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    d.apply(AnnotOp::Create(ink())).unwrap();
    let ink_id = d
        .annotations(0)
        .unwrap()
        .iter()
        .find(|a| a.kind == AnnotKind::Ink)
        .unwrap()
        .id;
    d.apply(AnnotOp::Delete {
        page: 0,
        id: ink_id,
    })
    .unwrap();
    d.save(&path).unwrap();
    assert!(!d.is_dirty() && !d.can_undo() && !d.can_redo());
    assert_eq!(
        common::count_raw_annotations(&path),
        1,
        "o desenho apagado saiu do arquivo"
    );
}

#[test]
fn operations_on_unknown_ids_fail_without_changes() {
    let (_d, _p, mut d) = doc();
    assert!(d
        .apply(AnnotOp::Delete {
            page: 0,
            id: 99_999
        })
        .is_err());
    assert!(!d.is_dirty() && !d.can_undo());
}

#[test]
fn originally_hidden_annotations_are_left_alone() {
    let (_dir, path, mut d) = doc();
    d.save_with(&[note()], &path).unwrap();
    common::hide_all_annotations(&path);
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.apply(AnnotOp::Create(ink())).unwrap();
    d.save(&path).unwrap();
    assert_eq!(
        common::count_raw_annotations(&path),
        2,
        "a escondida de fábrica continua no arquivo"
    );
}

#[test]
fn hidden_annotations_are_not_rendered() {
    let (_d, _p, mut d) = doc();
    let blank = d.render(0, 0.5, false).unwrap();
    d.apply(AnnotOp::Create(ink())).unwrap();
    assert_ne!(d.render(0, 0.5, false).unwrap(), blank);
    let id = only(&d).id;
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert_eq!(d.render(0, 0.5, false).unwrap(), blank);
}

#[test]
fn print_copy_comes_from_the_edited_document() {
    let (dir, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let dest = dir.path().join("imprimir.pdf");
    d.export_for_print(&dest, true).unwrap();
    assert_eq!(
        PdfDoc::open(&dest, None)
            .unwrap()
            .annotations(0)
            .unwrap()
            .len(),
        1
    );
    d.export_for_print(&dest, false).unwrap();
    assert!(PdfDoc::open(&dest, None)
        .unwrap()
        .annotations(0)
        .unwrap()
        .is_empty());
}
