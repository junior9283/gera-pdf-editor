//! Janelas "Atalhos de teclado" e "Sobre".
use crate::shell::commands::shortcuts;

pub fn shortcuts_window(ui: &mut egui::Ui, open: &mut bool) {
    let ctx = ui.ctx().clone();
    egui::Window::new("Atalhos de teclado")
        .open(open)
        .collapsible(false)
        .show(ui, |ui| {
            egui::Grid::new("atalhos").striped(true).show(ui, |ui| {
                for (sc, cmd) in shortcuts() {
                    ui.label(cmd.label());
                    ui.monospace(ctx.format_shortcut(&sc));
                    ui.end_row();
                }
                ui.label("Página anterior / próxima");
                ui.monospace("PageUp / PageDown");
                ui.end_row();
                ui.label("Ampliar ou reduzir com a roda");
                ui.monospace("Ctrl + roda");
                ui.end_row();
            });
        });
}

pub fn about_window(ui: &mut egui::Ui, open: &mut bool) {
    egui::Window::new("Sobre o Gera PDF Editor")
        .open(open)
        .collapsible(false)
        .show(ui, |ui| {
            ui.heading(format!("Gera PDF Editor {}", env!("CARGO_PKG_VERSION")));
            ui.label("Editor de PDF leve e de código aberto.");
            ui.label("Licença: GNU AGPL-3.0.");
            ui.label("Renderização: MuPDF (Artifex Software, AGPL-3.0).");
            ui.label("Avisos de terceiros: THIRD_PARTY_NOTICES.md.");
        });
}
