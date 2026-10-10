//! Ferramentas Marca d'água e Cabeçalho e rodapé (como no Acrobat): Adicionar…, Atualizar… e
//! Remover, com as janelas e o estado do documento (há marca? quais as escolhas dela?).
use crate::doc_tab::DocTab;
use crate::shell::header_footer_dialog::HeaderFooterDialog;
use crate::shell::watermark_dialog::WatermarkDialog;
use pdf_core::{Pagination, PaginationConfig, Request};

/// Janela aberta e se ela substitui o existente (Atualizar).
pub enum PaginationDialog {
    Watermark(WatermarkDialog, bool),
    HeaderFooter(HeaderFooterDialog, bool),
}

#[derive(Default)]
pub struct PaginationTool {
    /// A ferramenta aberta.
    pub tool: Option<Pagination>,
    /// O documento tem a marca (ou o cabeçalho) e as escolhas gravadas, como o worker respondeu.
    pub present: bool,
    config: Option<PaginationConfig>,
    pub dialog: Option<PaginationDialog>,
}

impl PaginationTool {
    /// Abre a janela de Adicionar (`update`: Atualizar, com as escolhas da existente).
    pub fn open(&mut self, update: bool) {
        let Some(kind) = self.tool else {
            return;
        };
        self.dialog = Some(
            match (kind, update.then_some(self.config.as_ref()).flatten()) {
                (Pagination::Watermark, Some(PaginationConfig::Watermark(w))) => {
                    PaginationDialog::Watermark(WatermarkDialog::from_watermark(w), true)
                }
                (Pagination::Watermark, _) => {
                    PaginationDialog::Watermark(Default::default(), update)
                }
                (Pagination::HeaderFooter, Some(PaginationConfig::HeaderFooter(h))) => {
                    PaginationDialog::HeaderFooter(HeaderFooterDialog::from_header_footer(h), true)
                }
                (Pagination::HeaderFooter, _) => {
                    PaginationDialog::HeaderFooter(Default::default(), update)
                }
            },
        );
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
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let total = view.sizes.len();
        let page = view
            .sizes
            .get(self.last.current_page)
            .map_or((612.0, 792.0), |s| (s.width, s.height));
        let request = match self.pagination.dialog.as_mut() {
            Some(PaginationDialog::Watermark(d, replace)) => {
                let title = if *replace {
                    "Atualizar marca d'água"
                } else {
                    "Adicionar marca d'água"
                };
                if std::mem::take(&mut d.wants_file) {
                    self.pick_watermark_file(ctx);
                    return;
                }
                let replace = *replace;
                d.show(ctx, title, page, total).map(|out| {
                    out.map(|w| Request::AddWatermark {
                        watermark: Box::new(w),
                        replace,
                    })
                })
            }
            Some(PaginationDialog::HeaderFooter(d, replace)) => {
                let title = if *replace {
                    "Atualizar cabeçalho e rodapé"
                } else {
                    "Adicionar cabeçalho e rodapé"
                };
                let replace = *replace;
                d.show(ctx, title, page, total).map(|out| {
                    out.map(|h| Request::AddHeaderFooter {
                        header_footer: Box::new(h),
                        replace,
                    })
                })
            }
            None => None,
        };
        if let Some(outcome) = request {
            self.pagination.dialog = None;
            if let Some(r) = outcome {
                self.worker.send(r);
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
        d.texts[1] = "Título".into();
        let h = d.header_footer(1).unwrap();
        t.on_state(
            Pagination::HeaderFooter,
            true,
            Some(PaginationConfig::HeaderFooter(h)),
        );
        t.open(true);
        let Some(PaginationDialog::HeaderFooter(d, true)) = &t.dialog else {
            panic!("Atualizar substitui");
        };
        assert_eq!(d.texts[1], "Título");
        t.open(false);
        assert!(matches!(
            t.dialog,
            Some(PaginationDialog::HeaderFooter(_, false))
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
