use app_state::transform::PageMap;
use app_state::{annots::PendingAnnots, tools::Tool};
use pdf_core::{NewAnnot, PdfRect, Request, TextSelection, Worker};
use std::path::PathBuf;

/// Texto selecionado: numa página (com as áreas marcadas) ou o documento inteiro (Ctrl+A).
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// `None`: documento inteiro (sem áreas para pintar ou marcar).
    pub page: Option<i32>,
    pub quads: Vec<PdfRect>,
    pub text: String,
}

/// Arrasto de seleção em andamento.
#[derive(Debug, Clone, Copy)]
struct SelectDrag {
    page: i32,
    from: (f32, f32),
    /// Último ponto já pedido ao worker (só pede de novo se o mouse andou).
    last: (f32, f32),
}

/// Marca de texto criada a partir de uma seleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    Highlight,
    Underline,
    StrikeOut,
}

impl Markup {
    /// A marca que uma ferramenta de arrastar cria ao soltar (Selecionar não cria nenhuma).
    fn for_tool(tool: Tool) -> Option<Self> {
        match tool {
            Tool::Highlight => Some(Self::Highlight),
            Tool::Underline => Some(Self::Underline),
            _ => None,
        }
    }

    fn annot(self, page: i32, rects: Vec<PdfRect>) -> NewAnnot {
        match self {
            Self::Highlight => NewAnnot::Highlight { page, rects },
            Self::Underline => NewAnnot::Underline { page, rects },
            Self::StrikeOut => NewAnnot::StrikeOut { page, rects },
        }
    }
}

/// Ferramenta ativa, anotações pendentes e gestos em andamento.
#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    pub pending: PendingAnnots,
    pub saving: bool,
    stroke: Vec<(f32, f32)>,
    pub selection: Option<Selection>,
    select_drag: Option<SelectDrag>,
    /// Último pedido de texto enviado ao worker; respostas mais antigas são descartadas.
    pub(crate) latest_tag: u64,
    /// Arrasto de Destacar/Sublinhar terminado: tag da resposta final e a marca a criar.
    pub(crate) finish_tag: Option<(u64, Markup)>,
    /// Nota sendo digitada: (página, posição, texto).
    note_draft: Option<(i32, (f32, f32), String)>,
}

