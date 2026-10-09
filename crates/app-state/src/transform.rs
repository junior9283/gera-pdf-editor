//! Conversão entre coordenadas da página (pontos do PDF) e da tela, com zoom e rotação da vista.
use crate::zoom::finite_positive;
use pdf_core::{PdfRect, Rotation};

/// Tamanho que a página ocupa depois de girada.
pub fn rotated(size: (f32, f32), rotation: Rotation) -> (f32, f32) {
    if rotation.is_sideways() {
        (size.1, size.0)
    } else {
        size
    }
}

/// Onde uma página está na tela e como ela foi girada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMap {
    /// Canto superior esquerdo da página na tela.
    pub origin: (f32, f32),
    pub zoom: f32,
    /// Tamanho da página em pontos, sem a rotação da vista.
    pub size: (f32, f32),
    pub rotation: Rotation,
}

impl PageMap {
    fn zoom(&self) -> f32 {
        finite_positive(self.zoom).unwrap_or(1.0)
    }

    /// Ponto da página (pontos) -> ponto na tela. Mesma convenção do render girado do `pdf-core`.
    pub fn to_screen(&self, p: (f32, f32)) -> (f32, f32) {
        let ((w, h), (x, y)) = (self.size, p);
        let (dx, dy) = match self.rotation {
            Rotation::R0 => (x, y),
            Rotation::R90 => (h - y, x),
            Rotation::R180 => (w - x, h - y),
            Rotation::R270 => (y, w - x),
        };
        let z = self.zoom();
        (self.origin.0 + dx * z, self.origin.1 + dy * z)
    }

    /// Ponto na tela -> ponto da página (pontos). Inverso de `to_screen`.
    pub fn to_page(&self, s: (f32, f32)) -> (f32, f32) {
        let (w, h) = self.size;
        let z = self.zoom();
        let (dx, dy) = ((s.0 - self.origin.0) / z, (s.1 - self.origin.1) / z);
        match self.rotation {
            Rotation::R0 => (dx, dy),
            Rotation::R90 => (dy, h - dx),
            Rotation::R180 => (w - dx, h - dy),
            Rotation::R270 => (w - dy, dx),
        }
    }

    /// Retângulo da página -> retângulo na tela, como `(mínimo, máximo)`.
    pub fn rect_to_screen(&self, r: &PdfRect) -> ((f32, f32), (f32, f32)) {
        let a = self.to_screen((r.x0, r.y0));
        let b = self.to_screen((r.x1, r.y1));
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::{sample::sample_pdf, PdfDoc, RenderedPage};

    const ALL: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

    fn map(rotation: Rotation) -> PageMap {
        PageMap {
            origin: (100.0, 50.0),
            zoom: 2.0,
            size: (612.0, 792.0),
            rotation,
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn page_to_screen_and_back_is_identity_at_every_angle() {
        for r in ALL {
            let m = map(r);
            for p in [(0.0, 0.0), (30.0, 40.0), (612.0, 792.0), (500.5, 12.25)] {
                assert!(close(m.to_page(m.to_screen(p)), p), "{r:?} {p:?}");
            }
        }
    }

    #[test]
    fn page_corners_land_where_a_clockwise_turn_puts_them() {
        // canto superior esquerdo da página
        assert!(close(
            map(Rotation::R0).to_screen((0.0, 0.0)),
            (100.0, 50.0)
        ));
        assert!(close(
            map(Rotation::R90).to_screen((0.0, 0.0)),
            (100.0 + 792.0 * 2.0, 50.0)
        ));
        assert!(close(
            map(Rotation::R180).to_screen((0.0, 0.0)),
            (100.0 + 612.0 * 2.0, 50.0 + 792.0 * 2.0)
        ));
        assert!(close(
            map(Rotation::R270).to_screen((0.0, 0.0)),
            (100.0, 50.0 + 612.0 * 2.0)
        ));
    }

    #[test]
    fn rects_come_out_normalized() {
        let r = PdfRect::new(10.0, 20.0, 30.0, 40.0);
        for rot in ALL {
            let (min, max) = map(rot).rect_to_screen(&r);
            assert!(min.0 < max.0 && min.1 < max.1, "{rot:?}");
            let area = (max.0 - min.0) * (max.1 - min.1);
            assert!((area - 20.0 * 20.0 * 4.0).abs() < 1e-2, "{rot:?}: {area}");
        }
    }

    #[test]
    fn rotated_size_swaps_sideways() {
        assert_eq!(rotated((612.0, 792.0), Rotation::R90), (792.0, 612.0));
        assert_eq!(rotated((612.0, 792.0), Rotation::R180), (612.0, 792.0));
    }

    #[test]
    fn bad_zoom_does_not_produce_non_finite_points() {
        let mut m = map(Rotation::R90);
        m.zoom = 0.0;
        let p = m.to_page((10.0, 10.0));
        assert!(p.0.is_finite() && p.1.is_finite());
    }

    fn darkest(img: &RenderedPage) -> (usize, usize) {
        let w = img.width as usize;
        let i = img
            .rgb
            .chunks(3)
            .enumerate()
            .min_by_key(|(_, p)| p.iter().map(|&b| b as u32).sum::<u32>())
            .map(|(i, _)| i)
            .unwrap();
        (i % w, i / w)
    }

    fn dark_near(img: &RenderedPage, x: usize, y: usize) -> bool {
        let (w, h) = (img.width as usize, img.height as usize);
        (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| {
            (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| {
                let i = (yy * w + xx) * 3;
                img.rgb[i..i + 3].iter().map(|&b| b as u32).sum::<u32>() < 300
            })
        })
    }

    /// O `PageMap` tem que concordar com o render girado do MuPDF; senão as anotações feitas com a
    /// vista girada cairiam fora do lugar.
    #[test]
    fn rotation_matches_the_renderer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pdf");
        std::fs::write(&path, sample_pdf(1, 0)).unwrap();
        let doc = PdfDoc::open(&path, None).unwrap();
        let zoom = 0.5;
        let flat = doc.render_rotated(0, zoom, false, Rotation::R0).unwrap();
        let (x, y) = darkest(&flat);
        let point = ((x as f32 + 0.5) / zoom, (y as f32 + 0.5) / zoom);
        for r in [Rotation::R90, Rotation::R180, Rotation::R270] {
            let img = doc.render_rotated(0, zoom, false, r).unwrap();
            let m = PageMap {
                origin: (0.0, 0.0),
                zoom,
                size: (612.0, 792.0),
                rotation: r,
            };
            let (sx, sy) = m.to_screen(point);
            assert!(
                dark_near(&img, sx as usize, sy as usize),
                "{r:?}: ({sx}, {sy})"
            );
        }
    }
}
