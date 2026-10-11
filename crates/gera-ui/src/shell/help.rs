//! Janelas "Atalhos de teclado" e "Sobre".
use crate::shell::commands::{shortcut_text, shortcuts};

/// Teclas tratadas fora da tabela de comandos (navegação e ferramentas na página).
pub const OTHER_KEYS: [(&str, &str); 7] = [
    ("Página anterior / próxima", "← / →"),
    ("Tela anterior / seguinte", "PageUp / PageDown"),
    ("Primeira / última página", "Home / End"),
    ("Próxima página (página única e duas páginas)", "Espaço"),
    ("Mover o comentário selecionado (10 pt com Shift)", "Setas"),
    ("Voltar à ferramenta Selecionar / sair da tela cheia", "Esc"),
    ("Ampliar ou reduzir", "Ctrl + roda"),
];

pub fn shortcuts_window(ui: &mut egui::Ui, open: &mut bool) {
    egui::Window::new("Atalhos de teclado")
        .open(open)
        .collapsible(false)
        .show(ui, |ui| {
            egui::Grid::new("atalhos").striped(true).show(ui, |ui| {
                for (sc, cmd) in shortcuts() {
                    ui.label(cmd.label());
                    ui.monospace(shortcut_text(&sc));
                    ui.end_row();
                }
                for (what, keys) in OTHER_KEYS {
                    ui.label(what);
                    ui.monospace(keys);
                    ui.end_row();
                }
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
