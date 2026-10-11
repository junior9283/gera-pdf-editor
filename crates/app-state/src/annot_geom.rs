//! Geometria da seleção de anotações: qual está sob o mouse, alças e redimensionamento.
use pdf_core::{LineEnd, PdfRect};

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

/// Como `resized`, mantendo a proporção (assinaturas e carimbos): o canto oposto fica fixo e vale
/// o eixo que o mouse puxou mais.
pub fn resized_keep_aspect(r: PdfRect, h: Handle, to: (f32, f32), min: f32) -> PdfRect {
    let (w0, h0) = (r.width().max(0.01), r.height().max(0.01));
    let (ax, ay, sx, sy) = match h {
        Handle::TopLeft => (r.x1, r.y1, -1.0, -1.0),
        Handle::TopRight => (r.x0, r.y1, 1.0, -1.0),
        Handle::BottomLeft => (r.x1, r.y0, -1.0, 1.0),
        Handle::BottomRight => (r.x0, r.y0, 1.0, 1.0),
    };
    let pull = ((to.0 - ax) * sx).max(0.0) / w0;
    let pull = pull.max(((to.1 - ay) * sy).max(0.0) / h0);
    let scale = pull.max(min / w0.min(h0));
    let (w, hh) = (w0 * scale, h0 * scale);
    PdfRect::from_corners((ax, ay), (ax + sx * w, ay + sy * hh))
}

/// Retângulo do arrasto de `a` até `b`, em qualquer direção; com `square` (Shift), o lado maior
/// vale para os dois (quadrado, ou círculo na elipse).
pub fn rect_from_drag(a: (f32, f32), b: (f32, f32), square: bool) -> PdfRect {
    let b = if square {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let side = dx.abs().max(dy.abs());
        let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
        (a.0 + side * sign(dx), a.1 + side * sign(dy))
    } else {
        b
    };
    PdfRect::from_corners(a, b)
}

/// Ponto final de uma linha presa a múltiplos de 45° (Shift), com o mesmo comprimento.
pub fn snap_45(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    let round = |v: f32| (v * 100.0).round() / 100.0;
    (
        round(a.0 + len * angle.cos()),
        round(a.1 + len * angle.sin()),
    )
}

/// Distância de `p` ao segmento de `a` a `b` (a mesma conta da borracha do pdf-core).
pub use pdf_core::dist_to_segment;

/// Ponta da linha sob `p` (a mais próxima, se as duas estiverem a menos de `size`).
pub fn line_handle_at(a: (f32, f32), b: (f32, f32), p: (f32, f32), size: f32) -> Option<LineEnd> {
    let da = (p.0 - a.0).hypot(p.1 - a.1);
    let db = (p.0 - b.0).hypot(p.1 - b.1);
    match (da <= size, db <= size) {
        (true, true) if db < da => Some(LineEnd::End),
        (true, _) => Some(LineEnd::Start),
        (false, true) => Some(LineEnd::End),
        (false, false) => None,
    }
}

/// O vértice (polígono, linhas conectadas, nuvem) mais perto de `p`, a até `size`.
pub fn vertex_at(points: &[(f32, f32)], p: (f32, f32), size: f32) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(k, &(x, y))| (k, (x - p.0).hypot(y - p.1)))
        .filter(|&(_, d)| d <= size)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// Clicar perto do primeiro ponto fecha o polígono (a partir de 3 pontos), como no Acrobat.
pub fn close_polygon(points: &[(f32, f32)], click: (f32, f32), tolerance: f32) -> bool {
    match points.first() {
        Some(&(x, y)) if points.len() >= 3 => (click.0 - x).hypot(click.1 - y) <= tolerance,
        _ => false,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_vertex_under_the_pointer_is_the_nearest_one() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (100.0, 100.0)];
        assert_eq!(vertex_at(&pts, (9.0, 1.0), 5.0), Some(1));
        assert_eq!(vertex_at(&pts, (4.0, 0.0), 5.0), Some(0), "o mais perto");
        assert_eq!(vertex_at(&pts, (50.0, 50.0), 5.0), None);
    }

    #[test]
    fn a_click_near_the_first_point_closes_the_polygon() {
        let pts = [(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)];
        assert!(
            close_polygon(&pts, (3.0, 4.0), 6.0),
            "a 5 px do primeiro ponto"
        );
        assert!(!close_polygon(&pts, (10.0, 0.0), 6.0), "longe");
        assert!(
            !close_polygon(&pts[..2], (1.0, 1.0), 6.0),
            "com menos de 3 pontos não fecha"
        );
    }
    use super::*;

    #[test]
    fn keeping_the_aspect_follows_the_wider_pull_from_the_opposite_corner() {
        let r = PdfRect::new(100.0, 100.0, 200.0, 150.0); // 2:1
        let out = resized_keep_aspect(r, Handle::BottomRight, (300.0, 160.0), 4.0);
        assert_eq!(
            (out.x0, out.y0, out.x1, out.y1),
            (100.0, 100.0, 300.0, 200.0)
        );
        let out = resized_keep_aspect(r, Handle::TopLeft, (180.0, 60.0), 4.0);
        assert_eq!((out.x1, out.y1), (200.0, 150.0), "canto oposto fixo");
        assert!((out.width() / out.height() - 2.0).abs() < 0.01);
        let tiny = resized_keep_aspect(r, Handle::BottomRight, (90.0, 90.0), 4.0);
        assert!(tiny.height() >= 4.0 && (tiny.width() / tiny.height() - 2.0).abs() < 0.01);
    }
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

    #[test]
    fn drag_rect_works_in_any_direction_and_squares_with_shift() {
        assert_eq!(
            rect_from_drag((50.0, 80.0), (10.0, 20.0), false),
            PdfRect::new(10.0, 20.0, 50.0, 80.0)
        );
        assert_eq!(
            rect_from_drag((10.0, 10.0), (50.0, 30.0), true),
            PdfRect::new(10.0, 10.0, 50.0, 50.0)
        );
        assert_eq!(
            rect_from_drag((50.0, 50.0), (10.0, 40.0), true),
            PdfRect::new(10.0, 10.0, 50.0, 50.0)
        );
    }

    #[test]
    fn snap_45_keeps_the_length_on_the_nearest_eighth() {
        assert_eq!(snap_45((0.0, 0.0), (100.0, 8.0)), (100.32, 0.0));
        let (x, y) = snap_45((0.0, 0.0), (70.0, 75.0));
        assert!((x - y).abs() < 0.02 && x > 0.0);
        assert_eq!(snap_45((10.0, 10.0), (7.0, -80.0)), (10.0, -80.05));
    }

    #[test]
    fn distance_to_segment_clamps_to_the_ends() {
        assert_eq!(dist_to_segment((5.0, 5.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
        assert_eq!(dist_to_segment((13.0, 4.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
        assert_eq!(dist_to_segment((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)), 5.0);
    }

    #[test]
    fn line_handles_are_at_the_two_ends() {
        use pdf_core::LineEnd;
        let (a, b) = ((0.0, 0.0), (100.0, 50.0));
        assert_eq!(line_handle_at(a, b, (2.0, -1.0), 4.0), Some(LineEnd::Start));
        assert_eq!(line_handle_at(a, b, (99.0, 52.0), 4.0), Some(LineEnd::End));
        assert_eq!(line_handle_at(a, b, (50.0, 25.0), 4.0), None);
    }
}
