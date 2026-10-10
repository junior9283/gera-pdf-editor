//! Ferramenta Redigir (como no Acrobat): marcar texto e áreas, Localizar texto e redigir,
//! Propriedades, Aplicar (permanente, com confirmação) e Limpar documento.
use crate::doc_tab::DocTab;
use crate::shell::dialog::ok_cancel_window;
use pdf_core::{
    AnnotOp, NewAnnot, RedactHit, RedactLook, RedactPattern, RedactQuery, Request, SanitizeOptions,
};

/// Janela "Localizar texto e redigir": frase ou padrões, e os resultados para marcar.
#[derive(Default)]
pub struct FindDialog {
    patterns_mode: bool,
    phrase: String,
    patterns: [bool; RedactPattern::ALL.len()],
    /// Resultados e quais estão marcados; `None` antes de procurar.
    results: Option<Vec<(RedactHit, bool)>>,
    pub searching: bool,
}

impl FindDialog {
    /// O que procurar (nada, se a frase está vazia ou nenhum padrão foi escolhido).
    pub fn query(&self) -> Option<RedactQuery> {
        if self.patterns_mode {
            let chosen: Vec<RedactPattern> = RedactPattern::ALL
                .into_iter()
                .zip(self.patterns)
                .filter_map(|(p, on)| on.then_some(p))
                .collect();
            (!chosen.is_empty()).then_some(RedactQuery::Patterns(chosen))
        } else {
            (!self.phrase.trim().is_empty()).then(|| RedactQuery::Phrase(self.phrase.clone()))
        }
    }

    pub fn on_hits(&mut self, hits: Vec<RedactHit>) {
        self.searching = false;
        self.results = Some(hits.into_iter().map(|h| (h, true)).collect());
    }

    /// As marcas dos resultados escolhidos: uma anotação por página, com todas as caixas.
    pub fn marks(&self) -> Vec<NewAnnot> {
        let mut out: Vec<NewAnnot> = Vec::new();
        for (hit, _) in self.results.iter().flatten().filter(|(_, on)| *on) {
            match out.last_mut() {
                Some(NewAnnot::Redact { page, rects }) if *page == hit.page => {
                    rects.extend(hit.rects.iter().copied());
                }
                _ => out.push(NewAnnot::Redact {
                    page: hit.page,
                    rects: hit.rects.clone(),
                }),
            }
        }
        out
    }

    /// Desenha a janela; os pedidos vão para `requests`. Devolve se é para fechar.
    fn show(&mut self, ctx: &egui::Context, requests: &mut Vec<Request>) -> bool {
        let mut close = false;
        egui::Window::new("Localizar texto e redigir")
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.patterns_mode, false, "Frase");
                    ui.radio_value(&mut self.patterns_mode, true, "Padrões");
                });
                if self.patterns_mode {
                    ui.horizontal_wrapped(|ui| {
                        for (p, on) in RedactPattern::ALL.iter().zip(&mut self.patterns) {
                            ui.checkbox(on, p.label());
                        }
                    });
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.phrase).hint_text("Palavra ou frase"),
                    );
                }
                let query = self.query();
                if ui
                    .add_enabled(
                        query.is_some() && !self.searching,
                        egui::Button::new("Procurar"),
                    )
                    .clicked()
                {
                    self.searching = true;
                    self.results = None;
                    requests.extend(query.map(Request::FindRedactions));
                }
                if self.searching {
                    ui.spinner();
                }
                if let Some(results) = self.results.as_mut() {
                    ui.separator();
                    ui.label(format!("{} resultado(s)", results.len()));
                    // só as linhas visíveis são montadas (uma busca de padrões acha milhares)
                    let row = ui.spacing().interact_size.y;
                    egui::ScrollArea::vertical().max_height(240.0).show_rows(
                        ui,
                        row,
                        results.len(),
                        |ui, range| {
                            for (hit, on) in &mut results[range] {
                                ui.checkbox(on, format!("Página {}: {}", hit.page + 1, hit.text));
                            }
                        },
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let any = self.results.iter().flatten().any(|(_, on)| *on);
                    if ui
                        .add_enabled(any, egui::Button::new("Marcar selecionados"))
                        .clicked()
                    {
                        requests.extend(
                            self.marks()
                                .into_iter()
                                .map(|m| Request::Annot(AnnotOp::Create(m))),
                        );
                        close = true;
                    }
                    if ui.button("Fechar").clicked() {
                        close = true;
                    }
                });
            });
        close
    }
}

