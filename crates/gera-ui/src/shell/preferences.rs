//! Janela Preferências (Editar → Preferências…, Ctrl+K), em categorias como no Acrobat.
use app_state::layout::ViewMode;
use app_state::prefs::{DefaultZoom, Prefs, MAX_RECENT_SETTING};
use app_state::zoom;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Category {
    #[default]
    General,
    PageDisplay,
    Documents,
    Identity,
    Security,
    Appearance,
}

impl Category {
    const ALL: [Category; 6] = [
        Category::General,
        Category::PageDisplay,
        Category::Documents,
        Category::Identity,
        Category::Security,
        Category::Appearance,
    ];

    fn label(self) -> &'static str {
        match self {
            Category::General => "Geral",
            Category::PageDisplay => "Exibição de página",
            Category::Documents => "Documentos",
            Category::Identity => "Identidade",
            Category::Security => "Segurança",
            Category::Appearance => "Aparência",
        }
    }
}

/// Preferências em edição: só valem com OK.
#[derive(Debug, Clone)]
pub struct PrefsDraft {
    pub prefs: Prefs,
    category: Category,
    /// "Esquecer os documentos recentes" pedido (vale com OK).
    pub forget_recents: bool,
    /// Nome como está sendo digitado (aparado só no OK).
    pub author_text: String,
}

impl PrefsDraft {
    pub fn new(prefs: &Prefs) -> Self {
        Self {
            prefs: prefs.clone(),
            category: Category::default(),
            forget_recents: false,
            author_text: prefs.author.clone().unwrap_or_default(),
        }
    }

    /// OK: as preferências novas (dentro dos limites); Cancelar: `None`.
    pub fn finish(mut self, ok: bool) -> Option<Prefs> {
        let name = self.author_text.trim();
        self.prefs.author = (!name.is_empty()).then(|| name.to_owned());
        ok.then(|| self.prefs.sanitized())
    }

    /// Mostra a janela; `Some(true)` no OK, `Some(false)` no Cancelar ou ao fechar.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut answer = None;
        let mut open = true;
        egui::Window::new("Preferências")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                // altura fixa: o separador vertical se esticaria até a borda da tela
                ui.set_max_height(340.0);
                ui.horizontal_top(|ui| {
                    ui.set_min_height(300.0);
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for c in Category::ALL {
                            ui.selectable_value(&mut self.category, c, c.label());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(360.0);
                        self.page(ui);
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if !open {
            answer = Some(false);
        }
        answer
    }

    fn page(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.prefs;
        match self.category {
            Category::General => {
                ui.checkbox(&mut p.single_key, "Usar aceleradores de tecla única")
                    .on_hover_text("V Selecionar, H Mão, Z Zoom com marca, S Nota, U Destacar, D Desenhar, X Caixa de texto, K Carimbo");
                ui.checkbox(
                    &mut p.tools_panel,
                    "Abrir com o painel de ferramentas visível",
                );
            }
            Category::PageDisplay => {
                ui.label(
                    "Valem para os documentos novos; a visualização pedida pelo arquivo prevalece.",
                );
                egui::ComboBox::from_label("Layout de página")
                    .selected_text(p.default_mode.map_or("Automático", |m| m.label()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.default_mode, None, "Automático");
                        for m in ViewMode::ALL {
                            ui.selectable_value(&mut p.default_mode, Some(m), m.label());
                        }
                    });
                egui::ComboBox::from_label("Zoom")
                    .selected_text(zoom_label(p.default_zoom))
                    .show_ui(ui, |ui| {
                        for z in [
                            DefaultZoom::Auto,
                            DefaultZoom::FitPage,
                            DefaultZoom::FitWidth,
                            DefaultZoom::FitHeight,
                        ] {
                            ui.selectable_value(&mut p.default_zoom, z, zoom_label(z));
                        }
                        for l in zoom::LEVELS {
                            let z = DefaultZoom::Level(l);
                            ui.selectable_value(&mut p.default_zoom, z, zoom_label(z));
                        }
                    });
            }
            Category::Documents => {
                ui.checkbox(
                    &mut p.restore_views,
                    "Restaurar a última vista ao reabrir documentos",
                );
                ui.add(
                    egui::Slider::new(&mut p.max_recents, 1..=MAX_RECENT_SETTING)
                        .text("documentos na lista de recentes"),
                );
                ui.checkbox(
                    &mut self.forget_recents,
                    "Esquecer os documentos recentes (ao clicar em OK)",
                );
            }
            Category::Identity => {
                ui.label("Nome (autor dos comentários e linha dos carimbos dinâmicos):");
                ui.text_edit_singleline(&mut self.author_text);
            }
            Category::Security => {
                ui.label("Sites lembrados no aviso de segurança dos links:");
                if p.link_sites.is_empty() {
                    ui.weak("Nenhum.");
                }
                let mut forget = None;
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (site, decision) in &p.link_sites {
                            ui.horizontal(|ui| {
                                let what = match decision {
                                    app_state::link_policy::LinkDecision::Allow => "permitir",
                                    app_state::link_policy::LinkDecision::Block => "bloquear",
                                };
                                ui.monospace(crate::links::safe_display(site));
                                ui.weak(what);
                                if ui.small_button("Esquecer").clicked() {
                                    forget = Some(site.clone());
                                }
                            });
                        }
                    });
                if let Some(site) = forget {
                    p.link_sites.remove(&site);
                }
                if !p.link_sites.is_empty() && ui.button("Esquecer todos").clicked() {
                    p.link_sites.clear();
                }
            }
            Category::Appearance => {
                ui.checkbox(&mut p.dark, "Modo escuro");
            }
        }
    }
}

fn zoom_label(z: DefaultZoom) -> String {
    use crate::doc_tab::DocCommand;
    use crate::shell::commands::Command;
    let fit = |f| Command::Doc(DocCommand::Fit(f)).label();
    match z {
        DefaultZoom::Auto => "Automático".into(),
        DefaultZoom::FitPage => fit(zoom::Fit::Page),
        DefaultZoom::FitWidth => fit(zoom::Fit::Width),
        DefaultZoom::FitHeight => fit(zoom::Fit::Height),
        DefaultZoom::Level(l) => zoom::percent_text(l),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_keeps_its_spaces_while_typing() {
        let mut draft = PrefsDraft::new(&Prefs::default());
        draft.author_text = "João ".into(); // digitando: o espaço fica
        draft.author_text.push_str("Silva ");
        assert_eq!(
            draft.finish(true).unwrap().author.as_deref(),
            Some("João Silva")
        );
        let mut draft = PrefsDraft::new(&Prefs::default());
        draft.author_text = "   ".into();
        assert_eq!(draft.finish(true).unwrap().author, None);
    }

    #[test]
    fn cancel_keeps_the_old_values_and_ok_applies() {
        let old = Prefs::default();
        let mut draft = PrefsDraft::new(&old);
        draft.prefs.single_key = true;
        draft.prefs.max_recents = 7;
        assert_eq!(draft.clone().finish(false), None, "Cancelar");
        let new = draft.finish(true).expect("OK");
        assert!(new.single_key && new.max_recents == 7);
        // OK passa pelo sanitize (valor fora dos limites não entra)
        let mut draft = PrefsDraft::new(&old);
        draft.prefs.max_recents = 0;
        assert_eq!(draft.finish(true).unwrap().max_recents, 1);
    }
}
