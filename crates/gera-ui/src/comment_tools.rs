//! Ferramentas de comentário da v6d: polígono, linhas conectadas e nuvem por cliques, texto
//! explicativo em duas etapas e borracha. Só a lógica (testável); o desenho fica no `edit.rs`.

use app_state::annot_geom::close_polygon;
use pdf_core::PdfRect;

/// Polígono, linhas conectadas ou nuvem em construção (cliques nos vértices).
#[derive(Debug, Clone, PartialEq)]
pub struct PolyBuilder {
    pub page: i32,
    /// Polígono e nuvem (fecham, pedem 3 vértices); linhas conectadas pedem 2.
    pub closable: bool,
    pub points: Vec<(f32, f32)>,
}

impl PolyBuilder {
    pub fn new(page: i32, closable: bool) -> Self {
        Self {
            page,
            closable,
            points: Vec::new(),
        }
    }

    /// Um clique (`double`: o segundo clique de um clique duplo, que termina). Fechar no primeiro
    /// ponto vale para polígono e nuvem. Devolve os vértices ao terminar com o mínimo de pontos;
    /// senão a construção continua.
    pub fn click(
        &mut self,
        p: (f32, f32),
        double: bool,
        tolerance: f32,
    ) -> Option<Vec<(f32, f32)>> {
        if self.closable && close_polygon(&self.points, p, tolerance) {
            return self.finish();
        }
        // o primeiro clique do duplo já pôs o vértice, a menos que os dois tenham chegado juntos
        let repeated = self
            .points
            .last()
            .is_some_and(|&(x, y)| (p.0 - x).hypot(p.1 - y) <= tolerance);
        if !(double && repeated) && self.points.len() < pdf_core::MAX_VERTICES {
            self.points.push(p);
        }
        if double {
            return self.finish();
        }
        None
    }

    /// Termina com o mínimo de vértices; senão a construção continua.
    fn finish(&mut self) -> Option<Vec<(f32, f32)>> {
        let min = if self.closable { 3 } else { 2 };
        (self.points.len() >= min).then(|| std::mem::take(&mut self.points))
    }

    /// Backspace: tira o último vértice.
    pub fn undo_last(&mut self) {
        self.points.pop();
    }
}

/// Joelho da linha de chamada: no meio do caminho entre a ponta e a caixa.
pub fn callout_knee(rect: PdfRect, tip: (f32, f32)) -> (f32, f32) {
    let edge = (tip.0.clamp(rect.x0, rect.x1), tip.1.clamp(rect.y0, rect.y1));
    ((tip.0 + edge.0) / 2.0, (tip.1 + edge.1) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_builder_closes_on_first_point_and_needs_three() {
        let mut b = PolyBuilder::new(0, true);
        for p in [(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)] {
            assert_eq!(b.click(p, false, 6.0), None);
        }
        assert_eq!(
            b.click((2.0, 2.0), false, 6.0),
            Some(vec![(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)]),
            "clicar no primeiro ponto fecha"
        );
    }

    #[test]
    fn double_click_with_two_points_creates_nothing() {
        let mut b = PolyBuilder::new(0, true);
        b.click((0.0, 0.0), false, 6.0);
        b.click((100.0, 0.0), false, 6.0);
        // o segundo clique do duplo não vira vértice
        assert_eq!(b.click((100.0, 0.0), true, 6.0), None, "polígono pede 3");
        assert_eq!(b.points.len(), 2, "a ferramenta continua com os pontos");
        let mut line = PolyBuilder::new(0, false);
        line.click((0.0, 0.0), false, 6.0);
        line.click((100.0, 0.0), false, 6.0);
        assert_eq!(
            line.click((100.0, 0.0), true, 6.0),
            Some(vec![(0.0, 0.0), (100.0, 0.0)]),
            "linhas conectadas terminam com 2"
        );
    }

    #[test]
    fn double_click_on_a_new_spot_is_the_last_vertex() {
        // os dois cliques do duplo chegam no mesmo quadro: só o duplo é visto
        let mut b = PolyBuilder::new(0, true);
        b.click((0.0, 0.0), false, 6.0);
        b.click((100.0, 0.0), false, 6.0);
        assert_eq!(
            b.click((50.0, 80.0), true, 6.0),
            Some(vec![(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)])
        );
    }

    #[test]
    fn backspace_removes_the_last_vertex() {
        let mut b = PolyBuilder::new(0, true);
        b.click((0.0, 0.0), false, 6.0);
        b.click((10.0, 0.0), false, 6.0);
        b.undo_last();
        assert_eq!(b.points, [(0.0, 0.0)]);
    }

    #[test]
    fn callout_knee_sits_between_tip_and_box() {
        let rect = PdfRect::new(200.0, 100.0, 300.0, 140.0);
        let knee = callout_knee(rect, (100.0, 220.0));
        assert!(knee.0 > 100.0 && knee.0 < 200.0, "{knee:?}");
        assert!(knee.1 > 140.0 && knee.1 < 220.0, "{knee:?}");
    }
}
