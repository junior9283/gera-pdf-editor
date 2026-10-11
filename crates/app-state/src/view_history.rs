//! Vista anterior e seguinte (`Alt+←` / `Alt+→`), como no Acrobat: cada salto guarda a vista de
//! onde se saiu.
use crate::zoom::{quantize, Fit};
use std::collections::VecDeque;

/// Uma vista: página, posição dentro dela (pontos, do topo) e ampliação.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewEntry {
    pub page: usize,
    pub y: f32,
    pub zoom: f32,
    pub fit: Fit,
}

impl ViewEntry {
    /// Mesma vista, desprezando diferenças de rolagem menores que um ponto.
    fn same(&self, other: &ViewEntry) -> bool {
        self.page == other.page
            && (self.y - other.y).abs() < 1.0
            && quantize(self.zoom) == quantize(other.zoom)
            && self.fit == other.fit
    }
}

const MAX_ENTRIES: usize = 100;

#[derive(Debug, Default)]
pub struct ViewHistory {
    back: VecDeque<ViewEntry>,
    forward: Vec<ViewEntry>,
}

impl ViewHistory {
    /// Guarda `current` antes de um salto. Um salto novo apaga o "seguinte".
    pub fn push(&mut self, current: ViewEntry) {
        self.forward.clear();
        if self.back.back().is_some_and(|last| last.same(&current)) {
            return;
        }
        if self.back.len() == MAX_ENTRIES {
            self.back.pop_front();
        }
        self.back.push_back(current);
    }

    /// Volta: devolve a vista anterior e guarda `current` como seguinte.
    pub fn back(&mut self, current: ViewEntry) -> Option<ViewEntry> {
        let to = self.back.pop_back()?;
        self.forward.push(current);
        Some(to)
    }

    /// Avança: o inverso de `back`.
    pub fn forward(&mut self, current: ViewEntry) -> Option<ViewEntry> {
        let to = self.forward.pop()?;
        self.back.push_back(current);
        Some(to)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(page: usize) -> ViewEntry {
        ViewEntry {
            page,
            y: 10.0,
            zoom: 1.0,
            fit: Fit::Width,
        }
    }

    #[test]
    fn history_back_and_forward_round_trip() {
        let mut h = ViewHistory::default();
        h.push(at(0)); // saltou de 0 para 5
        h.push(at(5)); // saltou de 5 para 9
        assert_eq!(h.back(at(9)), Some(at(5)));
        assert_eq!(h.back(at(5)), Some(at(0)));
        assert_eq!(h.back(at(0)), None);
        assert_eq!(h.forward(at(0)), Some(at(5)));
        assert_eq!(h.forward(at(5)), Some(at(9)));
        assert_eq!(h.forward(at(9)), None);
        assert!(h.can_back() && !h.can_forward());
    }

    #[test]
    fn push_clears_forward() {
        let mut h = ViewHistory::default();
        h.push(at(0));
        h.back(at(3));
        assert!(h.can_forward());
        h.push(at(0));
        assert!(!h.can_forward());
    }

    #[test]
    fn same_view_is_not_pushed_twice() {
        let mut h = ViewHistory::default();
        h.push(at(2));
        h.push(ViewEntry { y: 10.4, ..at(2) });
        assert_eq!(h.back(at(7)), Some(at(2)));
        assert!(!h.can_back());
    }

    #[test]
    fn history_is_capped() {
        let mut h = ViewHistory::default();
        for p in 0..250 {
            h.push(at(p));
        }
        let mut n = 0;
        let mut cur = at(999);
        while let Some(e) = h.back(cur) {
            cur = e;
            n += 1;
        }
        assert_eq!(n, MAX_ENTRIES);
        assert_eq!(cur.page, 150, "as mais antigas saem primeiro");
    }
}
