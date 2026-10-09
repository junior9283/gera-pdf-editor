use app_state::transform::PageMap;
use app_state::{annots::PendingAnnots, tools::Tool};
use pdf_core::{NewAnnot, PdfRect, Request, Word, Worker};
use std::collections::HashMap;
use std::path::PathBuf;

/// Ferramenta ativa, anotações pendentes e gestos em andamento.
#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    pub pending: PendingAnnots,
    pub saving: bool,
    stroke: Vec<(f32, f32)>,
    drag_start: Option<(f32, f32)>,
    /// Pedidos de palavras em andamento: qual ferramenta os pediu.
    word_jobs: HashMap<u64, Tool>,
    next_tag: u64,
    /// Nota sendo digitada: (página, posição, texto).
    note_draft: Option<(i32, (f32, f32), String)>,
}

/// Retângulo da página (pontos) -> retângulo na tela.
pub(crate) fn screen_rect(r: &PdfRect, map: &PageMap) -> egui::Rect {
    let (min, max) = map.rect_to_screen(r);
    egui::Rect::from_min_max(min.into(), max.into())
}

/// Anotação de marcação que cobre `words`, se `tool` marca texto.
fn annot_from_words(tool: Tool, page: i32, words: Vec<Word>) -> Option<NewAnnot> {
    let rects: Vec<PdfRect> = words.into_iter().map(|w| w.rect).collect();
    match tool {
        Tool::Highlight if !rects.is_empty() => Some(NewAnnot::Highlight { page, rects }),
        Tool::Underline if !rects.is_empty() => Some(NewAnnot::Underline { page, rects }),
        _ => None,
    }
}

