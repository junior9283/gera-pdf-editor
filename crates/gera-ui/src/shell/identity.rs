//! Diálogo "Identidade" (como no Acrobat): o nome que assina os carimbos dinâmicos e as anotações.

/// Desenha o diálogo. Devolve `Some(true)` no OK (com nome), `Some(false)` no Cancelar.
pub fn window(ui: &mut egui::Ui, name: &mut String) -> Option<bool> {
    let mut choice = None;
    egui::Window::new("Identidade")
        .collapsible(false)
        .resizable(false)
        .show(ui, |ui| {
            ui.label("Seu nome aparece nos carimbos dinâmicos e como autor dos comentários.");
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Nome completo"));
            if !field.has_focus() && name.is_empty() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let ok = !name.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new("OK")).clicked() || (enter && ok) {
                    choice = Some(true);
                }
                if ui.button("Cancelar").clicked() {
                    choice = Some(false);
                }
            });
        });
    choice
}
