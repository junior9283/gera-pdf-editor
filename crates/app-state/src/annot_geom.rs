//! Geometria da seleção de anotações: qual está sob o mouse, alças e redimensionamento.
use pdf_core::PdfRect;

/// Canto de uma anotação selecionada, por onde ela é redimensionada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Handle {
    pub const ALL: [Handle; 4] = [
        Handle::TopLeft,
        Handle::TopRight,
        Handle::BottomLeft,
        Handle::BottomRight,
    ];

    /// Posição do canto no retângulo.
    pub fn point(self, r: PdfRect) -> (f32, f32) {
        match self {
            Handle::TopLeft => (r.x0, r.y0),
            Handle::TopRight => (r.x1, r.y0),
            Handle::BottomLeft => (r.x0, r.y1),
            Handle::BottomRight => (r.x1, r.y1),
        }
    }
}

/// Id da anotação sob `p` (a de cima, ou seja, a última da lista), com `tol` de folga.
pub fn annot_hit(rects: &[(i32, PdfRect)], p: (f32, f32), tol: f32) -> Option<i32> {
    rects
        .iter()
        .rev()
        .find(|(_, r)| {
            p.0 >= r.x0 - tol && p.0 <= r.x1 + tol && p.1 >= r.y0 - tol && p.1 <= r.y1 + tol
        })
        .map(|&(id, _)| id)
}

/// Alça de `r` sob `p` (quadrado de lado `2 * size` em volta de cada canto).
pub fn handle_at(r: PdfRect, p: (f32, f32), size: f32) -> Option<Handle> {
    Handle::ALL.into_iter().find(|h| {
        let (x, y) = h.point(r);
        (p.0 - x).abs() <= size && (p.1 - y).abs() <= size
    })
}

/// Retângulo novo ao arrastar a alça `h` até `to`: o canto oposto fica parado, nunca inverte e
/// nunca fica menor que `min` em cada lado.
pub fn resized(r: PdfRect, h: Handle, to: (f32, f32), min: f32) -> PdfRect {
    match h {
        Handle::TopLeft => PdfRect::new(to.0.min(r.x1 - min), to.1.min(r.y1 - min), r.x1, r.y1),
        Handle::TopRight => PdfRect::new(r.x0, to.1.min(r.y1 - min), to.0.max(r.x0 + min), r.y1),
        Handle::BottomLeft => PdfRect::new(to.0.min(r.x1 - min), r.y0, r.x1, to.1.max(r.y0 + min)),
        Handle::BottomRight => PdfRect::new(r.x0, r.y0, to.0.max(r.x0 + min), to.1.max(r.y0 + min)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    #[test]
    fn hit_picks_the_topmost_and_respects_tolerance() {
        let rects = [
            (1, PdfRect::new(0.0, 0.0, 100.0, 100.0)),
            (2, PdfRect::new(50.0, 50.0, 150.0, 150.0)),
        ];
        assert_eq!(
            annot_hit(&rects, (60.0, 60.0), 0.0),
            Some(2),
            "a de cima ganha"
        );
        assert_eq!(annot_hit(&rects, (10.0, 10.0), 0.0), Some(1));
        assert_eq!(
            annot_hit(&rects, (152.0, 152.0), 3.0),
            Some(2),
            "tolerância"
        );
        assert_eq!(annot_hit(&rects, (300.0, 300.0), 3.0), None);
    }

    #[test]
    fn handles_are_found_at_the_corners() {
        let r = PdfRect::new(10.0, 10.0, 110.0, 60.0);
        assert_eq!(handle_at(r, (11.0, 9.0), 4.0), Some(Handle::TopLeft));
        assert_eq!(handle_at(r, (108.0, 62.0), 4.0), Some(Handle::BottomRight));
        assert_eq!(handle_at(r, (108.0, 9.0), 4.0), Some(Handle::TopRight));
        assert_eq!(handle_at(r, (12.0, 61.0), 4.0), Some(Handle::BottomLeft));
        assert_eq!(handle_at(r, (60.0, 35.0), 4.0), None);
    }

    #[test]
    fn resizing_never_flips_or_collapses() {
        let r = PdfRect::new(10.0, 10.0, 110.0, 60.0);
        assert_eq!(
            resized(r, Handle::BottomRight, (210.0, 160.0), 5.0),
            PdfRect::new(10.0, 10.0, 210.0, 160.0)
        );
        let tiny = resized(r, Handle::BottomRight, (0.0, 0.0), 5.0);
        assert_eq!(
            (tiny.width(), tiny.height()),
            (5.0, 5.0),
            "não inverte: fica no mínimo"
        );
        assert_eq!(
            resized(r, Handle::TopLeft, (0.0, 0.0), 5.0),
            PdfRect::new(0.0, 0.0, 110.0, 60.0)
        );
    }
}
