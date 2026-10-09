mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    (dir, doc)
}

#[test]
fn finds_text_with_a_sensible_rectangle() {
    let (_d, doc) = doc();
    let hits = doc.search(0, "Hello").unwrap();
    assert_eq!(hits.len(), 1);
    let r = hits[0];
    assert!((r.x0 - 72.0 - 70.0).abs() < 40.0, "x0 = {}", r.x0); // depois de "Page 1 "
    assert!(r.y0 > 50.0 && r.y1 < 110.0, "y = {}..{}", r.y0, r.y1); // topo da página (origem no canto superior)
    assert!(r.x1 > r.x0 && r.y1 > r.y0);
}

#[test]
fn degenerate_needles_return_nothing_without_panicking() {
    let (_d, doc) = doc();
    assert!(doc.search(0, "").unwrap().is_empty());
    assert!(doc.search(0, "a\0b").unwrap().is_empty());
    assert!(doc.search(0, "inexistente").unwrap().is_empty());
    assert!(doc.search(0, &"x".repeat(100_000)).unwrap().is_empty());
}

#[test]
fn words_in_rect_selects_by_intersection() {
    let (_d, doc) = doc();
    let all = doc
        .words_in_rect(0, PdfRect::new(0.0, 0.0, 612.0, 792.0))
        .unwrap();
    let texts: Vec<_> = all.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["Page", "1", "Hello", "Gera"]);
    let none = doc
        .words_in_rect(0, PdfRect::new(400.0, 600.0, 600.0, 780.0))
        .unwrap();
    assert!(none.is_empty());
}
