//! Ferramentas Marca d'água e Cabeçalho e rodapé (como no Acrobat): Adicionar…, Atualizar… e
//! Remover, com as janelas e o estado do documento (há marca? quais as escolhas dela?).
use crate::doc_tab::DocTab;
use crate::shell::header_footer_dialog::HeaderFooterDialog;
use crate::shell::watermark_dialog::WatermarkDialog;
use pdf_core::{Pagination, PaginationConfig, Request};

/// A janela aberta.
pub enum PaginationWindow {
    Watermark(Box<WatermarkDialog>),
    HeaderFooter(Box<HeaderFooterDialog>),
}

/// Nome da ferramenta no meio das frases ("Sem marca d'água.", "Atualizar cabeçalho e rodapé").
pub fn label(kind: Pagination) -> &'static str {
    match kind {
        Pagination::Watermark => "marca d'água",
        Pagination::HeaderFooter => "cabeçalho e rodapé",
    }
}

#[derive(Default)]
pub struct PaginationTool {
    /// A ferramenta aberta.
    pub tool: Option<Pagination>,
    /// O documento tem a marca (ou o cabeçalho) e as escolhas gravadas, como o worker respondeu.
    pub present: bool,
    config: Option<PaginationConfig>,
    /// A janela aberta e se ela substitui o existente (Atualizar).
    pub dialog: Option<(PaginationWindow, bool)>,
}

impl PaginationTool {
    /// Abre a janela de Adicionar (`update`: Atualizar, com as escolhas da existente).
    pub fn open(&mut self, update: bool) {
        let Some(kind) = self.tool else {
            return;
        };
        let current = update.then_some(self.config.as_ref()).flatten();
        let window = match kind {
            Pagination::Watermark => PaginationWindow::Watermark(Box::new(match current {
                Some(PaginationConfig::Watermark(w)) => WatermarkDialog::from_watermark(w),
                _ => Default::default(),
            })),
            Pagination::HeaderFooter => PaginationWindow::HeaderFooter(Box::new(match current {
                Some(PaginationConfig::HeaderFooter(h)) => {
                    HeaderFooterDialog::from_header_footer(h)
                }
                _ => Default::default(),
            })),
        };
        self.dialog = Some((window, update));
    }

    /// A resposta do worker sobre o documento.
    pub fn on_state(&mut self, kind: Pagination, present: bool, config: Option<PaginationConfig>) {
        if self.tool == Some(kind) {
            self.present = present;
            self.config = config;
        }
    }

    /// Outra ferramenta (ou esta, de novo): pergunta ao worker como está o documento.
    pub fn set_tool(&mut self, tool: Option<Pagination>) -> Option<Request> {
        self.tool = tool;
        self.present = false;
        self.config = None;
        self.dialog = None;
        tool.map(Request::PaginationState)
    }

    /// O documento mudou: o estado é pedido de novo.
    pub fn refresh(&self) -> Option<Request> {
        self.tool.map(Request::PaginationState)
    }
}

impl DocTab {
    /// As janelas de Adicionar/Atualizar marca d'água e cabeçalho e rodapé.
    pub(crate) fn pagination_windows(&mut self, ctx: &egui::Context) {
        let (Some(view), Some(kind)) = (self.view.as_ref(), self.pagination.tool) else {
            return;
        };
        let total = view.sizes.len();
        let page = view
            .sizes
            .get(self.last.current_page)
            .map_or((612.0, 792.0), |s| (s.width, s.height));
        let Some((window, replace)) = self.pagination.dialog.as_mut() else {
            return;
        };
        let replace = *replace;
        let title = format!(
            "{} {}",
            if replace { "Atualizar" } else { "Adicionar" },
            label(kind)
        );
        let outcome = match window {
            PaginationWindow::Watermark(d) => {
                if std::mem::take(&mut d.wants_file) {
                    self.pick_watermark_file(ctx);
                    return;
                }
                d.show(ctx, &title, page, total)
                    .map(|o| o.map(PaginationConfig::Watermark))
            }
            PaginationWindow::HeaderFooter(d) => d
                .show(ctx, &title, page, total)
                .map(|o| o.map(PaginationConfig::HeaderFooter)),
        };
        if let Some(outcome) = outcome {
            self.pagination.dialog = None;
            if let Some(config) = outcome {
                self.worker.send(Request::AddPagination {
                    config: Box::new(config),
                    replace,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_opens_with_the_current_choices_and_replaces() {
        let mut t = PaginationTool::default();
        assert!(matches!(
            t.set_tool(Some(Pagination::HeaderFooter)),
            Some(Request::PaginationState(Pagination::HeaderFooter))
        ));
        let mut d = HeaderFooterDialog::default();
        d.texts()[1] = "Título".into();
        let h = d.header_footer(1).unwrap();
        t.on_state(
            Pagination::HeaderFooter,
            true,
            Some(PaginationConfig::HeaderFooter(h)),
        );
        t.open(true);
        let Some((PaginationWindow::HeaderFooter(d), true)) = &mut t.dialog else {
            panic!("Atualizar substitui");
        };
        assert_eq!(d.texts()[1], "Título");
        t.open(false);
        assert!(matches!(
            t.dialog,
            Some((PaginationWindow::HeaderFooter(_), false))
        ));
    }

    #[test]
    fn a_state_for_another_tool_is_ignored() {
        let mut t = PaginationTool::default();
        t.set_tool(Some(Pagination::Watermark));
        t.on_state(Pagination::HeaderFooter, true, None);
        assert!(!t.present);
    }
}
