//! Barra de abas: Início e Ferramentas fixas, documentos com ✕ e o ＋.
use crate::shell::commands::{hover, Command};
use app_state::tabs::{FIXED, HOME, TOOLS};

pub struct TabLabel {
    pub title: String,
    pub tooltip: String,
    pub dirty: bool,
}

pub enum TabAction {
    Activate(usize),
    Close(usize),
    New,
}

pub fn show(ui: &mut egui::Ui, active: usize, docs: &[TabLabel]) -> Option<TabAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (tab, name) in [(HOME, "🏠 Início"), (TOOLS, "🛠 Ferramentas")] {
            if ui.selectable_label(active == tab, name).clicked() {
                action = Some(TabAction::Activate(tab));
            }
        }
        ui.separator();
        for (n, doc) in docs.iter().enumerate() {
            let tab = FIXED + n;
            let text = if doc.dirty {
                format!("{} •", doc.title)
            } else {
                doc.title.clone()
            };
            if ui
                .selectable_label(active == tab, text)
                .on_hover_text(&doc.tooltip)
                .clicked()
            {
                action = Some(TabAction::Activate(tab));
            }
            if ui
                .small_button("×")
                .on_hover_text(hover(&Command::CloseTab))
                .clicked()
            {
                action = Some(TabAction::Close(tab));
            }
        }
        if ui
            .button("+")
            .on_hover_text(hover(&Command::Open))
            .clicked()
        {
            action = Some(TabAction::New);
        }
    });
    action
}
