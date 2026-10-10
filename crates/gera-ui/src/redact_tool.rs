//! Ferramenta Redigir (como no Acrobat): marcar texto e áreas, Localizar texto e redigir,
//! Propriedades, Aplicar (permanente, com confirmação) e Limpar documento.
use crate::doc_tab::DocTab;
use crate::shell::watermark_dialog::ok_cancel_window;
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
}

/// Estado da ferramenta: a aparência das caixas e as janelas abertas.
pub struct RedactTool {
    pub look: RedactLook,
    pub find: Option<FindDialog>,
    /// Propriedades em edição (cópia de `look`).
    pub props: Option<RedactLook>,
    /// Confirmação do Aplicar aberta.
    pub confirm: bool,
    pub sanitize: Option<SanitizeOptions>,
}

impl Default for RedactTool {
    /// Como o Acrobat: caixa preta, sem texto sobreposto (vermelho quando houver).
    fn default() -> Self {
        Self {
            look: RedactLook {
                fill: [0, 0, 0],
                overlay: String::new(),
                overlay_color: [255, 0, 0],
            },
            find: None,
            props: None,
            confirm: false,
            sanitize: None,
        }
    }
}

impl DocTab {
    /// As janelas do Redigir.
    pub(crate) fn redact_windows(&mut self, ctx: &egui::Context) {
        let mut requests = Vec::new();
        let r = &mut self.redact;
        if let Some(find) = r.find.as_mut() {
            let mut close = false;
            egui::Window::new("Localizar texto e redigir")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut find.patterns_mode, false, "Frase");
                        ui.radio_value(&mut find.patterns_mode, true, "Padrões");
                    });
                    if find.patterns_mode {
                        ui.horizontal_wrapped(|ui| {
                            for (p, on) in RedactPattern::ALL.iter().zip(&mut find.patterns) {
                                ui.checkbox(on, p.label());
                            }
                        });
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(&mut find.phrase)
                                .hint_text("Palavra ou frase"),
                        );
                    }
                    let query = find.query();
                    if ui
                        .add_enabled(
                            query.is_some() && !find.searching,
                            egui::Button::new("Procurar"),
                        )
                        .clicked()
                    {
                        find.searching = true;
                        find.results = None;
                        requests.extend(query.map(Request::FindRedactions));
                    }
                    if find.searching {
                        ui.spinner();
                    }
                    if let Some(results) = find.results.as_mut() {
                        ui.separator();
                        ui.label(format!("{} resultado(s)", results.len()));
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                for (hit, on) in results.iter_mut() {
                                    ui.checkbox(
                                        on,
                                        format!("Página {}: {}", hit.page + 1, hit.text),
                                    );
                                }
                            });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let marks = find.marks();
                        if ui
                            .add_enabled(
                                !marks.is_empty(),
                                egui::Button::new("Marcar selecionados"),
                            )
                            .clicked()
                        {
                            requests.extend(
                                marks
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
            if close {
                r.find = None;
            }
        }
        if let Some(look) = r.props.as_mut() {
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
                Ok(look.clone())
            });
            if let Some(o) = out {
                if let Some(l) = o {
                    r.look = l;
                }
                r.props = None;
            }
        }
        if r.confirm {
            let out = ok_cancel_window(ctx, "Aplicar redação", |ui| {
                ui.label(
                    "As marcas de redação serão aplicadas em todo o documento: o texto, as \
                     imagens e os desenhos sob elas serão apagados de vez. Depois de salvar, \
                     não há como recuperar. Continuar?",
                );
                Ok(())
            });
            if let Some(o) = out {
                r.confirm = false;
                if o.is_some() {
                    requests.push(Request::ApplyRedactions(r.look.clone()));
                }
            }
        }
        if let Some(opts) = r.sanitize.as_mut() {
            let out = ok_cancel_window(ctx, "Limpar documento", |ui| {
                ui.label("Remover do documento:");
                ui.checkbox(
                    &mut opts.metadata,
                    "Metadados e propriedades (autor, título…)",
                );
                ui.checkbox(&mut opts.attachments, "Arquivos anexados");
                ui.checkbox(&mut opts.scripts, "Scripts e ações automáticas");
                ui.checkbox(&mut opts.comments, "Comentários");
                if opts.metadata || opts.attachments || opts.scripts || opts.comments {
                    Ok(*opts)
                } else {
                    Err("Escolha ao menos um item.".into())
                }
            });
            if let Some(o) = out {
                r.sanitize = None;
                requests.extend(o.map(Request::Sanitize));
            }
        }
        for req in requests {
            self.worker.send(req);
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
}
