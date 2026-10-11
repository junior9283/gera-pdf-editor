//! Barra de abas: Início e Ferramentas fixas, documentos com ✕ e o ＋.
use crate::shell::commands::{hover, Command};
use app_state::tabs::{FIXED, HOME, TOOLS};

pub struct TabLabel {
    pub title: String,
    pub tooltip: String,
    pub dirty: bool,
}

pub enum TabAction {
    Activate(usize),
    Close(usize),
    New,
}

/// Aba de documento onde páginas arrastadas de `source` caem (nenhuma: a própria, as fixas ou
/// fora das abas).
pub fn drop_target(rects: &[(usize, egui::Rect)], p: egui::Pos2, source: usize) -> Option<usize> {
    rects
        .iter()
        .find(|(_, r)| r.contains(p))
        .map(|&(t, _)| t)
        .filter(|&t| t >= FIXED && t != source)
}

/// Desenha a barra; devolve a ação e o retângulo de cada aba de documento (para soltar
/// miniaturas arrastadas).
pub fn show(
    ui: &mut egui::Ui,
    active: usize,
    docs: &[TabLabel],
) -> (Option<TabAction>, Vec<(usize, egui::Rect)>) {
    let mut action = None;
    let mut rects = Vec::new();
    ui.horizontal(|ui| {
        for (tab, name) in [(HOME, "🏠 Início"), (TOOLS, "🛠 Ferramentas")] {
            if ui.selectable_label(active == tab, name).clicked() {
                action = Some(TabAction::Activate(tab));
            }
        }
        ui.separator();
        for (n, doc) in docs.iter().enumerate() {
            let tab = FIXED + n;
            let text = if doc.dirty {
                format!("{} •", doc.title)
            } else {
                doc.title.clone()
            };
            let label = ui
                .selectable_label(active == tab, text)
                .on_hover_text(&doc.tooltip);
            rects.push((tab, label.rect));
            if label.clicked() {
                action = Some(TabAction::Activate(tab));
            }
            if ui
                .small_button("×")
                .on_hover_text(hover(&Command::CloseTab))
                .clicked()
            {
                action = Some(TabAction::Close(tab));
            }
        }
        if ui
            .button("+")
            .on_hover_text(hover(&Command::Open))
            .clicked()
        {
            action = Some(TabAction::New);
        }
    });
    (action, rects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(80.0, 20.0))
    }

    #[test]
    fn drop_on_own_or_fixed_tab_does_nothing() {
        let rects = vec![
            (HOME, rect(0.0)),
            (TOOLS, rect(100.0)),
            (2, rect(200.0)),
            (3, rect(300.0)),
        ];
        let at = |x: f32| egui::pos2(x, 10.0);
        assert_eq!(
            drop_target(&rects, at(330.0), 2),
            Some(3),
            "outra aba de documento"
        );
        assert_eq!(drop_target(&rects, at(230.0), 2), None, "a própria aba");
        assert_eq!(drop_target(&rects, at(30.0), 2), None, "Início");
        assert_eq!(drop_target(&rects, at(130.0), 2), None, "Ferramentas");
        assert_eq!(drop_target(&rects, at(900.0), 2), None, "fora das abas");
    }
}
