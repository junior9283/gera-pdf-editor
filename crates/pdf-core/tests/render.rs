mod common;
use common::write_sample;
use pdf_core::sample::{dark_near, darkest_pixel as darkest};
use pdf_core::{PdfDoc, PdfError, Rotation};

fn doc(rotate: i32) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, rotate), None).unwrap();
    (dir, doc)
}

#[test]
fn renders_at_scale_with_white_background_and_ink() {
    let (_d, doc) = doc(0);
    let img = doc.render(0, 1.0, false).unwrap();
    assert_eq!((img.width, img.height), (612, 792));
    assert_eq!(img.rgb.len(), 612 * 792 * 3);
    assert_eq!(&img.rgb[0..3], &[255, 255, 255]);
    assert!(
        img.rgb.iter().any(|&b| b != 255),
        "o texto deveria aparecer"
    );
    let big = doc.render(0, 2.0, false).unwrap();
    assert_eq!((big.width, big.height), (1224, 1584));
}

#[test]
fn invert_turns_background_black() {
    let (_d, doc) = doc(0);
    let img = doc.render(0, 0.5, true).unwrap();
    assert_eq!(&img.rgb[0..3], &[0, 0, 0]);
}

#[test]
fn rotated_page_renders_rotated() {
    let (_d, doc) = doc(90);
    let img = doc.render(0, 1.0, false).unwrap();
    assert_eq!((img.width, img.height), (792, 612));
}

#[test]
fn invalid_or_huge_scale_is_rejected() {
    let (_d, doc) = doc(0);
    for s in [0.0, -1.0, f32::NAN, f32::INFINITY, 1000.0] {
        assert!(
            matches!(doc.render(0, s, false), Err(PdfError::InvalidScale(_))),
            "escala {s}"
        );
    }
    assert!(matches!(
        doc.render(9, 1.0, false),
        Err(PdfError::PageOutOfRange(9))
    ));
}

#[test]
fn very_tall_or_wide_images_are_rejected_even_under_the_pixel_budget() {
    // 200x3000 pt a escala 8 = 1600x24000 px (38 Mpx, abaixo do orçamento de pixels):
    // passaria do limite de textura de qualquer GPU e derrubaria a interface.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rolo.pdf");
    std::fs::write(&path, pdf_core::sample::sample_pdf_sized(1, 0, (200, 3000))).unwrap();
    let doc = PdfDoc::open(&path, None).unwrap();
    assert!(matches!(
        doc.render(0, 8.0, false),
        Err(PdfError::InvalidScale(_))
    ));
    let ok = doc.render(0, 4.0, false).unwrap(); // 800x12000: dentro do limite
    assert_eq!((ok.width, ok.height), (800, 12000));
}

#[test]
fn max_render_scale_respects_both_the_side_and_the_pixel_limits() {
    use pdf_core::{max_render_scale, PageSize};
    let letter = PageSize {
        width: 612.0,
        height: 792.0,
    };
    let scroll = PageSize {
        width: 200.0,
        height: 3000.0,
    };
    // carta: o orçamento de pixels manda (≈ 9,08); rolo: o lado máximo manda (16384 / 3000 ≈ 5,46)
    assert!(
        (max_render_scale(&letter) - 9.08).abs() < 0.05,
        "{}",
        max_render_scale(&letter)
    );
    assert!((max_render_scale(&scroll) - 16384.0 / 3000.0).abs() < 1e-3);
    // o limite é aceito por `render`; um pouco acima não é
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rolo.pdf");
    std::fs::write(&path, pdf_core::sample::sample_pdf_sized(1, 0, (200, 3000))).unwrap();
    let doc = PdfDoc::open(&path, None).unwrap();
    let cap = max_render_scale(&scroll);
    assert!(doc.render(0, cap, false).is_ok());
    assert!(matches!(
        doc.render(0, cap * 1.01, false),
        Err(PdfError::InvalidScale(_))
    ));
    // páginas degeneradas não produzem limite inválido
    let zero = PageSize {
        width: 0.0,
        height: 0.0,
    };
    assert!(max_render_scale(&zero).is_finite() && max_render_scale(&zero) > 0.0);
}

#[test]
fn view_rotation_swaps_dimensions_and_moves_content_clockwise() {
    let (_d, doc) = doc(0);
    let r0 = doc.render_rotated(0, 0.5, false, Rotation::R0).unwrap();
    assert_eq!(
        r0,
        doc.render(0, 0.5, false).unwrap(),
        "R0 é o render de sempre"
    );
    let r90 = doc.render_rotated(0, 0.5, false, Rotation::R90).unwrap();
    let r180 = doc.render_rotated(0, 0.5, false, Rotation::R180).unwrap();
    let r270 = doc.render_rotated(0, 0.5, false, Rotation::R270).unwrap();
    assert_eq!((r0.width, r0.height), (306, 396));
    assert_eq!((r90.width, r90.height), (396, 306));
    assert_eq!((r180.width, r180.height), (306, 396));
    assert_eq!((r270.width, r270.height), (396, 306));
    let (x, y) = darkest(&r0);
    let (w, h) = (306usize, 396usize);
    assert!(
        dark_near(&r90, h - 1 - y, x),
        "90° horário: (x, y) -> (h-y, x)"
    );
    assert!(
        dark_near(&r180, w - 1 - x, h - 1 - y),
        "180°: (x, y) -> (w-x, h-y)"
    );
    assert!(dark_near(&r270, y, w - 1 - x), "270°: (x, y) -> (y, w-x)");
}

#[test]
fn rotation_steps_and_degrees() {
    assert_eq!(Rotation::default(), Rotation::R0);
    assert_eq!(Rotation::R0.cw(), Rotation::R90);
    assert_eq!(Rotation::R270.cw(), Rotation::R0);
    assert_eq!(Rotation::R0.ccw(), Rotation::R270);
    assert_eq!(Rotation::R180.degrees(), 180);
    assert!(Rotation::R90.is_sideways() && Rotation::R270.is_sideways());
    assert!(!Rotation::R0.is_sideways() && !Rotation::R180.is_sideways());
}
