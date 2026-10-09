//! Aba Ferramentas (grade de cartões) e a lista resumida do painel direito.
use app_state::tools_catalog::{badge, in_category, Category, Status, ToolEntry};

/// Cartão de uma ferramenta; devolve `true` se clicado (só as prontas são clicáveis).
fn card(ui: &mut egui::Ui, t: &ToolEntry) -> bool {
    let ready = t.status == Status::Ready;
    let response = egui::Frame::group(ui.style())
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(180.0);
            ui.set_height(86.0);
            ui.add_enabled_ui(ready, |ui| {
                ui.label(egui::RichText::new(format!("{} {}", t.icon, t.name)).strong());
                ui.small(t.description);
                if !ready {
                    ui.small(egui::RichText::new(badge(t)).italics());
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    ready && response.clicked()
}

pub fn hub(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for c in Category::ALL {
                ui.add_space(12.0);
                ui.heading(c.label());
                ui.horizontal_wrapped(|ui| {
                    for t in in_category(c) {
                        if card(ui, t) {
                            clicked = Some(t.id);
                        }
                    }
                });
            }
        });
    clicked
}

/// Lista compacta para o painel direito.
pub fn side_list(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut clicked = None;
    ui.heading("Ferramentas");
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for c in Category::ALL {
                ui.add_space(6.0);
                ui.small(c.label());
                for t in in_category(c) {
                    let ready = t.status == Status::Ready;
                    let button = egui::Button::new(format!("{} {}", t.icon, t.name)).frame(false);
                    if ui
                        .add_enabled(ready, button)
                        .on_disabled_hover_text(badge(t))
                        .clicked()
                    {
                        clicked = Some(t.id);
                    }
                }
            }
        });
    clicked
}