/// Estado da ferramenta: aberta ou não, a aparência das caixas e as janelas abertas.
#[derive(Default)]
pub struct RedactTool {
    /// A linha do Redigir está aberta.
    pub open: bool,
    pub look: RedactLook,
    pub find: Option<FindDialog>,
    /// Propriedades em edição (cópia de `look`).
    pub props: Option<RedactLook>,
    /// Confirmação do Aplicar aberta.
    pub confirm: bool,
    pub sanitize: Option<SanitizeOptions>,
    /// Salvar com marcas não aplicadas: perguntando antes.
    pub pending_save: bool,
    /// "Aplicar e salvar": a gravação espera a redação dar certo.
    pub save_after_apply: bool,
}

/// Resposta à pergunta do Salvar com marcas não aplicadas.
pub(crate) enum SaveChoice {
    ApplyAndSave,
    SaveAnyway,
    Cancel,
}

impl RedactTool {
    /// Abre ou fecha a ferramenta; fechada, as janelas dela fecham junto.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if !open {
            self.find = None;
            self.props = None;
            self.confirm = false;
            self.sanitize = None;
        }
    }

    pub fn open_props(&mut self) {
        self.props = Some(self.look.clone());
    }

    /// Desenha as janelas abertas; os pedidos ao worker vão para `requests`.
    fn windows(&mut self, ctx: &egui::Context, requests: &mut Vec<Request>) {
        if self.find.as_mut().is_some_and(|f| f.show(ctx, requests)) {
            self.find = None;
        }
        if let Some(look) = self.props.as_mut() {
            let out = ok_cancel_window(ctx, "Propriedades da redação", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Cor da caixa");
                    ui.color_edit_button_srgb(&mut look.fill);
                });
                ui.horizontal(|ui| {
                    ui.label("Texto sobreposto");
                    ui.add(
                        egui::TextEdit::singleline(&mut look.overlay).hint_text("Ex.: SIGILOSO"),
                    );
                    ui.color_edit_button_srgb(&mut look.overlay_color);
                });
                ui.checkbox(
                    &mut look.keep_touched_drawings,
                    "Preservar desenhos que só tocam a marca (bordas de tabela)",
                )
                .on_hover_text(
                    "Desligado (o padrão), todo desenho que passa sob a marca sai inteiro: \
                     texto convertido em curvas ou assinaturas não ficam recuperáveis.",
                );
                Ok(look.clone())
            });
            if let Some(o) = out {
                if let Some(l) = o {
                    self.look = l;
                }
                self.props = None;
            }
        }
        if self.confirm {
            let out = ok_cancel_window(ctx, "Aplicar redação", |ui| {
                ui.label(
                    "As marcas de redação serão aplicadas em todo o documento: o texto, as \
                     imagens e os desenhos sob elas serão apagados de vez. Depois de salvar, \
                     não há como recuperar. Continuar?",
                );
                Ok(())
            });
            if let Some(o) = out {
                self.confirm = false;
                if o.is_some() {
                    requests.push(Request::ApplyRedactions(self.look.clone()));
                }
            }
        }
        if let Some(opts) = self.sanitize.as_mut() {
            let out = ok_cancel_window(ctx, "Limpar documento", |ui| {
                ui.label("Remover do documento:");
                ui.checkbox(
                    &mut opts.metadata,
                    "Metadados e propriedades (autor, título…)",
                );
                ui.checkbox(&mut opts.attachments, "Arquivos anexados");
                ui.checkbox(&mut opts.scripts, "Scripts e ações automáticas");
                ui.checkbox(&mut opts.comments, "Comentários");
                if opts.any() {
                    Ok(*opts)
                } else {
                    Err("Escolha ao menos um item.".into())
                }
            });
            if let Some(o) = out {
                self.sanitize = None;
                requests.extend(o.map(Request::Sanitize));
            }
        }
    }
}

