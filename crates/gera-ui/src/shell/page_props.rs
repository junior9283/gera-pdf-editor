//! Propriedades da página (como no Acrobat, pelo menu das Miniaturas): a aba Ações, com os
//! gatilhos Abrir página e Fechar página.
use crate::shell::actions_editor::ActionsEditor;
use pdf_core::{ActionKind, PageTrigger};

pub struct PagePropsWindow {
    pub page: i32,
    editor: ActionsEditor<PageTrigger>,
    pages: usize,
}

impl PagePropsWindow {
    pub fn new(page: i32, actions: Vec<(PageTrigger, ActionKind)>, pages: usize) -> Self {
        Self {
            page,
            editor: ActionsEditor::new(actions, &PageTrigger::ALL),
            pages,
        }
    }

    /// `Some(Some(ações))`: OK; `Some(None)`: cancelou.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        fields: &[String],
    ) -> Option<Option<Vec<(PageTrigger, ActionKind)>>> {
        let mut out = None;
        let mut open = true;
        egui::Window::new(format!("Propriedades da página {}", self.page + 1))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.strong("Ações");
                self.editor.ui(ui, fields, self.pages);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        out = Some(Some(self.editor.list.clone()));
                    }
                    if ui.button("Cancelar").clicked() {
                        out = Some(None);
                    }
                });
            });
        if !open {
            return Some(None);
        }
        out
    }
}
