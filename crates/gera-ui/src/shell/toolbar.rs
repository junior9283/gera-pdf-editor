//! Barra de ferramentas do documento e a linha de ferramentas de comentário.
use crate::doc_tab::{DocCommand, DocTab};
use crate::shell::commands::Command;
use app_state::tools::Tool;
use app_state::zoom::Fit;

pub fn show(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.button("📂").on_hover_text("Abrir (Ctrl+O)").clicked() {
            out = Some(Command::Open);
        }
        let save = if doc.is_dirty() { "💾•" } else { "💾" };
        if ui
            .add_enabled(!doc.edit.saving, egui::Button::new(save))
            .on_hover_text("Salvar (Ctrl+S)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Save));
        }
        ui.add_enabled(false, egui::Button::new("🖨"))
            .on_disabled_hover_text("Imprimir: em breve (v1.1-B)");
        if ui
            .button("🔍")
            .on_hover_text("Localizar (Ctrl+F)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Find));
        }
        ui.separator();
        doc.page_controls(ui);
        ui.separator();
        if ui.button("−").on_hover_text("Reduzir (Ctrl+-)").clicked() {
            out = Some(Command::Doc(DocCommand::ZoomOut));
        }
        if let Some(pct) = doc.zoom_percent() {
            ui.label(format!("{pct:.0}%"));
        }
        if ui.button("＋").on_hover_text("Ampliar (Ctrl+=)").clicked() {
            out = Some(Command::Doc(DocCommand::ZoomIn));
        }
        ui.menu_button("▾", |ui| {
            for cmd in [
                DocCommand::Fit(Fit::Width),
                DocCommand::Fit(Fit::Page),
                DocCommand::ZoomReset,
            ] {
                let cmd = Command::Doc(cmd);
                if ui.button(cmd.label()).clicked() {
                    out = Some(cmd);
                }
            }
        });
        ui.separator();
        doc.search_box(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(false, egui::Button::new("✍ Assinar"))
                .on_disabled_hover_text("Em breve (v3)");
            if ui
                .add(egui::Button::selectable(doc.comment_tools, "✏ Comentar"))
                .clicked()
            {
                doc.comment_tools = !doc.comment_tools;
                if !doc.comment_tools {
                    doc.edit.tool = Tool::Select; // sem a linha, a ferramenta ativa ficaria escondida
                }
            }
        });
    });
    out
}

/// Ferramentas de anotação, desfazer e refazer (aparece com "Comentar" ligado).
pub fn comment_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        for tool in Tool::ALL {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        ui.separator();
        let can_edit = !doc.edit.saving;
        if ui
            .add_enabled(can_edit && doc.is_dirty(), egui::Button::new("↶"))
            .on_hover_text("Desfazer (Ctrl+Z)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Undo));
        }
        if ui
            .add_enabled(can_edit, egui::Button::new("↷"))
            .on_hover_text("Refazer (Ctrl+Shift+Z)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Redo));
        }
    });
    out
}
