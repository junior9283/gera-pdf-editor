//! Fechar abas com alterações pendentes: pergunta uma por vez e espera cada salvamento.
use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Cancel,
}

/// O que a casca deve fazer depois de uma resposta.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Salvar este documento e esperar o resultado.
    Save(PathBuf),
    /// Fechar este documento.
    Close(PathBuf),
    /// Parar tudo: cancelado ou falha ao salvar (as abas restantes ficam abertas).
    Abort,
}

#[derive(Debug, Default)]
pub struct CloseFlow {
    queue: VecDeque<PathBuf>,
    quit: bool,
    saving: bool,
}

impl CloseFlow {
    /// `dirty`: documentos com alterações pendentes; `quit`: sair do app no fim.
    pub fn new(dirty: Vec<PathBuf>, quit: bool) -> Self {
        Self {
            queue: dirty.into(),
            quit,
            saving: false,
        }
    }

    /// Documento sobre o qual se pergunta (ou que está sendo salvo).
    pub fn current(&self) -> Option<&PathBuf> {
        self.queue.front()
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn answer(&mut self, answer: Answer) -> Step {
        let Some(front) = self.queue.front().cloned() else {
            return Step::Abort;
        };
        match answer {
            Answer::Save => {
                self.saving = true;
                Step::Save(front)
            }
            Answer::Discard => {
                self.queue.pop_front();
                Step::Close(front)
            }
            Answer::Cancel => self.abort(),
        }
    }

    /// O salvamento do documento atual terminou (`ok`: sem pendências restantes).
    pub fn saved(&mut self, ok: bool) -> Step {
        if !self.saving {
            return Step::Abort;
        }
        self.saving = false;
        match (ok, self.queue.pop_front()) {
            (true, Some(done)) => Step::Close(done),
            _ => self.abort(),
        }
    }

    fn abort(&mut self) -> Step {
        self.queue.clear();
        self.saving = false;
        self.quit = false;
        Step::Abort
    }
}

/// Decide o que fechar já e sobre o que perguntar. `targets`: (documento, precisa confirmar).
/// Ao sair do app nada fecha antes da confirmação: cancelar a saída deixa todas as abas como estavam.
pub fn plan_close(targets: Vec<(PathBuf, bool)>, quit: bool) -> (Vec<PathBuf>, Option<CloseFlow>) {
    let (ask, clean): (Vec<_>, Vec<_>) = targets.into_iter().partition(|&(_, confirm)| confirm);
    let clean: Vec<PathBuf> = clean.into_iter().map(|(p, _)| p).collect();
    if ask.is_empty() {
        return (clean, None);
    }
    let ask = ask.into_iter().map(|(p, _)| p).collect();
    let now = if quit { Vec::new() } else { clean };
    (now, Some(CloseFlow::new(ask, quit)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: &str) -> PathBuf {
        PathBuf::from(format!("/d/{n}.pdf"))
    }

    #[test]
    fn asks_one_document_at_a_time_and_discard_moves_on() {
        let mut f = CloseFlow::new(vec![p("a"), p("b")], true);
        assert_eq!(f.current(), Some(&p("a")));
        assert_eq!(f.answer(Answer::Discard), Step::Close(p("a")));
        assert_eq!(f.current(), Some(&p("b")));
        assert_eq!(f.answer(Answer::Discard), Step::Close(p("b")));
        assert!(f.is_done() && f.quit());
    }

    #[test]
    fn save_waits_for_the_result_before_closing() {
        let mut f = CloseFlow::new(vec![p("a")], false);
        assert_eq!(f.answer(Answer::Save), Step::Save(p("a")));
        assert!(f.is_saving() && !f.is_done());
        assert_eq!(f.saved(true), Step::Close(p("a")));
        assert!(f.is_done() && !f.quit());
    }

    #[test]
    fn a_failed_save_or_cancel_stops_everything_and_keeps_the_tabs() {
        let mut f = CloseFlow::new(vec![p("a"), p("b")], true);
        f.answer(Answer::Save);
        assert_eq!(f.saved(false), Step::Abort);
        assert!(
            f.is_done() && !f.quit(),
            "nada mais é fechado e o app não sai"
        );
        let mut g = CloseFlow::new(vec![p("a"), p("b")], true);
        assert_eq!(g.answer(Answer::Cancel), Step::Abort);
        assert!(g.is_done() && !g.quit());
    }

    #[test]
    fn answers_after_the_end_do_nothing() {
        let mut f = CloseFlow::new(vec![], false);
        assert!(f.is_done());
        assert_eq!(f.answer(Answer::Discard), Step::Abort);
        assert_eq!(f.saved(true), Step::Abort);
    }

    #[test]
    fn closing_tabs_closes_clean_ones_now_and_asks_about_the_rest() {
        let (now, flow) = plan_close(vec![(p("a"), false), (p("b"), true)], false);
        assert_eq!(now, [p("a")]);
        assert_eq!(flow.as_ref().and_then(|f| f.current()), Some(&p("b")));
        let (now, flow) = plan_close(vec![(p("a"), false)], false);
        assert_eq!((now, flow.is_none()), (vec![p("a")], true));
    }

    #[test]
    fn quitting_keeps_every_tab_open_until_the_user_confirms() {
        // cancelar a saída não pode fazer sumir as abas que não tinham pendências
        let (now, flow) = plan_close(vec![(p("a"), false), (p("b"), true)], true);
        assert!(now.is_empty());
        assert!(flow.is_some_and(|f| f.quit()));
    }
}
