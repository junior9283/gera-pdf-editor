mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfError};

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
