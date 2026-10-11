use pdf_core::{CancelToken, PdfRect, Request, SearchOptions, Worker};

#[derive(Debug, PartialEq, Eq)]
pub enum EnterAction {
    Search,
    Step(bool),
}

/// Estado da busca de texto: resultados chegam em lotes, por página (em ordem crescente),
/// de uma busca identificada por `id`.
#[derive(Default)]
pub struct SearchState {
    pub text: String,
    /// Palavras inteiras, maiúsculas, marcadores e comentários (⏷ ao lado do campo).
    pub options: SearchOptions,
    /// Texto da última busca iniciada (Enter de novo com o mesmo texto vai ao próximo).
    pub searched: String,
    id: u64,
    /// `Some` enquanto a busca atual está em andamento.
    cancel: Option<CancelToken>,
    hits: Vec<(i32, PdfRect)>,
    current: usize,
}

impl SearchState {
    /// Inicia uma nova busca com `text`, cancelando a anterior. Texto vazio só limpa.
    pub fn start(&mut self, worker: &Worker) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        self.id += 1;
        self.hits.clear();
        self.current = 0;
        self.searched = self.text.clone();
        if self.text.trim().is_empty() {
            return;
        }
        let cancel = CancelToken::new();
        self.cancel = Some(cancel.clone());
        worker.send(Request::Search {
            id: self.id,
            needle: self.text.clone(),
            options: self.options,
            cancel,
        });
    }

    pub fn on_hits(&mut self, id: u64, page: i32, rects: Vec<PdfRect>) {
        if id == self.id {
            self.hits.extend(rects.into_iter().map(|r| (page, r)));
        }
    }

    pub fn on_done(&mut self, id: u64) {
        if id == self.id {
            self.cancel = None;
        }
    }

    /// Resultados da página, com o índice global de cada um (`hits` está ordenado por página).
    pub fn hits_on(&self, page: i32) -> impl Iterator<Item = (usize, &PdfRect)> {
        let lo = self.hits.partition_point(|h| h.0 < page);
        let hi = self.hits.partition_point(|h| h.0 <= page);
        self.hits[lo..hi]
            .iter()
            .enumerate()
            .map(move |(i, h)| (lo + i, &h.1))
    }

    pub fn current(&self) -> Option<(i32, PdfRect)> {
        self.hits.get(self.current).copied()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Avança (ou volta) para o próximo resultado, em ordem circular.
    pub fn step(&mut self, forward: bool) -> Option<(i32, PdfRect)> {
        let n = self.hits.len();
        if n == 0 {
            return None;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
        self.current()
    }

    /// Enter no campo: busca da primeira vez (ou com texto novo); depois vai ao próximo
    /// (`Shift+Enter`, ao anterior), como no Acrobat.
    pub fn enter_action(&self, forward: bool) -> EnterAction {
        let same = !self.searched.trim().is_empty() && self.searched.trim() == self.text.trim();
        if same {
            EnterAction::Step(forward)
        } else {
            EnterAction::Search
        }
    }

    pub fn summary(&self) -> String {
        if !self.hits.is_empty() {
            format!("{} / {}", self.current + 1, self.hits.len())
        } else if self.text.trim().is_empty() {
            String::new()
        } else if self.cancel.is_none() {
            "Sem resultados".into()
        } else {
            "Buscando…".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(y: f32) -> PdfRect {
        PdfRect::new(0.0, y, 10.0, y + 5.0)
    }

    fn state() -> SearchState {
        let mut s = SearchState {
            id: 5,
            ..Default::default()
        };
        s.on_hits(5, 0, vec![r(1.0)]);
        s.on_hits(5, 2, vec![r(2.0), r(3.0)]);
        s.on_done(5);
        s
    }

    #[test]
    fn stale_ids_are_ignored() {
        let mut s = state();
        s.on_hits(4, 9, vec![r(9.0)]);
        assert_eq!(s.hits_on(9).count(), 0);
    }

    #[test]
    fn stepping_is_circular_and_summary_follows() {
        let mut s = state();
        assert_eq!(s.current().map(|c| c.0), Some(0));
        assert_eq!(s.summary(), "1 / 3");
        assert_eq!(s.step(true).map(|c| c.0), Some(2));
        s.step(true);
        assert_eq!(s.step(true).map(|c| c.0), Some(0), "volta ao início");
        assert_eq!(s.step(false).map(|c| c.0), Some(2), "volta ao fim");
    }

    #[test]
    fn hits_on_returns_only_that_page_with_global_indexes() {
        let s = state(); // página 0: 1 resultado; página 2: 2 resultados
        let on2: Vec<_> = s.hits_on(2).map(|(i, r)| (i, r.y0)).collect();
        assert_eq!(on2, [(1, 2.0), (2, 3.0)]);
        assert_eq!(s.hits_on(0).count(), 1);
        assert_eq!(s.hits_on(1).count(), 0);
        assert_eq!(s.hits_on(9).count(), 0);
    }

    #[test]
    fn starting_with_empty_text_clears_previous_results() {
        let worker = Worker::spawn(|| {});
        let mut s = state();
        s.text.clear();
        s.start(&worker);
        assert_eq!(s.hits_on(0).count() + s.hits_on(2).count(), 0);
        assert_eq!(s.summary(), "");
        assert!(s.step(true).is_none());
    }

    #[test]
    fn empty_results_report_clearly() {
        let mut s = SearchState {
            text: "xyz".into(),
            id: 1,
            ..Default::default()
        };
        s.on_done(1);
        assert_eq!(s.summary(), "Sem resultados");
        s.text.clear();
        assert_eq!(s.summary(), "", "busca vazia não mostra mensagem");
        assert!(s.current().is_none() && s.step(true).is_none());
    }

    #[test]
    fn enter_in_the_search_box_steps_after_the_first_search() {
        let mut s = state(); // "id 5": três resultados, texto vazio
        s.searched = "gera".into();
        s.text = "gera".into();
        assert_eq!(
            s.enter_action(true),
            EnterAction::Step(true),
            "mesmo texto: próximo"
        );
        assert_eq!(
            s.enter_action(false),
            EnterAction::Step(false),
            "Shift+Enter: anterior"
        );
        s.text = "outra".into();
        assert_eq!(
            s.enter_action(true),
            EnterAction::Search,
            "texto novo: busca"
        );
    }
}
