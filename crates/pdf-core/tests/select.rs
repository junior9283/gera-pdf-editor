mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfRect};

fn doc(pages: usize) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&write_sample(&dir, "a.pdf", pages, 0), None).unwrap();
    (dir, d)
}

/// Retângulo de uma palavra da página 0 (para mirar a seleção).
fn word(d: &PdfDoc, text: &str) -> PdfRect {
    d.words_in_rect(0, PdfRect::new(0.0, 0.0, 10_000.0, 10_000.0))
        .unwrap()
        .into_iter()
        .find(|w| w.text == text)
        .unwrap()
        .rect
}

fn left_mid(r: PdfRect) -> (f32, f32) {
    (r.x0 + 1.0, (r.y0 + r.y1) / 2.0)
}

fn right_mid(r: PdfRect) -> (f32, f32) {
    (r.x1 - 1.0, (r.y0 + r.y1) / 2.0)
}

#[test]
fn selecting_from_first_to_last_word_takes_the_whole_line() {
    let (_d, d) = doc(1);
    let (a, b) = (word(&d, "Page"), word(&d, "Gera"));
    let sel = d.select_text(0, left_mid(a), right_mid(b)).unwrap();
    assert_eq!(sel.text.trim(), "Page 1 Hello Gera");
    assert!(!sel.quads.is_empty());
}

#[test]
fn selection_works_backwards_and_partially() {
    let (_d, d) = doc(1);
    let (h, g) = (word(&d, "Hello"), word(&d, "Gera"));
    let fwd = d.select_text(0, left_mid(h), right_mid(g)).unwrap();
    let back = d.select_text(0, right_mid(g), left_mid(h)).unwrap();
    assert_eq!(fwd.text.trim(), "Hello Gera");
    assert_eq!(back.text, fwd.text, "a direção do arrasto não importa");
}

#[test]
fn empty_area_selects_nothing_and_bad_pages_are_errors() {
    let (_d, d) = doc(1);
    let sel = d.select_text(0, (5.0, 700.0), (5.0, 700.0)).unwrap();
    assert!(sel.text.trim().is_empty(), "{:?}", sel.text);
    assert!(d.select_text(3, (0.0, 0.0), (1.0, 1.0)).is_err());
}

#[test]
fn document_text_has_every_page_in_order() {
    let (_d, d) = doc(3);
    let text = d.document_text().unwrap();
    let (p1, p3) = (text.find("Page 1").unwrap(), text.find("Page 3").unwrap());
    assert!(p1 < p3);
}