/// Retângulo da página (pontos) -> retângulo na tela.
pub(crate) fn screen_rect(r: &PdfRect, map: &PageMap) -> egui::Rect {
    let (min, max) = map.rect_to_screen(r);
    egui::Rect::from_min_max(min.into(), max.into())
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
        self.note_draft = None;
        self.selection = None;
        self.select_drag = None;
        self.finish_tag = None;
    }

    /// Toda pergunta de texto ao worker avança a tag; só a resposta da mais nova vale.
    fn next_tag(&mut self) -> u64 {
        self.latest_tag += 1;
        self.latest_tag
    }

    fn request_selection(&mut self, worker: &Worker, page: i32, a: (f32, f32), b: (f32, f32)) {
        worker.send(Request::SelectText {
            tag: self.next_tag(),
            page,
            a,
            b,
        });
    }

    /// Resposta de seleção do worker: atualiza a seleção ou, no fim de um arrasto de
    /// Destacar/Sublinhar, cria a marca. Respostas atrasadas (tag antiga) são ignoradas.
    pub fn on_selection(&mut self, tag: u64, page: i32, sel: TextSelection) {
        if tag < self.latest_tag {
            return;
        }
        if let Some((_, markup)) = self.finish_tag.filter(|&(t, _)| t == tag) {
            self.finish_tag = None;
            self.selection = None;
            if !sel.quads.is_empty() {
                self.add(markup.annot(page, sel.quads));
            }
            return;
        }
        self.selection =
            (!sel.quads.is_empty() && !sel.text.trim().is_empty()).then_some(Selection {
                page: Some(page),
                quads: sel.quads,
                text: sel.text,
            });
    }

    /// Selecionar tudo: pede o texto do documento inteiro.
    pub fn select_all(&mut self, worker: &Worker) {
        worker.send(Request::DocumentText {
            tag: self.next_tag(),
        });
    }

    pub fn on_document_text(&mut self, tag: u64, text: String) {
        if tag == self.latest_tag && !text.trim().is_empty() {
            self.selection = Some(Selection {
                page: None,
                quads: Vec::new(),
                text,
            });
        }
    }

    /// Copia o texto selecionado (Ctrl+C); sem seleção, a área de transferência não muda.
    pub fn copy(&self, ctx: &egui::Context) {
        if let Some(s) = self.selection.as_ref().filter(|s| !s.text.is_empty()) {
            ctx.copy_text(s.text.clone());
        }
    }

    /// Transforma a seleção da página numa marca (menu de contexto).
    pub fn mark_selection(&mut self, markup: Markup) {
        if let Some(Selection {
            page: Some(page),
            quads,
            ..
        }) = self
            .selection
            .take_if(|s| s.page.is_some() && !s.quads.is_empty())
        {
            self.add(markup.annot(page, quads));
        }
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
                self.drag_text(worker, &response, pointer, page, map);
                self.selection_menu(&response, page);
            }
            Tool::Note => {
                if let (true, Some(at)) = (response.clicked(), pointer) {
                    self.note_draft = Some((page, at, String::new()));
                }
            }
            Tool::Ink => self.draw_ink(ui, &response, pointer, page, map),
        }
        self.paint_pending(ui.painter(), page, map);
        self.paint_selection(ui.painter(), page, map);
    }

    /// Arrastar seleciona o texto pelo fluxo (pedindo ao worker a cada movimento); ao soltar com
    /// Destacar/Sublinhar, a última seleção vira a marca. Clique simples limpa a seleção.
    fn drag_text(
        &mut self,
        worker: &Worker,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        page: i32,
        map: &PageMap,
    ) {
        if response.clicked() {
            self.selection = None;
        }
        if response.drag_started() {
            // O arrasto só "começa" depois de o mouse andar alguns pixels: a seleção parte de onde
            // o botão foi apertado, senão a primeira letra sob o mouse ficaria de fora.
            let pressed = response.ctx.input(|i| i.pointer.press_origin());
            if let Some(p) = pressed.map(|p| map.to_page(p.into())).or(pointer) {
                self.select_drag = Some(SelectDrag {
                    page,
                    from: p,
                    last: p,
                });
                self.selection = None;
            }
        }
        if let (Some(drag), Some(b)) = (self.select_drag, pointer) {
            let moved = (b.0 - drag.last.0).abs() + (b.1 - drag.last.1).abs() >= 1.0;
            if drag.page == page && (moved || response.drag_stopped()) {
                self.request_selection(worker, page, drag.from, b);
                self.select_drag = Some(SelectDrag { last: b, ..drag });
            }
        }
        if response.drag_stopped() {
            if let Some(markup) = Markup::for_tool(self.tool) {
                self.finish_tag = Some((self.latest_tag, markup));
            }
            self.select_drag = None;
        }
    }

    /// Botão direito sobre a seleção desta página: Copiar e marcar o texto.
    fn selection_menu(&mut self, response: &egui::Response, page: i32) {
        if self.selection.as_ref().and_then(|s| s.page) != Some(page) {
            return;
        }
        response.context_menu(|ui| {
            if ui.button("Copiar").clicked() {
                self.copy(ui.ctx());
            }
            ui.separator();
            for (label, markup) in [
                ("Destacar texto", Markup::Highlight),
                ("Sublinhar texto", Markup::Underline),
                ("Riscar texto", Markup::StrikeOut),
            ] {
                if ui.button(label).clicked() {
                    self.mark_selection(markup);
                }
            }
        });
    }

    fn paint_selection(&self, painter: &egui::Painter, page: i32, map: &PageMap) {
        let Some(sel) = self.selection.as_ref().filter(|s| s.page == Some(page)) else {
            return;
        };
        for r in &sel.quads {
            painter.rect_filled(
                screen_rect(r, map),
                0.0,
                egui::Color32::from_rgba_unmultiplied(60, 120, 255, 80),
            );
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
                NewAnnot::StrikeOut { rects, .. } => {
                    for r in rects {
                        let r = screen_rect(r, map);
                        let y = r.center().y;
                        painter.line_segment(
                            [egui::pos2(r.left(), y), egui::pos2(r.right(), y)],
                            egui::Stroke::new(1.5, egui::Color32::RED),
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
    use pdf_core::TextSelection;

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

    fn sel(text: &str) -> TextSelection {
        TextSelection {
            quads: vec![PdfRect::new(1.0, 1.0, 5.0, 5.0)],
            text: text.into(),
        }
    }

    #[test]
    fn stale_selection_responses_are_ignored() {
        let mut e = EditState {
            latest_tag: 5,
            ..Default::default()
        };
        e.on_selection(4, 0, sel("velha"));
        assert!(e.selection.is_none());
        e.on_selection(5, 0, sel("nova"));
        assert_eq!(e.selection.as_ref().map(|s| s.text.as_str()), Some("nova"));
    }

    #[test]
    fn finishing_a_highlight_drag_turns_the_selection_into_an_annotation() {
        let mut e = EditState {
            tool: Tool::Highlight,
            latest_tag: 3,
            finish_tag: Some((3, Markup::Highlight)),
            ..Default::default()
        };
        e.on_selection(3, 2, sel("x"));
        assert!(matches!(
            e.pending.items(),
            [NewAnnot::Highlight { page: 2, .. }]
        ));
        assert!(e.selection.is_none());
    }

    #[test]
    fn context_menu_marks_the_current_selection() {
        let mut e = EditState {
            selection: Some(Selection {
                page: Some(1),
                quads: vec![PdfRect::new(1.0, 1.0, 5.0, 5.0)],
                text: "x".into(),
            }),
            ..Default::default()
        };
        e.mark_selection(Markup::StrikeOut);
        assert!(matches!(
            e.pending.items(),
            [NewAnnot::StrikeOut { page: 1, .. }]
        ));
        assert!(e.selection.is_none());
    }

    #[test]
    fn whole_document_selection_cannot_be_marked() {
        let mut e = EditState {
            selection: Some(Selection {
                page: None,
                quads: vec![],
                text: "tudo".into(),
            }),
            ..Default::default()
        };
        e.mark_selection(Markup::Highlight);
        assert!(e.pending.items().is_empty());
    }

    #[test]
    fn select_all_keeps_only_the_latest_document_text() {
        let worker = Worker::spawn(|| {});
        let mut e = EditState::default();
        e.select_all(&worker);
        let tag = e.latest_tag;
        e.on_document_text(tag - 1, "velho".into());
        assert!(e.selection.is_none());
        e.on_document_text(tag, "tudo".into());
        assert_eq!(
            e.selection,
            Some(Selection {
                page: None,
                quads: vec![],
                text: "tudo".into()
            })
        );
    }

    #[test]
    fn empty_selections_are_dropped() {
        let mut e = EditState::default();
        e.on_selection(
            0,
            0,
            TextSelection {
                quads: vec![],
                text: String::new(),
            },
        );
        assert!(e.selection.is_none());
    }
}