/// "Há marcas de redação não aplicadas": aplicar e salvar, salvar assim ou cancelar.
fn save_question(ctx: &egui::Context) -> Option<SaveChoice> {
    let mut choice = None;
    let mut open = true;
    egui::Window::new("Marcas de redação não aplicadas")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                "O documento tem marcas de redação que ainda não foram aplicadas: salvo assim, o \
                 conteúdo sob elas continua no arquivo.",
            );
            ui.horizontal(|ui| {
                if ui.button("Aplicar e salvar").clicked() {
                    choice = Some(SaveChoice::ApplyAndSave);
                }
                if ui.button("Salvar sem aplicar").clicked() {
                    choice = Some(SaveChoice::SaveAnyway);
                }
                if ui.button("Cancelar").clicked() {
                    choice = Some(SaveChoice::Cancel);
                }
            });
        });
    if !open {
        choice = Some(SaveChoice::Cancel);
    }
    choice
}

impl DocTab {
    /// As janelas do Redigir.
    pub(crate) fn redact_windows(&mut self, ctx: &egui::Context) {
        let mut requests = Vec::new();
        self.redact.windows(ctx, &mut requests);
        for req in requests {
            self.worker.send(req);
        }
        if !self.redact.pending_save {
            return;
        }
        if let Some(choice) = save_question(ctx) {
            self.answer_save(choice);
        }
    }

    pub(crate) fn answer_save(&mut self, choice: SaveChoice) {
        self.redact.pending_save = false;
        match choice {
            SaveChoice::ApplyAndSave => {
                // falha fechado: grava só quando o worker confirmar que a redação foi aplicada
                self.worker
                    .send(Request::ApplyRedactions(self.redact.look.clone()));
                self.redact.save_after_apply = true;
            }
            SaveChoice::SaveAnyway => self.save_now(),
            SaveChoice::Cancel => {}
        }
    }

    /// Resultado da redação pedida pelo "Aplicar e salvar": grava só se deu certo.
    pub(crate) fn on_redaction_outcome(&mut self, applied: bool) {
        if !std::mem::take(&mut self.redact.save_after_apply) {
            return;
        }
        if applied {
            self.save_now();
        } else {
            self.status = "A redação não foi aplicada; o documento não foi salvo.".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    fn hit(page: i32, x: f32) -> RedactHit {
        RedactHit {
            page,
            rects: vec![PdfRect::new(x, 10.0, x + 20.0, 20.0)],
            text: "x".into(),
        }
    }

    #[test]
    fn the_query_needs_a_phrase_or_a_pattern() {
        let mut f = FindDialog::default();
        assert!(f.query().is_none());
        f.phrase = "Ana".into();
        assert_eq!(f.query(), Some(RedactQuery::Phrase("Ana".into())));
        f.patterns_mode = true;
        assert!(f.query().is_none(), "nenhum padrão");
        f.patterns[0] = true;
        f.patterns[3] = true;
        assert_eq!(
            f.query(),
            Some(RedactQuery::Patterns(vec![
                RedactPattern::Cpf,
                RedactPattern::Email
            ]))
        );
    }

    #[test]
    fn chosen_results_become_one_mark_per_page() {
        let mut f = FindDialog::default();
        f.on_hits(vec![hit(0, 10.0), hit(0, 50.0), hit(1, 10.0), hit(2, 10.0)]);
        f.results.as_mut().unwrap()[3].1 = false;
        let marks = f.marks();
        assert_eq!(marks.len(), 2);
        assert!(matches!(&marks[0], NewAnnot::Redact { page: 0, rects } if rects.len() == 2));
        assert!(matches!(&marks[1], NewAnnot::Redact { page: 1, .. }));
    }

    #[test]
    fn closing_the_tool_closes_its_windows() {
        let mut t = RedactTool::default();
        t.set_open(true);
        t.find = Some(FindDialog::default());
        t.open_props();
        t.confirm = true;
        t.set_open(false);
        assert!(t.find.is_none() && t.props.is_none() && !t.confirm);
    }
}
