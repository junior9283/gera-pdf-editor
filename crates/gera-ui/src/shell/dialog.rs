//! Peças comuns das janelas.

/// Janela com OK e Cancelar (OK desligado enquanto `body` devolve um problema, mostrado embaixo).
/// `Some(Some(_))`: OK; `Some(None)`: cancelou ou fechou.
pub fn ok_cancel_window<T>(
    ctx: &egui::Context,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> Result<T, String>,
) -> Option<Option<T>> {
    let mut out = None;
    let mut open = true;
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let result = body(ui);
            ui.separator();
            if let Err(p) = &result {
                ui.colored_label(ui.visuals().weak_text_color(), p);
            }
            let ok = result.is_ok();
            ui.horizontal(|ui| {
                if ui.add_enabled(ok, egui::Button::new("OK")).clicked() {
                    out = result.ok().map(Some);
                }
                if ui.button("Cancelar").clicked() {
                    out = Some(None);
                }
            });
        });
    if !open {
        return Some(None);
    }
    out
}
