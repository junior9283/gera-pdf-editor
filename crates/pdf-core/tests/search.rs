use pdf_core::{sample::pdf_from_objects, PdfDoc, SearchOptions};
use std::path::{Path, PathBuf};

/// Duas páginas: a 1 com uma linha de texto e uma nota; um marcador para a página 2.
fn doc(dir: &Path) -> PathBuf {
    let text = "BT /F1 12 Tf 72 700 Td (Gera geral GERA regera a.b \\(x\\) axb) Tj ET";
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Outlines 7 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R 5 0 R]/Count 2>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 6 0 R>>>>/Contents 4 0 R/Annots[9 0 R]>>".to_string(),
        format!("<</Length {}>>\nstream\n{text}\nendstream", text.len()),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string(),
        "<</Type/Outlines/First 8 0 R/Last 8 0 R/Count 1>>".to_string(),
        "<</Title(Capitulo sobre Gera)/Parent 7 0 R/Dest[5 0 R/Fit]>>".to_string(),
        "<</Type/Annot/Subtype/Text/Rect[300 300 320 320]/Contents(nota secreta da Gera)>>".to_string(),
    ];
    let path = dir.join("busca.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    path
}

fn count(opts: SearchOptions, needle: &str) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&doc(dir.path()), None).unwrap();
    d.search_with(0, needle, &opts).unwrap().len()
}

#[test]
fn default_search_ignores_case_and_finds_parts_of_words() {
    // Gera, geral, GERA, regera
    assert_eq!(count(SearchOptions::default(), "gera"), 4);
}

#[test]
fn whole_words_skip_parts_of_words() {
    let o = SearchOptions {
        whole_words: true,
        ..Default::default()
    };
    assert_eq!(count(o, "gera"), 2, "Gera e GERA");
}

#[test]
fn match_case_is_optional() {
    let o = SearchOptions {
        match_case: true,
        ..Default::default()
    };
    assert_eq!(count(o, "GERA"), 1);
    let both = SearchOptions {
        match_case: true,
        whole_words: true,
        ..Default::default()
    };
    assert_eq!(count(both, "Gera"), 1);
}

#[test]
fn regex_characters_are_literal() {
    let o = SearchOptions {
        whole_words: true,
        ..Default::default()
    };
    assert_eq!(count(o, "a.b"), 1, "não casa axb");
    let o = SearchOptions {
        match_case: true,
        ..Default::default()
    };
    assert_eq!(count(o, "(x)"), 1);
    assert_eq!(count(o, "\\"), 0);
}

#[test]
fn comments_are_found_when_asked() {
    let o = SearchOptions {
        comments: true,
        match_case: true,
        ..Default::default()
    };
    assert_eq!(
        count(
            SearchOptions {
                match_case: true,
                ..Default::default()
            },
            "secreta"
        ),
        0
    );
    assert_eq!(count(o, "secreta"), 1);
}

#[test]
fn bookmarks_are_found_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&doc(dir.path()), None).unwrap();
    let off = d.search_bookmarks("sobre", &SearchOptions::default());
    assert!(off.is_empty(), "sem a opção, nada");
    let o = SearchOptions {
        bookmarks: true,
        ..Default::default()
    };
    let hits = d.search_bookmarks("SOBRE", &o);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, 1, "a página do marcador");
}
