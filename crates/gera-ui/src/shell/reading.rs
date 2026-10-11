//! Barras flutuantes da tela cheia e do modo leitura (como no Acrobat).
use crate::doc_tab::{DocCommand, DocTab};
use crate::shell::commands::{hover, Command};

/// Tela cheia: ◀ página ▶ ✕ no canto inferior esquerdo. Modo leitura: página, zoom e ✕ no centro,
/// embaixo. Semitransparente, por cima das páginas.
pub fn bar(ctx: &egui::Context, doc: &mut DocTab, full_screen: bool) -> Option<Command> {
    let (anchor, offset, exit) = if full_screen {
        (
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(16.0, -16.0),
            Command::FullScreen,
        )
    } else {
        (
            egui::Align2::CENTER_BOTTOM,
            egui::vec2(0.0, -16.0),
            Command::ReadMode,
        )
    };
    let mut out = None;
    egui::Area::new(egui::Id::new("barra-flutuante"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let fill = ui.visuals().window_fill.gamma_multiply(0.85);
            egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                ui.horizontal(|ui| {
                    doc.page_controls(ui);
                    if !full_screen {
                        ui.separator();
                        for (text, cmd) in [("−", DocCommand::ZoomOut), ("+", DocCommand::ZoomIn)]
                        {
                            let cmd = Command::Doc(cmd);
                            if ui.button(text).on_hover_text(hover(&cmd)).clicked() {
                                out = Some(cmd);
                            }
                        }
                        if let Some(fit) = doc.zoom_controls(ui) {
                            out = Some(Command::Doc(fit));
                        }
                    }
                    ui.separator();
                    let label = if full_screen {
                        "Sair da tela cheia"
                    } else {
                        "Sair do modo leitura"
                    };
                    if ui.button("✕").on_hover_text(label).clicked() {
                        out = Some(exit.clone());
                    }
                });
            });
        });
    out
}
