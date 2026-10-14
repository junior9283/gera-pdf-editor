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
    let t = d
        .render_tile(0, scale, false, rotation, tile, size)
        .unwrap();
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
    assert!(d
        .render_tile(0, 1.0e6, false, Rotation::R0, (0, 0), 128)
        .is_err());
    assert!(d
        .render_tile(0, 1.0, false, Rotation::R0, (0, 0), 0)
        .is_err());
}

#[test]
fn tiles_allow_zooms_above_the_full_page_cap() {
    let (_dir, d) = doc(1, 0);
    // 6400 % em tela 2× (escala 128): a página inteira seria gigante, o ladrilho não
    let t = d
        .render_tile(0, 128.0, false, Rotation::R0, (72, 70), 512)
        .unwrap();
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

#[test]
fn content_box_of_a_giant_page_does_not_allocate_the_page() {
    // 14 400 × 14 400 pt é o maior tamanho válido; com /UserUnit 75 chega a 1 milhão de pontos
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.pdf");
    let content = b"BT /F1 2000 Tf 100 100 Td (X) Tj ET";
    let pdf = format!(
        "%PDF-1.6\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 14400 14400]/UserUnit 75/Contents 4 0 R\
         /Resources<</Font<</F1 5 0 R>>>>>>endobj\n4 0 obj<</Length {}>>stream\n{}\nendstream endobj\n\
         5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n",
        content.len(),
        String::from_utf8_lossy(content)
    );
    std::fs::write(&path, pdf).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let size = d.page_size(0).unwrap();
    let t = std::time::Instant::now();
    let r = d.content_box(0).unwrap();
    assert!(
        t.elapsed().as_secs_f32() < 2.0,
        "{:?} para {size:?}",
        t.elapsed()
    );
    assert!(r.is_some(), "o X aparece mesmo na escala reduzida");
}

#[test]
fn a_band_matches_the_same_rows_of_the_full_page() {
    let (_dir, d) = doc(1, 0);
    let full = d.render(0, 1.5, false).unwrap();
    let r = d.band_renderer(0, 1.5, false).unwrap();
    assert_eq!(r.size(), (full.width, full.height));
    let (h, bgra) = r.band_bgra(100, 64).unwrap();
    assert_eq!((h, bgra.len()), (64, full.width as usize * 64 * 4));
    let row = full.width as usize * 3;
    let same = &full.rgb[100 * row..164 * row];
    // BGRA contra RGB; a suavização nas bordas do recorte pode mudar alguns pixels
    let differ = bgra
        .as_chunks::<4>()
        .0
        .iter()
        .zip(same.as_chunks::<3>().0)
        .filter(|(b, r)| {
            b[2].abs_diff(r[0]) > 40 || b[1].abs_diff(r[1]) > 40 || b[0].abs_diff(r[2]) > 40
        })
        .count();
    assert!(differ * 100 < same.len() / 3, "{differ}");
    // a última faixa sai cortada na altura da página; fora dela, erro
    assert_eq!(r.band_bgra(full.height - 10, 64).unwrap().0, 10);
    assert!(r.band_bgra(full.height, 64).is_err());
    assert!(r.band_bgra(0, 0).is_err());
    // girada: largura e altura trocam
    let turned = d.band_renderer(0, 1.5, true).unwrap();
    assert_eq!(turned.size(), (full.height, full.width));
}
