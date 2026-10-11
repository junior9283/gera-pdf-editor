//! Barras flutuantes da tela cheia e do modo leitura (como no Acrobat).
use crate::doc_tab::DocTab;
use crate::shell::commands::Command;

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
                        crate::shell::toolbar::zoom_group(ui, doc, &mut out);
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

/// O arquivo pediu tela cheia (`/PageMode /FullScreen`): pergunta antes, como o Acrobat.
#[derive(Debug, Default)]
pub struct FullScreenRequest {
    opened_at: Option<f64>,
}

impl FullScreenRequest {
    /// Resposta efetiva para um clique (`Some(sim?)`) no instante `now`: cliques antes do atraso
    /// não contam.
    fn answer(&mut self, now: f64, clicked: Option<bool>) -> Option<bool> {
        let opened = *self.opened_at.get_or_insert(now);
        clicked.filter(|_| crate::links::armed(opened, now))
    }

    /// Mostra a pergunta; devolve `Some(true)` para entrar em tela cheia.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<bool> {
        let now = ctx.input(|i| i.time);
        let ready = self.opened_at.is_some_and(|t| crate::links::armed(t, now));
        if !ready {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(crate::links::ARM_DELAY));
        }
        let mut clicked = None;
        egui::Window::new("Tela cheia")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(
                    "Este documento está tentando entrar no modo tela cheia, que esconde o menu \
                     e as barras. Para sair da tela cheia, pressione Esc.",
                );
                ui.label("Deseja entrar no modo tela cheia?");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(ready, egui::Button::new("Sim")).clicked() {
                        clicked = Some(true);
                    }
                    if ui.add_enabled(ready, egui::Button::new("Não")).clicked() {
                        clicked = Some(false);
                    }
                });
            });
        self.answer(now, clicked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_screen_asked_by_the_file_waits_for_a_deliberate_answer() {
        // o PDF não entra sozinho em tela cheia (poderia imitar outra tela): pergunta antes, e os
        // botões só valem depois do atraso, como no aviso dos links
        let mut r = FullScreenRequest::default();
        assert_eq!(
            r.answer(10.0, Some(true)),
            None,
            "clique logo ao abrir não vale"
        );
        assert_eq!(r.answer(10.3, Some(true)), None);
        assert_eq!(r.answer(10.7, None), None, "sem clique, segue perguntando");
        assert_eq!(r.answer(10.8, Some(true)), Some(true));
        let mut r = FullScreenRequest::default();
        r.answer(0.0, None);
        assert_eq!(r.answer(1.0, Some(false)), Some(false));
    }
}
