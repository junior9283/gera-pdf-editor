//! Barra de menus no arranjo do Acrobat. Itens futuros aparecem desativados com a fase.
use crate::doc_tab::DocCommand;
use crate::shell::commands::{shortcut_of, Command};
use app_state::layout::ViewMode;
use app_state::recents::RecentFiles;
use app_state::tabs::tab_title;
use app_state::zoom::Fit;

pub struct MenuState<'a> {
    /// Modo de exibição do documento ativo (`None` no Início e em Ferramentas).
    pub doc_mode: Option<ViewMode>,
    /// "Mostrar capa sozinha" ligado no documento ativo.
    pub doc_cover: bool,
    pub dark: bool,
    pub recents: &'a RecentFiles,
    /// (aba, título) de todas as abas, na ordem da barra.
    pub tabs: Vec<(usize, String)>,
    pub active: usize,
}

fn item(ui: &mut egui::Ui, out: &mut Option<Command>, cmd: Command, enabled: bool) {
    let mut button = egui::Button::new(cmd.label());
    if let Some(sc) = shortcut_of(&cmd) {
        button = button.shortcut_text(ui.ctx().format_shortcut(&sc));
    }
    if ui.add_enabled(enabled, button).clicked() {
        *out = Some(cmd);
    }
}

fn check(ui: &mut egui::Ui, out: &mut Option<Command>, cmd: Command, checked: bool, enabled: bool) {
    if ui
        .add_enabled(enabled, egui::Button::selectable(checked, cmd.label()))
        .clicked()
    {
        *out = Some(cmd);
    }
}

fn soon(ui: &mut egui::Ui, label: &str, phase: &str) {
    ui.add_enabled(
        false,
        egui::Button::new(format!("{label}  · em breve ({phase})")),
    );
}

pub fn menu_bar(ui: &mut egui::Ui, s: &MenuState) -> Option<Command> {
    let mut out = None;
    let has_doc = s.doc_mode.is_some();
    let doc = Command::Doc;
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Arquivo", |ui| {
            item(ui, &mut out, Command::Open, true);
            ui.menu_button("Abrir recente", |ui| {
                if s.recents.items().is_empty() {
                    ui.label("Nenhum arquivo recente");
                }
                for r in s.recents.items() {
                    if ui
                        .button(tab_title(&r.path, 60))
                        .on_hover_text(r.path.display().to_string())
                        .clicked()
                    {
                        out = Some(Command::OpenRecent(r.path.clone()));
                    }
                }
            });
            ui.separator();
            item(ui, &mut out, doc(DocCommand::Save), has_doc);
            item(ui, &mut out, Command::SaveAs, has_doc);
            soon(ui, "Exportar", "v5");
            ui.separator();
            soon(ui, "Propriedades", "v1.1-B");
            soon(ui, "Imprimir", "v1.1-B");
            ui.separator();
            item(ui, &mut out, Command::CloseTab, has_doc);
            item(ui, &mut out, Command::Quit, true);
        });
        ui.menu_button("Editar", |ui| {
            item(ui, &mut out, doc(DocCommand::Undo), has_doc);
            item(ui, &mut out, doc(DocCommand::Redo), has_doc);
            ui.separator();
            soon(ui, "Copiar", "v1.1-B");
            soon(ui, "Selecionar tudo", "v1.1-B");
            ui.separator();
            item(ui, &mut out, doc(DocCommand::Find), has_doc);
            ui.separator();
            ui.menu_button("Preferências", |ui| {
                check(ui, &mut out, Command::ToggleDark, s.dark, true);
            });
        });
        ui.menu_button("Exibir", |ui| {
            ui.menu_button("Zoom", |ui| {
                for c in [
                    DocCommand::ZoomIn,
                    DocCommand::ZoomOut,
                    DocCommand::ZoomReset,
                    DocCommand::Fit(Fit::Width),
                    DocCommand::Fit(Fit::Page),
                ] {
                    item(ui, &mut out, doc(c), has_doc);
                }
            });
            ui.menu_button("Exibição de página", |ui| {
                for m in ViewMode::ALL {
                    let checked = s.doc_mode == Some(m);
                    check(ui, &mut out, doc(DocCommand::Mode(m)), checked, has_doc);
                }
                ui.separator();
                let two_up = s.doc_mode.is_some_and(|m| m.columns() == 2);
                check(
                    ui,
                    &mut out,
                    doc(DocCommand::ToggleCover),
                    s.doc_cover,
                    two_up,
                );
            });
            ui.menu_button("Girar vista", |ui| {
                item(
                    ui,
                    &mut out,
                    doc(DocCommand::Rotate { clockwise: true }),
                    has_doc,
                );
                item(
                    ui,
                    &mut out,
                    doc(DocCommand::Rotate { clockwise: false }),
                    has_doc,
                );
            });
            item(ui, &mut out, Command::FullScreen, true);
            ui.separator();
            check(ui, &mut out, Command::ToggleDark, s.dark, true);
        });
        ui.menu_button("Assinar", |ui| {
            soon(ui, "Preencher e assinar", "v3");
            soon(ui, "Assinatura digital (certificado)", "v3");
            soon(ui, "Validar assinaturas", "v3");
        });
        ui.menu_button("Janela", |ui| {
            for (tab, title) in &s.tabs {
                if ui
                    .add(egui::Button::selectable(*tab == s.active, title))
                    .clicked()
                {
                    out = Some(Command::ActivateTab(*tab));
                }
            }
            ui.separator();
            item(ui, &mut out, Command::NextTab, true);
            item(ui, &mut out, Command::PrevTab, true);
            item(ui, &mut out, Command::CloseAll, s.tabs.len() > 2);
        });
        ui.menu_button("Ajuda", |ui| {
            item(ui, &mut out, Command::Shortcuts, true);
            item(ui, &mut out, Command::About, true);
        });
    });
    out
}
