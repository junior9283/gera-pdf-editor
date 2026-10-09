mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfError};

#[test]
fn opens_and_reports_pages_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 3, 0);
    let doc = PdfDoc::open(&path, None).unwrap();
    assert_eq!(doc.page_count().unwrap(), 3);
    let size = doc.page_size(0).unwrap();
    assert_eq!((size.width, size.height), (612.0, 792.0));
}

#[test]
fn rotated_page_swaps_width_and_height() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 90);
    let size = PdfDoc::open(&path, None).unwrap().page_size(0).unwrap();
    assert_eq!((size.width, size.height), (792.0, 612.0));
}

#[test]
fn out_of_range_page_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    assert!(matches!(doc.page_size(2), Err(PdfError::PageOutOfRange(2))));
    assert!(matches!(
        doc.page_size(-1),
        Err(PdfError::PageOutOfRange(-1))
    ));
}