fn selection_text(words: &[Word]) -> String {
    words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn paint_stroke(painter: &egui::Painter, points: &[(f32, f32)], map: &PageMap) {
    let pts = points.iter().map(|&p| map.to_screen(p).into()).collect();
    painter.line(pts, egui::Stroke::new(2.0, egui::Color32::RED));
}

impl EditState {
    /// Guarda uma anotação nova. Recusa durante o salvamento: o que for criado agora seria
    /// descartado quando a resposta `Saved` limpar as pendentes.
    pub fn add(&mut self, annot: NewAnnot) -> bool {
        !self.saving && self.pending.add(annot)
    }

    pub fn undo(&mut self) -> bool {
        !self.saving && self.pending.undo()
    }

    pub fn redo(&mut self) -> bool {
        !self.saving && self.pending.redo()
    }

    /// Descarta anotações pendentes e gestos (ao abrir outro documento ou depois de salvar).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.saving = false;
        self.stroke.clear();
        self.drag_start = None;
        self.note_draft = None;
    }

    /// Trata o mouse sobre uma página visível e desenha as anotações pendentes dela.
    pub fn on_page(
        &mut self,
        ui: &mut egui::Ui,
        worker: &Worker,
        page: i32,
        rect: egui::Rect,
        map: &PageMap,
    ) {
        let response = ui.interact(
            rect,
            ui.id().with(("page", page)),
            egui::Sense::click_and_drag(),
        );
        let pointer = response
            .interact_pointer_pos()
            .map(|p| map.to_page(p.into()));
        if self.saving {
            // durante o salvamento só mostra as pendentes: gestos novos seriam perdidos
            self.paint_pending(ui.painter(), page, map);
            return;
        }
        match self.tool {
            Tool::Select | Tool::Highlight | Tool::Underline => {
                self.drag_select(ui, worker, &response, pointer, page, map)
            }
            Tool::Note => {
                if let (true, Some(at)) = (response.clicked(), pointer) {
                    self.note_draft = Some((page, at, String::new()));
                }
            }
            Tool::Ink => self.draw_ink(ui, &response, pointer, page, map),
        }
        self.paint_pending(ui.painter(), page, map);
    }

    /// Arrastar um retângulo: ao soltar, pede ao worker as palavras dentro dele.
    fn drag_select(
        &mut self,
        ui: &egui::Ui,
        worker: &Worker,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        page: i32,
        map: &PageMap,
    ) {
        if response.drag_started() {
            self.drag_start = pointer;
        }
        let (Some(start), Some(now)) = (self.drag_start, pointer) else {
            return;
        };
        let area = PdfRect::from_corners(start, now);
        ui.painter().rect_stroke(
            screen_rect(&area, map),
            0.0,
            egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
            egui::StrokeKind::Inside,
        );
        if response.drag_stopped() {
            self.drag_start = None;
            self.next_tag += 1;
            self.word_jobs.insert(self.next_tag, self.tool);
            worker.send(Request::Words {
                tag: self.next_tag,
                page,
                area,
            });
        }
    }

    /// Desenho à mão livre: acumula os pontos e, ao soltar, guarda o traço.
    fn draw_ink(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        page: i32,
        map: &PageMap,
    ) {
        if response.drag_started() {
            self.stroke.clear();
        }
        if response.dragged() {
            self.stroke.extend(pointer);
        }
        if response.drag_stopped() {
            let stroke = std::mem::take(&mut self.stroke);
            self.add(NewAnnot::Ink {
                page,
                strokes: vec![stroke],
            });
        }
        if self.stroke.len() > 1 {
            paint_stroke(ui.painter(), &self.stroke, map);
        }
    }

    fn paint_pending(&self, painter: &egui::Painter, page: i32, map: &PageMap) {
        for item in self.pending.items().iter().filter(|a| a.page() == page) {
            match item {
                NewAnnot::Highlight { rects, .. } => {
                    for r in rects {
                        painter.rect_filled(
                            screen_rect(r, map),
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(255, 235, 0, 110),
                        );
                    }
                }
                NewAnnot::Underline { rects, .. } => {
                    for r in rects {
                        let r = screen_rect(r, map);
                        painter.line_segment(
                            [r.left_bottom(), r.right_bottom()],
                            egui::Stroke::new(1.5, egui::Color32::BLUE),
                        );
                    }
                }
                NewAnnot::Note { at, .. } => {
                    let icon = egui::Rect::from_min_size(
                        map.to_screen(*at).into(),
                        egui::vec2(24.0 * map.zoom, 24.0 * map.zoom),
                    );
                    painter.rect_filled(icon, 3.0, egui::Color32::from_rgb(255, 200, 0));
                }
                NewAnnot::Ink { strokes, .. } => {
                    for s in strokes {
                        paint_stroke(painter, s, map);
                    }
                }
            }
        }
    }

    /// Resposta do worker com as palavras de uma seleção: copia o texto ou cria a marcação.
    pub fn on_words(&mut self, ctx: &egui::Context, tag: u64, page: i32, words: Vec<Word>) {
        let Some(tool) = self.word_jobs.remove(&tag) else {
            return;
        };
        if tool == Tool::Select {
            if !words.is_empty() {
                ctx.copy_text(selection_text(&words));
            }
        } else if let Some(annot) = annot_from_words(tool, page, words) {
            self.add(annot);
        }
    }

    /// Janelinha para digitar o texto de uma nota.
    pub fn note_window(&mut self, ui: &mut egui::Ui) {
        if self.saving {
            return; // o rascunho espera o fim do salvamento
        }
        let Some((page, at, mut text)) = self.note_draft.take() else {
            return;
        };
        let mut choice = None; // Some(true) = adicionar, Some(false) = cancelar
        egui::Window::new("Nova nota")
            .collapsible(false)
            .show(ui, |ui| {
                ui.text_edit_multiline(&mut text);
                ui.horizontal(|ui| {
                    if ui.button("Adicionar").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        choice = Some(false);
                    }
                });
            });
        match choice {
            Some(true) => {
                self.add(NewAnnot::Note { page, at, text });
            }
            Some(false) => {}
            None => self.note_draft = Some((page, at, text)),
        }
    }

    /// Pede ao worker para gravar as pendentes; ignora se não há nada ou já está salvando.
    pub fn save(&mut self, worker: &Worker, dest: PathBuf) {
        if self.pending.is_dirty() {
            self.save_as(worker, dest);
        }
    }

    /// Grava o documento em `dest` com as pendentes (mesmo sem nenhuma: "Salvar como").
    pub fn save_as(&mut self, worker: &Worker, dest: PathBuf) {
        if self.saving {
            return;
        }
        self.saving = true;
        worker.send(Request::Save {
            pending: self.pending.items().to_vec(),
            dest,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32) -> Word {
        Word {
            text: text.into(),
            rect: PdfRect::new(x, 10.0, x + 20.0, 22.0),
        }
    }

    #[test]
    fn screen_rect_scales_and_offsets_a_page_rect() {
        let map = PageMap {
            origin: (100.0, 50.0),
            zoom: 2.0,
            size: (612.0, 792.0),
            rotation: pdf_core::Rotation::R0,
        };
        let r = screen_rect(&PdfRect::new(10.0, 20.0, 30.0, 40.0), &map);
        assert_eq!(
            (r.min, r.max),
            (egui::pos2(120.0, 90.0), egui::pos2(160.0, 130.0))
        );
    }

    #[test]
    fn reset_drops_pending_work_and_unlocks_saving() {
        let mut edit = EditState::default();
        edit.pending.add(NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: "a".into(),
        });
        edit.saving = true;
        edit.reset();
        assert!(!edit.pending.is_dirty() && !edit.saving);
    }

    #[test]
    fn highlight_and_underline_cover_every_selected_word() {
        let words = vec![word("a", 0.0), word("b", 30.0)];
        match annot_from_words(Tool::Highlight, 3, words.clone()) {
            Some(NewAnnot::Highlight { page, rects }) => {
                assert_eq!(page, 3);
                assert_eq!(rects, [words[0].rect, words[1].rect]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            annot_from_words(Tool::Underline, 0, words),
            Some(NewAnnot::Underline { .. })
        ));
    }

    #[test]
    fn other_tools_or_no_words_create_nothing() {
        assert!(annot_from_words(Tool::Highlight, 0, vec![]).is_none());
        for tool in [Tool::Select, Tool::Note, Tool::Ink] {
            assert!(
                annot_from_words(tool, 0, vec![word("a", 0.0)]).is_none(),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn selection_text_joins_words_with_spaces() {
        assert_eq!(
            selection_text(&[word("Hello", 0.0), word("Gera", 30.0)]),
            "Hello Gera"
        );
        assert_eq!(selection_text(&[]), "");
    }

    fn note(text: &str) -> NewAnnot {
        NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: text.into(),
        }
    }

    #[test]
    fn edits_made_while_saving_are_refused_instead_of_silently_lost() {
        let worker = Worker::spawn(|| {});
        let mut edit = EditState::default();
        assert!(edit.add(note("a")));
        edit.save(&worker, "x.pdf".into());
        assert!(edit.saving);
        assert!(!edit.add(note("b")), "nada novo durante o salvamento");
        assert!(
            !edit.undo() && !edit.redo(),
            "desfazer/refazer também ficam bloqueados"
        );
        assert_eq!(
            edit.pending.items().len(),
            1,
            "o que foi enviado continua intacto"
        );
        edit.reset(); // resposta Saved
        assert!(!edit.pending.is_dirty());
        assert!(edit.add(note("c")), "depois de salvar volta a aceitar");
        assert!(edit.undo() && edit.redo());
    }

    #[test]
    fn save_is_ignored_when_clean_or_already_saving() {
        let worker = Worker::spawn(|| {});
        let mut edit = EditState::default();
        edit.save(&worker, "x.pdf".into());
        assert!(!edit.saving, "nada pendente: não salva");
        edit.pending.add(NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: "a".into(),
        });
        edit.save(&worker, "x.pdf".into());
        assert!(edit.saving);
    }
}
