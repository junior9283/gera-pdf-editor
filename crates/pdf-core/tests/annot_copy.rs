mod common;
use common::write_sample;
use pdf_core::{AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 2, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn shape(d: &mut PdfDoc) -> i32 {
    d.apply(AnnotOp::Create(NewAnnot::Shape {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 200.0, 150.0),
        ellipse: false,
    }))
    .unwrap();
    d.annotations(0).unwrap()[0].id
}

#[test]
fn duplicate_copies_kind_contents_and_shifts() {
    let (_t, _p, mut d) = doc();
    let id = shape(&mut d);
    d.apply(AnnotOp::SetContents {
        page: 0,
        id,
        text: "cópia".into(),
    })
    .unwrap();
    let new = d.duplicate_annot(0, id, 0, 10.0, 10.0).unwrap();
    let all = d.annotations(0).unwrap();
    assert_eq!(all.len(), 2);
    let (a, b) = (
        all.iter().find(|x| x.id == id).unwrap(),
        all.iter().find(|x| x.id == new).unwrap(),
    );
    assert_eq!((a.kind, a.contents.as_str()), (b.kind, b.contents.as_str()));
    assert!(
        (b.rect.x0 - a.rect.x0 - 10.0).abs() < 0.5 && (b.rect.y0 - a.rect.y0 - 10.0).abs() < 0.5,
        "{a:?} {b:?}"
    );
}

#[test]
fn duplicate_to_another_page_and_undo_in_one_step() {
    let (_t, _p, mut d) = doc();
    let id = shape(&mut d);
    let new = d.duplicate_annot(0, id, 1, 0.0, 0.0).unwrap();
    assert!(d.annotations(1).unwrap().iter().any(|x| x.id == new));
    d.undo().unwrap();
    assert!(
        d.annotations(1).unwrap().is_empty(),
        "um desfazer tira a cópia"
    );
    assert_eq!(d.annotations(0).unwrap().len(), 1, "o original fica");
}

#[test]
fn the_copy_does_not_share_its_appearance() {
    // mexer na cópia não pode mudar o original (a aparência foi copiada, não compartilhada)
    let (_t, _p, mut d) = doc();
    let id = shape(&mut d);
    let before = d.annotations(0).unwrap()[0].clone();
    let new = d.duplicate_annot(0, id, 0, 40.0, 40.0).unwrap();
    d.apply(AnnotOp::Style {
        page: 0,
        id: new,
        color: Some([0.0, 1.0, 0.0]),
        opacity: 0.5,
        width: Some(4.0),
    })
    .unwrap();
    let orig = d
        .annotations(0)
        .unwrap()
        .into_iter()
        .find(|x| x.id == id)
        .unwrap();
    assert_eq!(
        (orig.color, orig.width, orig.opacity),
        (before.color, before.width, before.opacity)
    );
}

#[test]
fn duplicate_rejects_missing_annotations_and_pages() {
    let (_t, _p, mut d) = doc();
    let id = shape(&mut d);
    assert!(d.duplicate_annot(0, 9999, 0, 0.0, 0.0).is_err());
    assert!(d.duplicate_annot(0, id, 7, 0.0, 0.0).is_err());
}

#[test]
fn duplicate_is_an_annotation_op() {
    let (_t, _p, mut d) = doc();
    let id = shape(&mut d);
    let pages = d
        .apply(AnnotOp::Duplicate {
            page: 0,
            id,
            to_page: 1,
            dx: 0.0,
            dy: 0.0,
        })
        .unwrap();
    assert_eq!(pages, vec![1]);
    assert_eq!(d.annotations(1).unwrap().len(), 1);
}

#[test]
fn revert_goes_back_to_the_saved_file() {
    let (_t, _p, mut d) = doc();
    shape(&mut d);
    assert!(d.is_dirty());
    d.revert().unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    assert!(!d.is_dirty() && !d.can_undo());
}

#[test]
fn cut_then_paste_gives_a_visible_comment_also_after_saving() {
    let (dir, _p, mut d) = doc();
    let id = shape(&mut d);
    // Recortar: o original fica escondido (apagado até salvar)
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    let new = d.duplicate_annot(0, id, 1, 0.0, 0.0).unwrap();
    assert!(d.annotations(1).unwrap().iter().any(|a| a.id == new), "a cópia aparece");
    let out = dir.path().join("salvo.pdf");
    d.save(&out).unwrap();
    let reopened = PdfDoc::open(&out, None).unwrap();
    assert_eq!(reopened.annotations(1).unwrap().len(), 1, "e continua visível no arquivo");
    assert!(reopened.annotations(0).unwrap().is_empty(), "o recortado saiu");
}

#[test]
fn indirect_border_is_not_shared_with_the_copy() {
    // PDF de outro programa: a borda (/BS) é um objeto indireto
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        "<</Type/Annot/Subtype/Square/Rect[100 100 200 200]/C[1 0 0]/BS 5 0 R/StructParent 3>>".to_string(),
        "<</W 1/S/S>>".to_string(),
    ];
    let path = dir.path().join("bs.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.annotations(0).unwrap()[0].id;
    let new = d.duplicate_annot(0, id, 0, 10.0, 10.0).unwrap();
    d.apply(AnnotOp::Style {
        page: 0,
        id: new,
        color: Some([0.0, 0.0, 1.0]),
        opacity: 1.0,
        width: Some(6.0),
    })
    .unwrap();
    let orig = d.annotations(0).unwrap().into_iter().find(|a| a.id == id).unwrap();
    assert_eq!(orig.width, Some(1.0), "a borda do original não mudou");
}
