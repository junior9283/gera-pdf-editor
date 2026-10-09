use crate::zoom::{finite_positive, DEFAULT_ZOOM};
use std::ops::Range;

pub const PAGE_GAP: f32 = 12.0;

#[derive(Debug, Clone, Copy)]
struct PageBox {
    top: f32,
    width: f32,
    height: f32,
}

/// Posição vertical de cada página (em pontos já multiplicados pelo zoom), numa coluna contínua.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    pages: Vec<PageBox>,
    max_width: f32,
}

impl Layout {
    pub fn new(sizes: &[(f32, f32)], zoom: f32) -> Self {
        let zoom = finite_positive(zoom).unwrap_or(DEFAULT_ZOOM);
        let dim = |v: f32| finite_positive(v).unwrap_or(0.0) * zoom;
        let mut pages = Vec::with_capacity(sizes.len());
        let mut max_width = 0.0f32;
        let mut y = PAGE_GAP;
        for &(w, h) in sizes {
            let page = PageBox {
                top: y,
                width: dim(w),
                height: dim(h),
            };
            max_width = max_width.max(page.width);
            y += page.height + PAGE_GAP;
            pages.push(page);
        }
        Self { pages, max_width }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn total_height(&self) -> f32 {
        self.pages
            .last()
            .map_or(PAGE_GAP, |p| p.top + p.height + PAGE_GAP)
    }
    pub fn max_width(&self) -> f32 {
        self.max_width
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

    /// Páginas que cruzam a janela `[scroll_y, scroll_y + viewport_h)`.
    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> Range<usize> {
        if viewport_h.is_nan() || viewport_h <= 0.0 || !scroll_y.is_finite() {
            return 0..0;
        }
        let bottom = scroll_y + viewport_h;
        let first = self.pages.partition_point(|p| p.top + p.height <= scroll_y);
        let last = self.pages.partition_point(|p| p.top < bottom);
        first..last.max(first)
    }

    pub fn page_at(&self, y: f32) -> usize {
        self.pages.partition_point(|p| p.top <= y).saturating_sub(1)
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
        assert_eq!(l.visible_range(1.0e9, 100.0), 10..10);
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
}
