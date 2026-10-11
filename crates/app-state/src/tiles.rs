//! Ladrilhos de alta resolução: quais recortes da página cobrem a parte visível.

/// Teto de ladrilhos por página e quadro: uma tela 4K com ladrilhos de 512 px, desalinhados, usa
/// até 9 × 6.
pub const MAX_TILES: usize = 56;

/// Ladrilhos (coluna, linha) de lado `tile` px que cobrem `visible` (x0, y0, x1, y1 em pixels
/// da página inteira renderizada, que mede `page_px`). Recortado à página; no máximo `MAX_TILES`.
pub fn visible_tiles(
    page_px: (f32, f32),
    visible: (f32, f32, f32, f32),
    tile: f32,
) -> Vec<(u32, u32)> {
    let ok = |v: f32| v.is_finite();
    if !(tile >= 1.0 && ok(page_px.0) && ok(page_px.1))
        || ![visible.0, visible.1, visible.2, visible.3]
            .into_iter()
            .all(ok)
    {
        return Vec::new();
    }
    let x0 = visible.0.max(0.0);
    let y0 = visible.1.max(0.0);
    let x1 = visible.2.min(page_px.0);
    let y1 = visible.3.min(page_px.1);
    if x0 >= x1 || y0 >= y1 {
        return Vec::new();
    }
    let (c0, c1) = ((x0 / tile) as u32, ((x1 / tile).ceil() as u32).max(1));
    let (r0, r1) = ((y0 / tile) as u32, ((y1 / tile).ceil() as u32).max(1));
    (r0..r1)
        .flat_map(|r| (c0..c1).map(move |c| (c, r)))
        .take(MAX_TILES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_tiles_cover_only_the_viewport_and_are_capped() {
        // página de 10 000 px; janela de 1000×600 a partir de (1100, 2000)
        let t = visible_tiles(
            (10_000.0, 10_000.0),
            (1100.0, 2000.0, 2100.0, 2600.0),
            512.0,
        );
        assert_eq!(t.first(), Some(&(2, 3)));
        assert_eq!(t.last(), Some(&(4, 5)));
        assert_eq!(t.len(), 9);
        // perto da borda: só o que existe
        let t = visible_tiles((600.0, 600.0), (300.0, 300.0, 2000.0, 2000.0), 512.0);
        assert_eq!(t, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
        // fora da página ou entrada ruim: nada
        assert!(visible_tiles((600.0, 600.0), (700.0, 0.0, 900.0, 100.0), 512.0).is_empty());
        assert!(visible_tiles((600.0, 600.0), (f32::NAN, 0.0, 9.0, 9.0), 512.0).is_empty());
        // janela enorme (A0 a 6400 % numa tela gigante): no máximo MAX_TILES
        let t = visible_tiles((1e7, 1e7), (0.0, 0.0, 1e5, 1e5), 512.0);
        assert_eq!(t.len(), MAX_TILES);
    }
}
