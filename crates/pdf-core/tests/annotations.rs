mod common;
use common::write_sample;
use pdf_core::{AnnotKind, NewAnnot, PdfDoc, PdfError, PdfRect};

fn setup() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let doc = PdfDoc::open(&path, None).unwrap();
    (dir, path, doc)
}

fn highlight(hit: PdfRect) -> NewAnnot {
    NewAnnot::Highlight {
        page: 0,
        rects: vec![hit],
    }
}

fn all_kinds(hit: PdfRect) -> Vec<NewAnnot> {
    vec![
        highlight(hit),
        NewAnnot::Underline {
            page: 0,
            rects: vec![hit],
        },
        NewAnnot::Note {
            page: 0,
            at: (300.0, 300.0),
            text: "olá nota".into(),
        },
        NewAnnot::Ink {
            page: 0,
            strokes: vec![vec![(100.0, 400.0), (150.0, 420.0), (200.0, 400.0)]],
        },
    ]
}

#[test]
fn all_four_kinds_survive_save_and_reopen() {
    let (_d, path, mut doc) = setup();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&all_kinds(hit), &path).unwrap();
    let reopened = PdfDoc::open(&path, None).unwrap();
    let annots = reopened.annotations(0).unwrap();
    let kinds: Vec<_> = annots.iter().map(|a| a.kind).collect();
    for k in [
        AnnotKind::Highlight,
        AnnotKind::Underline,
        AnnotKind::Note,
        AnnotKind::Ink,
    ] {
        assert!(kinds.contains(&k), "faltou {k:?}; veio {kinds:?}");
    }
    let note = annots.iter().find(|a| a.kind == AnnotKind::Note).unwrap();
    assert_eq!(note.contents, "olá nota");
    assert_eq!(reopened.all_annotations().unwrap().len(), 4);
}

#[test]
fn highlight_is_visible_when_rendered_after_reopen() {
    let (_d, path, mut doc) = setup();
    let before = doc.render(0, 1.0, false).unwrap();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&[highlight(hit)], &path).unwrap();
    let after = PdfDoc::open(&path, None)
        .unwrap()
        .render(0, 1.0, false)
        .unwrap();
    assert_ne!(before.rgb, after.rgb);
}

#[test]
fn degenerate_annotations_are_errors_not_panics() {
    let (_d, path, mut doc) = setup();
    let bad = [
        NewAnnot::Highlight {
            page: 0,
            rects: vec![],
        },
        NewAnnot::Highlight {
            page: 0,
            rects: vec![PdfRect::new(10.0, 10.0, 10.0, 10.0)],
        },
        NewAnnot::Ink {
            page: 0,
            strokes: vec![],
        },
        NewAnnot::Ink {
            page: 0,
            strokes: vec![vec![]],
        },
    ];
    for a in bad {
        assert!(
            doc.save_with(std::slice::from_ref(&a), &path).is_err(),
            "{a:?}"
        );
    }
    assert!(matches!(
        doc.save_with(
            &[NewAnnot::Note {
                page: 7,
                at: (0.0, 0.0),
                text: "x".into()
            }],
            &path
        ),
        Err(PdfError::PageOutOfRange(7))
    ));
    assert!(
        doc.annotations(0).unwrap().is_empty(),
        "nada deve ter ficado aplicado"
    );
}

#[test]
fn failed_save_keeps_original_and_document_usable() {
    let (dir, path, mut doc) = setup();
    let original = std::fs::read(&path).unwrap();
    let dest = dir.path().join("nao_existe").join("x.pdf");
    let hit = doc.search(0, "Hello").unwrap()[0];
    let res = doc.save_with(&[highlight(hit)], &dest);
    assert!(res.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original, "original intacto");
    assert!(!dest.exists());
    assert!(
        doc.annotations(0).unwrap().is_empty(),
        "mudanças em memória descartadas"
    );
    assert_eq!(doc.render(0, 1.0, false).unwrap().width, 612);
}

#[test]
fn highlight_on_a_rotated_page_lands_on_the_found_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "girada.pdf", 1, 90);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    let before = doc.render(0, 1.0, false).unwrap();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&[highlight(hit)], &path).unwrap();
    let after = PdfDoc::open(&path, None)
        .unwrap()
        .render(0, 1.0, false)
        .unwrap();
    assert_eq!((after.width, after.height), (792, 612));
    let changed = |x: u32, y: u32| {
        let i = ((y * after.width + x) * 3) as usize;
        before.rgb[i..i + 3] != after.rgb[i..i + 3]
    };
    // a maior parte dos pixels dentro do retângulo encontrado muda de cor (os pixels pretos das
    // letras continuam pretos sob o destaque); um ponto longe dele não muda
    let (mut total, mut inside) = (0, 0);
    for y in hit.y0 as u32..hit.y1 as u32 {
        for x in hit.x0 as u32..hit.x1 as u32 {
            total += 1;
            inside += usize::from(changed(x, y));
        }
    }
    assert!(
        inside * 100 > total * 40,
        "só {inside}/{total} pixels mudaram dentro de {hit:?}"
    );
    assert!(
        !changed(5, 5),
        "o destaque não deve atingir o canto da página"
    );
}

#[test]
fn strikeout_is_saved_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 0);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    let r = PdfRect::new(70.0, 90.0, 200.0, 110.0);
    doc.save_with(
        &[NewAnnot::StrikeOut {
            page: 0,
            rects: vec![r],
        }],
        &path,
    )
    .unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    assert_eq!(again.annotations(0).unwrap()[0].kind, AnnotKind::StrikeOut);
}

#[test]
fn annotations_have_stable_ids_and_style() {
    let (_d, path, mut doc) = setup();
    doc.save_with(&[highlight(PdfRect::new(70.0, 90.0, 200.0, 110.0))], &path)
        .unwrap();
    let a = doc.annotations(0).unwrap()[0].clone();
    let again = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap()[0].clone();
    assert!(a.id > 0);
    assert_eq!(a.id, again.id, "o número do objeto não muda ao reabrir");
    assert!(a.color.is_some() && a.opacity > 0.0);
    assert!(
        !a.movable() && !a.resizable(),
        "destaque fica preso ao texto"
    );
}
