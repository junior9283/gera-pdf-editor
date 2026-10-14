mod common;
use common::write_sample;
use pdf_core::{AnnotOp, NewAnnot, PdfDoc};

fn note() -> NewAnnot {
    NewAnnot::Note {
        page: 0,
        at: (50.0, 50.0),
        text: "pendente".into(),
    }
}

#[test]
fn export_for_print_includes_edits_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    doc.apply(AnnotOp::Create(note())).unwrap();
    let dest = dir.path().join("imprimir.pdf");
    doc.export_for_print(&dest, true).unwrap();
    assert_eq!(
        PdfDoc::open(&dest, None)
            .unwrap()
            .annotations(0)
            .unwrap()
            .len(),
        1
    );
    doc.export_for_print(&dest, false).unwrap();
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
    doc.export_for_print(&dest, false).unwrap();
    assert!(PdfDoc::open(&dest, None)
        .unwrap()
        .annotations(0)
        .unwrap()
        .is_empty());
}

#[test]
fn export_for_print_does_not_touch_the_open_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, 0), None).unwrap();
    doc.apply(AnnotOp::Create(note())).unwrap();
    doc.export_for_print(&dir.path().join("x.pdf"), false)
        .unwrap();
    assert_eq!(doc.annotations(0).unwrap().len(), 1, "a edição continua");
    assert!(doc.is_dirty() && doc.can_undo(), "e continua desfazível");
}

#[test]
fn export_for_print_writes_a_readable_unencrypted_copy() {
    let dir = tempfile::tempdir().unwrap();
    let locked = common::encrypted_sample(&dir);
    let doc = PdfDoc::open(&locked, Some("segredo")).unwrap();
    let dest = dir.path().join("claro.pdf");
    doc.export_for_print(&dest, true).unwrap();
    assert!(
        PdfDoc::open(&dest, None).is_ok(),
        "abre sem senha (o CUPS precisa ler)"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_export_leaves_no_file_behind() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, 0), None).unwrap();
    // nome que não é UTF-8 válido: o MuPDF não consegue gravar
    let dest = dir
        .path()
        .join(std::ffi::OsStr::from_bytes(b"\xffimprimir.pdf"));
    std::fs::write(&dest, b"restos de uma tentativa anterior").unwrap();
    assert!(doc.export_for_print(&dest, true).is_err());
    assert!(!dest.exists(), "a cópia sem senha não pode ficar no disco");
}

#[test]
fn printing_a_protected_file_does_not_unprotect_or_damage_the_next_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::encrypted_sample(&dir);
    let mut doc = PdfDoc::open(&path, Some("segredo")).unwrap();
    doc.apply(AnnotOp::Create(note())).unwrap();
    doc.export_for_print(&dir.path().join("imprimir.pdf"), true)
        .unwrap();
    doc.save(&path).unwrap();
    assert!(
        matches!(
            PdfDoc::open(&path, None),
            Err(pdf_core::PdfError::PasswordRequired)
        ),
        "o arquivo continua protegido"
    );
    let again = PdfDoc::open(&path, Some("segredo")).unwrap();
    assert_eq!(again.annotations(0).unwrap()[0].contents, "pendente");
    assert!(!again.document_text().unwrap().trim().is_empty());
}
