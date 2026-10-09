use crate::zoom::{finite_positive, DEFAULT_ZOOM};
use std::ops::Range;

pub const PAGE_GAP: f32 = 12.0;

/// Como as páginas são dispostas na tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Uma página por vez.
    Single,
    #[default]
    Continuous,
    /// Duas páginas lado a lado, um par por vez.
    TwoUp,
    TwoUpContinuous,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [
        ViewMode::Single,
        ViewMode::Continuous,
        ViewMode::TwoUp,
        ViewMode::TwoUpContinuous,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Single => "Página única",
            ViewMode::Continuous => "Rolagem contínua",
            ViewMode::TwoUp => "Duas páginas",
            ViewMode::TwoUpContinuous => "Duas páginas contínuas",
        }
    }

    pub fn columns(self) -> usize {
        match self {
            ViewMode::TwoUp | ViewMode::TwoUpContinuous => 2,
            ViewMode::Single | ViewMode::Continuous => 1,
        }
    }

    /// Mostra uma linha (página ou par) por vez, em vez de rolar o documento inteiro.
    pub fn is_paged(self) -> bool {
        matches!(self, ViewMode::Single | ViewMode::TwoUp)
    }
}

#[derive(Debug, Clone, Copy)]
struct PageBox {
    x: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone)]
struct RowBox {
    top: f32,
    height: f32,
    pages: Range<usize>,
}

/// Posição de cada página (em pontos já multiplicados pelo zoom), agrupadas em linhas.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    pages: Vec<PageBox>,
    rows: Vec<RowBox>,
    max_width: f32,
}

impl Layout {
    /// Uma coluna contínua (o layout da v1).
    pub fn new(sizes: &[(f32, f32)], zoom: f32) -> Self {
        Self::with_mode(sizes, zoom, ViewMode::Continuous, false)
    }

