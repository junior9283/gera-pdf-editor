use pdf_core::{sample, PdfDoc, PdfError, Rotation};

fn doc(pages: usize, rotate: i32) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.pdf");
    std::fs::write(&path, sample::sample_pdf(pages, rotate)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, d)
}

/// O ladrilho (tx, ty) de lado `size` é o mesmo recorte do render inteiro na mesma escala.
fn assert_tile_matches(d: &PdfDoc, rotation: Rotation, scale: f32, tile: (u32, u32), size: u32) {
    let full = d.render_rotated(0, scale, false, rotation).unwrap();
    let t = d.render_tile(0, scale, false, rotation, tile, size).unwrap();
    let (x0, y0) = (tile.0 * size, tile.1 * size);
    assert_eq!(t.width, size.min(full.width - x0));
    assert_eq!(t.height, size.min(full.height - y0));
    let mut differ = 0;
    for y in 0..t.height {
        for x in 0..t.width {
            for c in 0..3 {
                let a = full.rgb[(((y0 + y) * full.width + x0 + x) * 3 + c) as usize];
                let b = t.rgb[((y * t.width + x) * 3 + c) as usize];
                differ += usize::from(a.abs_diff(b) > 40);
            }
        }
    }
    // a suavização nas bordas do recorte pode mudar alguns pixels
    assert!(differ < 30, "{differ} pixels diferentes");
}

#[test]
fn tile_is_the_matching_crop_of_the_full_render() {
    let (_dir, d) = doc(1, 0);
    // a tinta do texto ("Page 1 Hello Gera" em 72,700) cai no ladrilho (0..=1, 0)
    assert_tile_matches(&d, Rotation::R0, 1.0, (1, 0), 128);
    assert_tile_matches(&d, Rotation::R0, 2.0, (2, 1), 128);
    // ladrilho da borda: cortado no tamanho da página
    assert_tile_matches(&d, Rotation::R0, 1.0, (4, 6), 128);
}

#[test]
fn rotated_tiles_match_too() {
    let (_dir, d) = doc(1, 0);
    assert_tile_matches(&d, Rotation::R90, 1.0, (5, 0), 128);
    assert_tile_matches(&d, Rotation::R270, 1.0, (0, 3), 128);
}

#[test]
fn tile_outside_the_page_is_refused() {
    let (_dir, d) = doc(1, 0);
    let err = d.render_tile(0, 1.0, false, Rotation::R0, (50, 0), 128);
    assert!(matches!(err, Err(PdfError::InvalidScale(_))), "{err:?}");
    // escala absurda também
    assert!(d.render_tile(0, 1.0e6, false, Rotation::R0, (0, 0), 128).is_err());
    assert!(d.render_tile(0, 1.0, false, Rotation::R0, (0, 0), 0).is_err());
}

#[test]
fn tiles_allow_zooms_above_the_full_page_cap() {
    let (_dir, d) = doc(1, 0);
    // 6400 % em tela 2× (escala 128): a página inteira seria gigante, o ladrilho não
    let t = d.render_tile(0, 128.0, false, Rotation::R0, (72, 70), 512).unwrap();
    assert_eq!((t.width, t.height), (512, 512));
}

#[test]
fn content_box_finds_the_ink() {
    let (_dir, d) = doc(1, 0);
    let r = d.content_box(0).unwrap().expect("tem texto");
    assert!(r.x0 > 60.0 && r.x0 < 80.0, "{r:?}");
    assert!(r.y0 > 60.0 && r.y0 < 85.0, "{r:?}");
    assert!(r.y1 > 90.0 && r.y1 < 110.0, "{r:?}");
    assert!(r.x1 > 200.0 && r.x1 < 320.0, "{r:?}");
}

#[test]
fn blank_page_has_no_content_box() {
    let (_dir, d) = doc(0, 0);
    assert!(d.content_box(0).is_err(), "sem páginas");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.pdf");
    std::fs::write(
        &path,
        b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n",
    )
    .unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.content_box(0).unwrap(), None);
}
