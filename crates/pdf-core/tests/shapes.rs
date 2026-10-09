mod common;
use common::write_sample;
use pdf_core::{AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn reopen_only(path: &std::path::Path) -> pdf_core::AnnotInfo {
    let mut v = PdfDoc::open(path, None).unwrap().annotations(0).unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

fn shape(ellipse: bool) -> NewAnnot {
    NewAnnot::Shape {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 200.0, 150.0),
        ellipse,
    }
}

fn line() -> NewAnnot {
    NewAnnot::Line {
        page: 0,
        a: (100.0, 100.0),
        b: (300.0, 200.0),
        arrow: true,
    }
}

#[test]
fn shapes_and_lines_are_created_and_read_back() {
    for (annot, kind) in [
        (shape(false), AnnotKind::Rect),
        (shape(true), AnnotKind::Ellipse),
        (line(), AnnotKind::Line),
    ] {
        let (_d, path, mut d) = doc();
        d.set_author(Some("Maria Silva".into()));
        d.apply(AnnotOp::Create(annot)).unwrap();
        d.save(&path).unwrap();
        let a = reopen_only(&path);
        assert_eq!(a.kind, kind);
        assert_eq!(a.author, "Maria Silva");
        assert_eq!(a.color, Some([1.0, 0.0, 0.0]), "vermelho padrão");
        assert_eq!(a.width, Some(1.0));
        assert_eq!(a.fill, None);
        if kind == AnnotKind::Line {
            assert_eq!(a.line, Some(((100.0, 100.0), (300.0, 200.0))));
        }
    }
}

#[test]
fn text_box_with_accents_and_lines_round_trips() {
    let (_d, path, mut d) = doc();
    d.apply(AnnotOp::Create(NewAnnot::TextBox {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        text: "Atenção: revisão\nsegunda linha".into(),
        size: 12.0,
        color: [0.0, 0.0, 0.0],
    }))
    .unwrap();
    d.save(&path).unwrap();
    let a = reopen_only(&path);
    assert_eq!(a.kind, AnnotKind::TextBox);
    assert_eq!(a.contents, "Atenção: revisão\nsegunda linha");
    assert_eq!(a.font, Some((12.0, [0.0, 0.0, 0.0])));
    assert_eq!(
        a.color,
        Some([0.0, 0.0, 0.0]),
        "a borda segue a cor do texto"
    );
    assert_eq!(a.fill, None, "sem fundo");
    assert_eq!(a.width, Some(1.0));
}

#[test]
fn empty_text_boxes_and_degenerate_shapes_are_refused() {
    let (_d, _p, mut d) = doc();
    for a in [
        NewAnnot::TextBox {
            page: 0,
            rect: PdfRect::new(1.0, 1.0, 50.0, 20.0),
            text: " ".into(),
            size: 12.0,
            color: [0.0; 3],
        },
        NewAnnot::Shape {
            page: 0,
            rect: PdfRect::new(5.0, 5.0, 6.0, 6.0),
            ellipse: false,
        },
        NewAnnot::Line {
            page: 0,
            a: (5.0, 5.0),
            b: (6.0, 6.0),
            arrow: false,
        },
    ] {
        assert!(d.apply(AnnotOp::Create(a)).is_err());
    }
    assert!(d.annotations(0).unwrap().is_empty());
}
