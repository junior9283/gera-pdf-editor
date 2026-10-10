mod common;
use common::write_sample;
use pdf_core::{
    signature, AnnotKind, AnnotOp, NewAnnot, PathCmd, PdfDoc, PdfRect, SignatureArt, VectorPath,
};

fn doc(rotate: i32) -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, rotate);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

/// Traço horizontal grosso no meio da arte (proporção 4:1).
fn bar() -> SignatureArt {
    SignatureArt::Vector {
        paths: vec![VectorPath {
            cmds: vec![PathCmd::Move(0.2, 0.5), PathCmd::Line(3.8, 0.5)],
            fill: false,
            width: 0.3,
        }],
        aspect: 4.0,
    }
}

fn place(d: &mut PdfDoc, rect: PdfRect, art: SignatureArt) {
    d.apply(AnnotOp::Create(NewAnnot::Signature {
        page: 0,
        rect,
        art,
        initials: false,
    }))
    .unwrap();
}

/// Caixa (x0, y0, x1, y1) dos pixels que ficaram escuros em relação a `base` (página renderizada
/// a 1:1 antes), se houver.
fn dark_box(d: &PdfDoc, base: &pdf_core::RenderedPage) -> Option<(u32, u32, u32, u32)> {
    let img = d.render(0, 1.0, false).unwrap();
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for y in 0..img.height {
        for x in 0..img.width {
            let i = ((y * img.width + x) * 3) as usize;
            let dark = |rgb: &[u8]| rgb[i..i + 3].iter().all(|&c| c < 80);
            if dark(&img.rgb) && !dark(&base.rgb) {
                b = Some(match b {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    b
}

#[test]
fn vector_signature_is_saved_as_a_stamp_and_drawn_in_its_box() {
    let (_d, path, mut d) = doc(0);
    let base = d.render(0, 1.0, false).unwrap();
    place(&mut d, PdfRect::new(100.0, 300.0, 300.0, 350.0), bar());
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let a = d.annotations(0).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].kind, AnnotKind::Stamp);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(13).any(|w| w == b"GeraSignature"));
    let (x0, y0, x1, y1) = dark_box(&d, &base).expect("assinatura desenhada");
    assert!(
        x0 >= 100 && x1 <= 300 && y0 >= 300 && y1 <= 350,
        "{x0} {y0} {x1} {y1}"
    );
    assert!(x1 - x0 > 150, "o traço atravessa a caixa");
}

#[test]
fn signature_stays_upright_on_a_rotated_page() {
    let (_d, _p, mut d) = doc(90);
    let base = d.render(0, 1.0, false).unwrap();
    place(&mut d, PdfRect::new(100.0, 300.0, 300.0, 350.0), bar());
    let (x0, y0, x1, y1) = dark_box(&d, &base).expect("assinatura desenhada");
    assert!(
        x1 - x0 > 3 * (y1 - y0),
        "deitada na tela: {x0} {y0} {x1} {y1}"
    );
}

#[test]
fn typed_signature_becomes_outlines_that_grow_with_the_text() {
    let (short, a1) = signature::typed("Ana").unwrap();
    let (long, a2) = signature::typed("Ana Maria").unwrap();
    assert!(!short.is_empty() && long.len() > short.len());
    assert!(a2 > a1 * 1.5, "{a1} {a2}");
    assert!(short.iter().all(|p| p.fill));
    // sem glifo na fonte: ignorado, sem erro
    let (none, _) = signature::typed("😀").unwrap();
    assert!(none.is_empty());
}

#[test]
fn image_signature_and_invalid_image() {
    let (dir, _p, mut d) = doc(0);
    let png = dir.path().join("sig.png");
    // PNG 2x1 preto gerado pelo próprio MuPDF
    let pix = mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 2, 1, false).unwrap();
    pix.save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    assert!((signature::image_aspect(&png).unwrap() - 2.0).abs() < 0.01);
    place(
        &mut d,
        PdfRect::new(100.0, 100.0, 200.0, 150.0),
        SignatureArt::Image { path: png },
    );
    assert_eq!(d.annotations(0).unwrap().len(), 1);
    let bad = dir.path().join("x.png");
    std::fs::write(&bad, b"nada").unwrap();
    let r = d.apply(AnnotOp::Create(NewAnnot::Signature {
        page: 0,
        rect: PdfRect::new(10.0, 10.0, 60.0, 30.0),
        art: SignatureArt::Image { path: bad },
        initials: true,
    }));
    assert!(r.is_err());
}

#[test]
fn moving_a_signature_keeps_its_drawing_and_undo_removes_it() {
    let (_d, _p, mut d) = doc(0);
    let base = d.render(0, 1.0, false).unwrap();
    place(&mut d, PdfRect::new(100.0, 300.0, 300.0, 350.0), bar());
    let id = d.annotations(0).unwrap()[0].id;
    d.apply(AnnotOp::Move {
        page: 0,
        id,
        dx: 0.0,
        dy: 100.0,
    })
    .unwrap();
    let (_, y0, _, y1) = dark_box(&d, &base).unwrap();
    assert!(y0 >= 400 && y1 <= 450, "{y0} {y1}");
    d.undo().unwrap();
    d.undo().unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
}

fn raw_annot(path: &std::path::Path) -> String {
    // dicionário da (única) anotação, como texto, para conferir as chaves gravadas
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    let a = page.annotations().next().unwrap();
    a.object().resolve().unwrap().unwrap().to_string()
}

#[test]
fn fill_text_is_a_borderless_typewriter_box() {
    let (_d, path, mut d) = doc(0);
    d.apply(AnnotOp::Create(NewAnnot::FillText {
        page: 0,
        at: (100.0, 100.0),
        text: "Rua das Flores, 12".into(),
        size: 14.0,
    }))
    .unwrap();
    d.save(&path).unwrap();
    let raw = raw_annot(&path);
    assert!(raw.contains("FreeTextTypeWriter"), "{raw}");
    assert!(raw.contains("14 Tf"), "{raw}");
    let a = &PdfDoc::open(&path, None).unwrap().annotations(0).unwrap()[0];
    assert_eq!(a.kind, AnnotKind::TextBox);
    assert_eq!(a.width.unwrap_or(0.0), 0.0, "sem borda");
    assert!((a.rect.x0 - 100.0).abs() < 0.5 && (a.rect.y0 - 100.0).abs() < 0.5);
    // a caixa tem a largura do texto (uma linha), como o Typewriter do Acrobat
    assert!(
        a.rect.width() > 100.0 && a.rect.width() < 160.0,
        "{:?}",
        a.rect
    );
    assert!(a.rect.height() < 25.0, "{:?}", a.rect);
}

#[test]
fn marks_are_black_and_fit_their_box() {
    use pdf_core::Mark;
    for mark in [
        Mark::Cross,
        Mark::Check,
        Mark::Circle,
        Mark::Line,
        Mark::Dot,
    ] {
        let (_d, _p, mut d) = doc(0);
        let base = d.render(0, 1.0, false).unwrap();
        d.apply(AnnotOp::Create(NewAnnot::Mark {
            page: 0,
            rect: PdfRect::new(200.0, 200.0, 224.0, 224.0),
            mark,
        }))
        .unwrap();
        let (x0, y0, x1, y1) = dark_box(&d, &base).unwrap_or_else(|| panic!("{mark:?} sem tinta"));
        assert!(
            x0 >= 198 && y0 >= 198 && x1 <= 226 && y1 <= 226,
            "{mark:?}: {x0} {y0} {x1} {y1}"
        );
    }
}

#[test]
fn editing_fill_text_resizes_the_box_to_the_new_text() {
    let (_d, _p, mut d) = doc(0);
    d.apply(AnnotOp::Create(NewAnnot::FillText {
        page: 0,
        at: (100.0, 100.0),
        text: "Ana".into(),
        size: 12.0,
    }))
    .unwrap();
    let id = d.annotations(0).unwrap()[0].id;
    d.apply(AnnotOp::SetContents {
        page: 0,
        id,
        text: "Ana Maria de Souza Albuquerque".into(),
    })
    .unwrap();
    let r = d.annotations(0).unwrap()[0].rect;
    assert!(r.width() > 150.0 && r.height() < 20.0, "{r:?}");
}

#[test]
fn bigger_fill_text_gets_a_wider_box() {
    let (_d, _p, mut d) = doc(0);
    d.apply(AnnotOp::Create(NewAnnot::FillText {
        page: 0,
        at: (100.0, 100.0),
        text: "Ana Maria".into(),
        size: 12.0,
    }))
    .unwrap();
    let a = d.annotations(0).unwrap()[0].clone();
    d.apply(AnnotOp::Font {
        page: 0,
        id: a.id,
        size: 24.0,
        color: [0.0; 3],
    })
    .unwrap();
    let r = d.annotations(0).unwrap()[0].rect;
    assert!(
        r.width() > a.rect.width() * 1.8 && r.height() < 35.0,
        "{r:?}"
    );
}