    /// `sizes` já vêm girados (largura e altura como aparecem na tela).
    pub fn with_mode(sizes: &[(f32, f32)], zoom: f32, mode: ViewMode, cover: bool) -> Self {
        let zoom = finite_positive(zoom).unwrap_or(DEFAULT_ZOOM);
        let dim = |v: f32| finite_positive(v).unwrap_or(0.0) * zoom;
        let dims: Vec<(f32, f32)> = sizes.iter().map(|&(w, h)| (dim(w), dim(h))).collect();
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < dims.len() {
            let alone = mode.columns() == 1 || (cover && start == 0);
            let end = if alone {
                start + 1
            } else {
                (start + 2).min(dims.len())
            };
            ranges.push(start..end);
            start = end;
        }
        let row_width = |r: &Range<usize>| -> f32 {
            dims[r.clone()].iter().map(|d| d.0).sum::<f32>() + PAGE_GAP * (r.len() - 1) as f32
        };
        let max_width = ranges.iter().map(row_width).fold(0.0, f32::max);
        let mut pages = Vec::with_capacity(dims.len());
        let mut rows = Vec::with_capacity(ranges.len());
        let mut y = PAGE_GAP;
        for r in ranges {
            let height = dims[r.clone()].iter().map(|d| d.1).fold(0.0, f32::max);
            let mut x = (max_width - row_width(&r)) / 2.0;
            for &(width, h) in &dims[r.clone()] {
                pages.push(PageBox {
                    x,
                    top: y,
                    width,
                    height: h,
                });
                x += width + PAGE_GAP;
            }
            rows.push(RowBox {
                top: y,
                height,
                pages: r,
            });
            y += height + PAGE_GAP;
        }
        Self {
            pages,
            rows,
            max_width,
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn total_height(&self) -> f32 {
        self.rows
            .last()
            .map_or(PAGE_GAP, |r| r.top + r.height + PAGE_GAP)
    }
    pub fn max_width(&self) -> f32 {
        self.max_width
    }
    pub fn max_row_height(&self) -> f32 {
        self.rows.iter().map(|r| r.height).fold(0.0, f32::max)
    }
    /// Distância da borda esquerda do conteúdo (largura `max_width`) até a página.
    pub fn page_x(&self, i: usize) -> f32 {
        self.pages[i].x
    }
    pub fn page_top(&self, i: usize) -> f32 {
        self.pages[i].top
    }
    pub fn page_width(&self, i: usize) -> f32 {
        self.pages[i].width
    }
    pub fn page_height(&self, i: usize) -> f32 {
        self.pages[i].height
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    /// Índice da última linha (0 sem páginas).
    pub fn last_row(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }
    /// Linha da página `page` (a última, se a página não existir).
    pub fn row_of(&self, page: usize) -> usize {
        self.rows
            .partition_point(|r| r.pages.end <= page)
            .min(self.last_row())
    }
    pub fn row_pages(&self, row: usize) -> Range<usize> {
        self.rows.get(row).map_or(0..0, |r| r.pages.clone())
    }
    pub fn row_top(&self, row: usize) -> f32 {
        self.rows.get(row).map_or(PAGE_GAP, |r| r.top)
    }
    pub fn row_height(&self, row: usize) -> f32 {
        self.rows.get(row).map_or(0.0, |r| r.height)
    }

    /// Páginas das linhas que cruzam a janela `[scroll_y, scroll_y + viewport_h)`.
    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> Range<usize> {
        if viewport_h.is_nan() || viewport_h <= 0.0 || !scroll_y.is_finite() {
            return 0..0;
        }
        let bottom = scroll_y + viewport_h;
        let first = self.rows.partition_point(|r| r.top + r.height <= scroll_y);
        let last = self.rows.partition_point(|r| r.top < bottom);
        if first >= last {
            return 0..0;
        }
        self.rows[first].pages.start..self.rows[last - 1].pages.end
    }

    /// Primeira página da linha que contém `y`.
    pub fn page_at(&self, y: f32) -> usize {
        let row = self.rows.partition_point(|r| r.top <= y).saturating_sub(1);
        self.rows.get(row).map_or(0, |r| r.pages.start)
    }

    pub fn expand(&self, r: Range<usize>, by: usize) -> Range<usize> {
        if self.pages.is_empty() {
            return 0..0;
        }
        r.start.saturating_sub(by)..(r.end + by).min(self.pages.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ten() -> Layout {
        Layout::new(&[(100.0, 200.0); 10], 1.0)
    }

    #[test]
    fn positions_and_totals() {
        let l = ten();
        assert_eq!(l.page_count(), 10);
        assert_eq!(l.page_top(0), PAGE_GAP);
        assert_eq!(l.page_top(1), PAGE_GAP + 200.0 + PAGE_GAP);
        assert_eq!(l.total_height(), PAGE_GAP + 10.0 * (200.0 + PAGE_GAP));
        assert_eq!(l.max_width(), 100.0);
        let z = Layout::new(&[(100.0, 200.0)], 2.0);
        assert_eq!((z.page_width(0), z.page_height(0)), (200.0, 400.0));
    }

    #[test]
    fn visible_range_covers_only_intersecting_pages() {
        let l = ten(); // páginas em 12..212, 224..424, 436..636, ...
        assert_eq!(l.visible_range(0.0, 300.0), 0..2);
        assert_eq!(l.visible_range(230.0, 10.0), 1..2);
        assert!(
            l.visible_range(215.0, 5.0).is_empty(),
            "a janela 215..220 cai na lacuna entre as páginas 0 e 1"
        );
    }

    #[test]
    fn visible_range_edges_do_not_panic() {
        let l = ten();
        assert!(l.visible_range(1.0e9, 100.0).is_empty());
        assert_eq!(l.visible_range(-500.0, 100.0), 0..0);
        assert_eq!(l.visible_range(f32::NAN, 100.0), 0..0);
        assert_eq!(l.visible_range(0.0, 0.0), 0..0);
        let empty = Layout::new(&[], 1.0);
        assert_eq!(empty.visible_range(0.0, 500.0), 0..0);
        assert_eq!(empty.page_at(100.0), 0);
        assert_eq!(empty.expand(0..0, 3), 0..0);
    }

    #[test]
    fn bad_zoom_and_sizes_are_sanitized() {
        let l = Layout::new(&[(f32::NAN, -5.0), (100.0, 100.0)], f32::NAN);
        assert_eq!((l.page_width(0), l.page_height(0)), (0.0, 0.0));
        assert_eq!(l.page_width(1), 100.0);
        let z = Layout::new(&[(100.0, 100.0)], -2.0);
        assert_eq!(z.page_width(0), 100.0);
    }

    #[test]
    fn page_at_and_expand() {
        let l = ten();
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(230.0), 1);
        assert_eq!(l.page_at(1.0e9), 9);
        assert_eq!(l.expand(2..4, 1), 1..5);
        assert_eq!(l.expand(0..2, 3), 0..5);
        assert_eq!(l.expand(8..10, 3), 5..10);
    }

    #[test]
    fn two_up_puts_pairs_side_by_side_and_centers_rows() {
        let sizes = [(100.0, 200.0), (100.0, 300.0), (100.0, 200.0)];
        let l = Layout::with_mode(&sizes, 1.0, ViewMode::TwoUpContinuous, false);
        assert_eq!(l.row_count(), 2);
        assert_eq!(l.row_pages(0), 0..2);
        assert_eq!(l.row_pages(1), 2..3);
        assert_eq!(l.max_width(), 200.0 + PAGE_GAP);
        assert_eq!((l.page_x(0), l.page_x(1)), (0.0, 100.0 + PAGE_GAP));
        assert_eq!(l.page_top(0), l.page_top(1), "o par começa na mesma altura");
        assert_eq!(
            l.row_height(0),
            300.0,
            "a linha tem a altura da maior página"
        );
        assert_eq!(l.page_top(2), PAGE_GAP + 300.0 + PAGE_GAP);
        assert_eq!(
            l.page_x(2),
            (200.0 + PAGE_GAP - 100.0) / 2.0,
            "página sozinha centralizada"
        );
        assert_eq!(l.max_row_height(), 300.0);
    }

    #[test]
    fn cover_leaves_the_first_page_alone() {
        let l = Layout::with_mode(&[(100.0, 100.0); 4], 1.0, ViewMode::TwoUp, true);
        assert_eq!(l.row_count(), 3);
        assert_eq!(l.row_pages(0), 0..1);
        assert_eq!(l.row_pages(1), 1..3);
        assert_eq!(l.row_pages(2), 3..4);
        assert_eq!(l.row_of(2), 1);
        assert_eq!(
            l.row_of(99),
            2,
            "página fora do documento vai para a última linha"
        );
    }

    #[test]
    fn visible_range_in_two_columns_uses_whole_rows() {
        // par com páginas de alturas diferentes: a baixa termina antes, mas a linha continua visível
        let l = Layout::with_mode(
            &[(100.0, 300.0), (100.0, 100.0), (100.0, 100.0)],
            1.0,
            ViewMode::TwoUpContinuous,
            false,
        );
        assert_eq!(l.visible_range(200.0, 50.0), 0..2);
        assert_eq!(
            l.visible_range(PAGE_GAP + 300.0 + PAGE_GAP + 1.0, 10.0),
            2..3
        );
        assert_eq!(l.page_at(PAGE_GAP + 300.0 + PAGE_GAP + 1.0), 2);
    }

    #[test]
    fn single_column_modes_keep_the_old_layout() {
        let sizes = [(100.0, 200.0); 10];
        for mode in [ViewMode::Single, ViewMode::Continuous] {
            let l = Layout::with_mode(&sizes, 1.0, mode, false);
            assert_eq!(l.row_count(), 10);
            assert_eq!(l.page_top(1), ten().page_top(1));
            assert_eq!(l.total_height(), ten().total_height());
            assert_eq!(l.page_x(0), 0.0);
        }
    }

    #[test]
    fn empty_layout_row_queries_do_not_panic() {
        let l = Layout::with_mode(&[], 1.0, ViewMode::TwoUp, true);
        assert_eq!(l.row_count(), 0);
        assert_eq!(l.row_of(0), 0);
        assert_eq!(l.row_pages(5), 0..0);
        assert_eq!(l.row_top(5), PAGE_GAP);
        assert_eq!(l.row_height(5), 0.0);
    }

    #[test]
    fn view_modes_describe_columns_and_paging() {
        assert_eq!(ViewMode::default(), ViewMode::Continuous);
        assert_eq!(ViewMode::TwoUp.columns(), 2);
        assert!(ViewMode::Single.is_paged() && ViewMode::TwoUp.is_paged());
        assert!(!ViewMode::Continuous.is_paged() && !ViewMode::TwoUpContinuous.is_paged());
        let labels: std::collections::HashSet<_> =
            ViewMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels.len(), 4);
    }
}
