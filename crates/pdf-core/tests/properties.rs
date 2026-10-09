mod common;
use common::write_sample;
use pdf_core::PdfDoc;

#[test]
fn properties_describe_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "p.pdf", 3, 0);
    let p = PdfDoc::open(&path, None).unwrap().properties().unwrap();
    assert_eq!(p.pages, 3);
    assert_eq!((p.first_page.width, p.first_page.height), (612.0, 792.0));
    assert!(p.format.starts_with("PDF"), "{}", p.format);
    assert!(!p.encrypted);
    assert_eq!(p.file_size, std::fs::metadata(&path).unwrap().len());
    assert_eq!(p.path, path);
}

#[test]
fn protected_files_are_reported_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::encrypted_sample(&dir);
    let p = PdfDoc::open(&path, Some("segredo"))
        .unwrap()
        .properties()
        .unwrap();
    assert!(p.encrypted);
}
