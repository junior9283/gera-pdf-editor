//! Trilho de ícones à esquerda, que abre e fecha o painel lateral.
use app_state::prefs::LeftPanel;

pub fn rail(ui: &mut egui::Ui, open: Option<LeftPanel>) -> Option<LeftPanel> {
    let mut clicked = None;
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        for p in LeftPanel::ALL {
            let icon = egui::RichText::new(p.icon()).size(18.0);
            if ui
                .add(egui::Button::selectable(open == Some(p), icon))
                .on_hover_text(p.label())
                .clicked()
            {
                clicked = Some(p);
            }
            ui.add_space(4.0);
        }
    });
    clicked
}
