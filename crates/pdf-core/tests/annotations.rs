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
