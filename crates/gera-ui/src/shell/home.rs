//! Aba Início: abrir arquivo e lista de recentes.
use app_state::recents::{ago, now_secs, RecentFiles};
use app_state::tabs::tab_title;
use std::path::PathBuf;

pub enum HomeAction {
    OpenDialog,
    Open(PathBuf),
    Forget(PathBuf),
}

/// `exists[i]`: o recente `i` ainda está no disco (calculado pela casca, não a cada quadro).
pub fn show(
    ui: &mut egui::Ui,
    recents: &RecentFiles,
    exists: &[bool],
    status: &str,
) -> Option<HomeAction> {
    let mut action = None;
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.heading("Gera PDF Editor");
        ui.add_space(16.0);
        let open = egui::Button::new("📂  Abrir arquivo…").min_size(egui::vec2(220.0, 40.0));
        if ui.add(open).clicked() {
            action = Some(HomeAction::OpenDialog);
        }
        if !status.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().warn_fg_color, status);
        }
    });
    ui.add_space(24.0);
    ui.heading("Recentes");
    ui.separator();
    if recents.items().is_empty() {
        ui.label("Os arquivos que você abrir aparecem aqui.");
        return action;
    }
    let now = now_secs();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("recentes")
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    for (n, r) in recents.items().iter().enumerate() {
                        let exists = exists.get(n).copied().unwrap_or(true);
                        let mut name =
                            egui::RichText::new(format!("📄 {}", tab_title(&r.path, 60)));
                        if !exists {
                            name = name.strikethrough().weak();
                        }
                        let folder = r
                            .path
                            .parent()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        if ui
                            .add_enabled(exists, egui::Button::new(name).frame(false))
                            .on_hover_text(r.path.display().to_string())
                            .on_disabled_hover_text("O arquivo não existe mais.")
                            .clicked()
                        {
                            action = Some(HomeAction::Open(r.path.clone()));
                        }
                        ui.weak(folder);
                        ui.weak(ago(now, r.opened_at));
                        if !exists && ui.small_button("Remover da lista").clicked() {
                            action = Some(HomeAction::Forget(r.path.clone()));
                        }
                        ui.end_row();
                    }
                });
        });
    action
}
