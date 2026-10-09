//! Diálogo "Salvar alterações?" do fluxo de fechamento (`app_state::close`).
use app_state::close::Answer;

/// Janela "Salvar alterações em X?". Devolve a resposta quando o usuário clica.
pub fn ask(ui: &mut egui::Ui, name: &str) -> Option<Answer> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("salvar-alteracoes")).show(ui.ctx(), |ui| {
        ui.heading("Alterações não salvas");
        ui.label(format!("Salvar as alterações em “{name}” antes de fechar?"));
        ui.horizontal(|ui| {
            if ui.button("Salvar").clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Não salvar").clicked() {
                answer = Some(Answer::Discard);
            }
            if ui.button("Cancelar").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer
}
