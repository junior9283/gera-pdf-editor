//! Barra de ferramentas do documento e a linha de ferramentas de comentário.
use crate::doc_tab::{DocCommand, DocTab};
use crate::shell::commands::{hover, Command};
use app_state::stamps::StampKind;
use app_state::tools::Tool;
use app_state::tools_catalog;
use app_state::zoom::Fit;

pub fn show(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        button(ui, &mut out, "📂", Command::Open, true);
        let save = if doc.is_dirty() { "💾•" } else { "💾" };
        let can_save = !doc.edit.saving;
        button(ui, &mut out, save, Command::Doc(DocCommand::Save), can_save);
        button(ui, &mut out, "⎙", Command::Print, true);
        button(ui, &mut out, "🔍", Command::Doc(DocCommand::Find), true);
        ui.separator();
        doc.page_controls(ui);
        ui.separator();
        button(ui, &mut out, "−", Command::Doc(DocCommand::ZoomOut), true);
        if let Some(pct) = doc.zoom_percent() {
            ui.label(format!("{pct:.0}%"));
        }
        button(ui, &mut out, "+", Command::Doc(DocCommand::ZoomIn), true);
        ui.menu_button("⏷", |ui| {
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
            if let Some(t) = tools_catalog::find("assinar") {
                ui.add_enabled(false, egui::Button::new("🖊 Assinar"))
                    .on_disabled_hover_text(tools_catalog::badge(t));
            }
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
        for tool in Tool::ALL.into_iter().filter(|t| *t != Tool::Stamp) {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        stamp_menu(ui, doc);
        ui.separator();
        let can_edit = !doc.edit.saving;
        let can_undo = can_edit && doc.is_dirty();
        button(ui, &mut out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
        button(ui, &mut out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
    });
    out
}

/// Ferramenta Carimbo com o menu dos carimbos, nos dois grupos do Acrobat.
fn stamp_menu(ui: &mut egui::Ui, doc: &mut DocTab) {
    let active = doc.edit.tool == Tool::Stamp;
    let label = if active {
        format!("Carimbo: {}", doc.edit.stamp.title())
    } else {
        "Carimbo".to_owned()
    };
    let button = egui::Button::selectable(active, label);
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        for (n, (group, kinds)) in [
            ("Dinâmicos", &StampKind::DYNAMIC[..]),
            ("Padrão comercial", &StampKind::STANDARD[..]),
        ]
        .into_iter()
        .enumerate()
        {
            if n > 0 {
                ui.separator();
            }
            ui.label(egui::RichText::new(group).small().strong());
            for &kind in kinds {
                let [r, g, b] = kind.color().map(|c| (c * 255.0) as u8);
                let text =
                    egui::RichText::new(kind.title()).color(egui::Color32::from_rgb(r, g, b));
                if ui.button(text).clicked() {
                    doc.edit.stamp = kind;
                    doc.edit.tool = Tool::Stamp;
                }
            }
        }
    });
}

/// Botão de ícone que dispara `cmd`, com a dica (nome e atalho) tirada da tabela de comandos.
fn button(ui: &mut egui::Ui, out: &mut Option<Command>, icon: &str, cmd: Command, enabled: bool) {
    let tip = hover(&cmd);
    if ui
        .add_enabled(enabled, egui::Button::new(icon))
        .on_hover_text(tip)
        .clicked()
    {
        *out = Some(cmd);
    }
}
