mod common;
use common::write_sample;
use pdf_core::{NewAnnot, PdfDoc};

fn note() -> NewAnnot {
    NewAnnot::Note {
        page: 0,
        at: (50.0, 50.0),
        text: "pendente".into(),
    }
}

#[test]
fn export_for_print_includes_pending_annotations_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    let dest = dir.path().join("imprimir.pdf");
    doc.export_for_print(&[note()], &dest, true).unwrap();
    assert_eq!(
        PdfDoc::open(&dest, None)
            .unwrap()
            .annotations(0)
            .unwrap()
            .len(),
        1
    );
    doc.export_for_print(&[note()], &dest, false).unwrap();
    assert!(
        PdfDoc::open(&dest, None)
            .unwrap()
            .annotations(0)
            .unwrap()
            .is_empty(),
        "só o documento"
    );
}

#[test]
fn saved_annotations_are_dropped_when_printing_only_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "b.pdf", 1, 0);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    doc.save_with(&[note()], &path).unwrap();
    let dest = dir.path().join("so-documento.pdf");
    doc.export_for_print(&[], &dest, false).unwrap();
    assert!(PdfDoc::open(&dest, None)
        .unwrap()
        .annotations(0)
        .unwrap()
        .is_empty());
}

#[test]
fn export_for_print_does_not_touch_the_open_document() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, 0), None).unwrap();
    doc.export_for_print(&[note()], &dir.path().join("x.pdf"), true)
        .unwrap();
    assert!(
        doc.annotations(0).unwrap().is_empty(),
        "a pendente não entra no documento aberto"
    );
}

#[test]
fn export_for_print_writes_a_readable_unencrypted_copy() {
    let dir = tempfile::tempdir().unwrap();
    let locked = common::encrypted_sample(&dir);
    let doc = PdfDoc::open(&locked, Some("segredo")).unwrap();
    let dest = dir.path().join("claro.pdf");
    doc.export_for_print(&[], &dest, true).unwrap();
    assert!(
        PdfDoc::open(&dest, None).is_ok(),
        "abre sem senha (o CUPS precisa ler)"
    );
}
