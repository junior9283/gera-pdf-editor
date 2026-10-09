use crate::shell::annot_props::{self, PropsDraft};
use app_state::annot_geom::{annot_hit, handle_at, resized, Handle};
use app_state::tools::Tool;
use app_state::transform::PageMap;
use pdf_core::{AnnotInfo, AnnotKind, AnnotOp, NewAnnot, PdfRect, Request, TextSelection, Worker};
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

/// Como a anotação está sendo arrastada.
#[derive(Debug, Clone, Copy)]
enum Grab {
    Move,
    Resize(Handle),
}

/// Arrasto de anotação em andamento (mover ou redimensionar pela alça).
#[derive(Debug, Clone, Copy)]
struct AnnotDrag {
    page: i32,
    id: i32,
    /// Retângulo da anotação quando o arrasto começou.
    rect: PdfRect,
    grab: Grab,
    from: (f32, f32),
    to: (f32, f32),
}

/// Menor lado (pontos) de um desenho redimensionado.
const MIN_SIZE: f32 = 4.0;

impl AnnotDrag {
    fn offset(&self) -> (f32, f32) {
        (self.to.0 - self.from.0, self.to.1 - self.from.1)
    }

    /// Onde a anotação ficaria se o mouse fosse solto agora.
    fn preview(&self) -> PdfRect {
        let (dx, dy) = self.offset();
        let r = self.rect;
        match self.grab {
            Grab::Move => PdfRect::new(r.x0 + dx, r.y0 + dy, r.x1 + dx, r.y1 + dy),
            Grab::Resize(h) => {
                let (x, y) = h.point(r);
                resized(r, h, (x + dx, y + dy), MIN_SIZE)
            }
        }
    }

    /// A edição a enviar ao soltar (nada se o mouse quase não andou).
    fn op(&self) -> Option<AnnotOp> {
        let (dx, dy) = self.offset();
        if dx.abs().max(dy.abs()) < 1.0 {
            return None;
        }
        let (page, id) = (self.page, self.id);
        Some(match self.grab {
            Grab::Move => AnnotOp::Move { page, id, dx, dy },
            Grab::Resize(_) => AnnotOp::Resize {
                page,
                id,
                rect: self.preview(),
            },
        })
    }
}

/// Nota sendo digitada: nova (na posição) ou o texto de uma existente (duplo clique).
enum NoteTarget {
    New((f32, f32)),
    Existing(i32),
}

struct NoteDraft {
    page: i32,
    target: NoteTarget,
    text: String,
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

/// Ferramenta ativa, gestos em andamento e as edições ainda não enviadas ao worker.
#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    /// Edições de anotação geradas neste quadro; a aba as envia ao worker (`take_ops`).
    ops: Vec<AnnotOp>,
    pub saving: bool,
    stroke: Vec<(f32, f32)>,
    pub selection: Option<Selection>,
    select_drag: Option<SelectDrag>,
    /// Último pedido de texto enviado ao worker; respostas mais antigas são descartadas.
    pub(crate) latest_tag: u64,
    /// Arrasto de Destacar/Sublinhar terminado: tag da resposta final e a marca a criar.
    pub(crate) finish_tag: Option<(u64, Markup)>,
    note_draft: Option<NoteDraft>,
    /// Anotação selecionada: (página, id).
    pub selected: Option<(i32, i32)>,
    annot_drag: Option<AnnotDrag>,
    /// O último clique direito caiu sobre uma anotação (o menu é o dela, não o do texto).
    menu_on_annot: bool,
    /// Janela de propriedades aberta.
    props: Option<PropsDraft>,
}

/// Anotações da página como (id, retângulo), na ordem de desenho.
fn rects_on(annots: &[AnnotInfo], page: i32) -> Vec<(i32, PdfRect)> {
    annots
        .iter()
        .filter(|a| a.page == page)
        .map(|a| (a.id, a.rect))
        .collect()
}

fn find(annots: &[AnnotInfo], (page, id): (i32, i32)) -> Option<&AnnotInfo> {
    annots.iter().find(|a| a.page == page && a.id == id)
}

const SELECTED: egui::Color32 = egui::Color32::from_rgb(30, 110, 230);

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
    /// Cria uma anotação (vai ao worker, que a grava no documento em memória).
    pub fn add(&mut self, annot: NewAnnot) {
        self.ops.push(AnnotOp::Create(annot));
    }

    /// As edições geradas desde a última chamada, na ordem.
    pub fn take_ops(&mut self) -> Vec<AnnotOp> {
        std::mem::take(&mut self.ops)
    }

    /// Descarta gestos e edições não enviadas (ao abrir outro documento).
    pub fn reset(&mut self) {
        self.ops.clear();
        self.saving = false;
        self.stroke.clear();
        self.note_draft = None;
        self.selection = None;
        self.select_drag = None;
        self.finish_tag = None;
        self.selected = None;
        self.annot_drag = None;
        self.props = None;
    }

    /// Apaga a anotação selecionada (tecla Delete ou menu).
    pub fn delete_selected(&mut self) {
        if let Some((page, id)) = self.selected.take() {
            self.ops.push(AnnotOp::Delete { page, id });
        }
    }

    /// Lista nova do worker: a seleção (e a janela de propriedades) só fica se a anotação existe.
    pub fn keep_valid_selection(&mut self, annots: &[AnnotInfo]) {
        if self.selected.is_some_and(|s| find(annots, s).is_none()) {
            self.selected = None;
        }
        if self
            .props
            .as_ref()
            .is_some_and(|d| find(annots, (d.page, d.id)).is_none())
        {
            self.props = None;
        }
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

    /// Trata o mouse sobre uma página visível e desenha os gestos em andamento nela.
    pub fn on_page(
        &mut self,
        ui: &mut egui::Ui,
        worker: &Worker,
        annots: &[AnnotInfo],
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
        match self.tool {
            Tool::Select | Tool::Highlight | Tool::Underline => {
                let grabbed = self.tool == Tool::Select
                    && self.annot_gestures(&response, pointer, annots, page, map);
                if !grabbed {
                    self.drag_text(worker, &response, pointer, page, map);
                }
                self.context_menu(&response, pointer, annots, page, map);
            }
            Tool::Note => {
                if let (true, Some(at)) = (response.clicked(), pointer) {
                    self.note_draft = Some(NoteDraft {
                        page,
                        target: NoteTarget::New(at),
                        text: String::new(),
                    });
                }
            }
            Tool::Ink => self.draw_ink(ui, &response, pointer, page, map),
        }
        self.paint_selection(ui.painter(), page, map);
        self.paint_selected(ui.painter(), annots, page, map);
    }

    /// Com Selecionar: clicar numa anotação a seleciona; arrastar uma nota ou desenho move; a alça
    /// de um desenho selecionado redimensiona; duplo clique numa nota edita o texto. Devolve se o
    /// gesto foi da anotação (senão segue como seleção de texto).
    fn annot_gestures(
        &mut self,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        annots: &[AnnotInfo],
        page: i32,
        map: &PageMap,
    ) -> bool {
        let rects = rects_on(annots, page);
        let hit = |p| annot_hit(&rects, p, 3.0 / map.zoom).map(|id| (page, id));
        if response.double_clicked() {
            if let Some(note) = pointer
                .and_then(hit)
                .and_then(|s| find(annots, s))
                .filter(|a| a.kind == AnnotKind::Note)
            {
                self.note_draft = Some(NoteDraft {
                    page,
                    target: NoteTarget::Existing(note.id),
                    text: note.contents.clone(),
                });
                return true;
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let pressed = response.ctx.input(|i| i.pointer.press_origin());
            let Some(p) = pressed.map(|p| map.to_page(p.into())).or(pointer) else {
                return false;
            };
            let handle = self
                .selected
                .and_then(|s| find(annots, s))
                .filter(|a| a.page == page && a.resizable())
                .and_then(|a| Some((a, handle_at(a.rect, p, 5.0 / map.zoom)?)));
            let grab = match handle {
                Some((a, h)) => Some((a, Grab::Resize(h))),
                None => hit(p)
                    .and_then(|s| find(annots, s))
                    .filter(|a| a.movable())
                    .map(|a| (a, Grab::Move)),
            };
            let Some((a, grab)) = grab else {
                self.selected = None; // marcação ou área vazia: o arrasto seleciona texto
                return false;
            };
            self.selected = Some((page, a.id));
            self.selection = None;
            self.annot_drag = Some(AnnotDrag {
                page,
                id: a.id,
                rect: a.rect,
                grab,
                from: p,
                to: p,
            });
            return true;
        }
        if let Some(drag) = self.annot_drag.as_mut().filter(|d| d.page == page) {
            if let Some(p) = pointer {
                drag.to = p;
            }
            if !response.dragged() {
                self.ops.extend(drag.op());
                self.annot_drag = None;
            }
            return true;
        }
        if response.clicked() {
            self.selected = pointer.and_then(hit);
            if self.selected.is_some() {
                self.selection = None;
                return true;
            }
        }
        false
    }

    /// Botão direito: sobre uma anotação, Propriedades… e Excluir; sobre a seleção de texto desta
    /// página, Copiar e as marcas.
    fn context_menu(
        &mut self,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        annots: &[AnnotInfo],
        page: i32,
        map: &PageMap,
    ) {
        if response.secondary_clicked() {
            let rects = rects_on(annots, page);
            let hit = pointer.and_then(|p| annot_hit(&rects, p, 3.0 / map.zoom));
            self.menu_on_annot = hit.is_some();
            if let Some(id) = hit {
                self.selected = Some((page, id));
            }
        }
        let on_annot = self.menu_on_annot && self.selected.is_some_and(|(p, _)| p == page);
        let on_text = self.selection.as_ref().and_then(|s| s.page) == Some(page);
        if !on_annot && !on_text {
            return;
        }
        response.context_menu(|ui| {
            if on_annot {
                if ui.button("Propriedades…").clicked() {
                    self.props = self
                        .selected
                        .and_then(|s| find(annots, s))
                        .map(PropsDraft::new);
                }
                if ui.button("Excluir").clicked() {
                    self.delete_selected();
                }
                return;
            }
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

    /// Contorno da anotação selecionada (com as alças, se der para redimensionar) ou, durante o
    /// arrasto, o contorno tracejado de onde ela vai ficar.
    fn paint_selected(
        &self,
        painter: &egui::Painter,
        annots: &[AnnotInfo],
        page: i32,
        map: &PageMap,
    ) {
        if let Some(drag) = self.annot_drag.filter(|d| d.page == page) {
            let r = screen_rect(&drag.preview(), map);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &corners,
                egui::Stroke::new(1.5, SELECTED),
                6.0,
                4.0,
            ));
            return;
        }
        let Some(a) = self
            .selected
            .filter(|(p, _)| *p == page)
            .and_then(|s| find(annots, s))
        else {
            return;
        };
        let r = screen_rect(&a.rect, map).expand(2.0);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.5, SELECTED),
            egui::StrokeKind::Outside,
        );
        if a.resizable() {
            for h in Handle::ALL {
                let c = map.to_screen(h.point(a.rect));
                let square = egui::Rect::from_center_size(c.into(), egui::vec2(7.0, 7.0));
                painter.rect_filled(square, 0.0, egui::Color32::WHITE);
                painter.rect_stroke(
                    square,
                    0.0,
                    egui::Stroke::new(1.0, SELECTED),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    /// Janela "Propriedades da anotação", se aberta.
    pub fn props_window(&mut self, ui: &mut egui::Ui, annots: &[AnnotInfo]) {
        let Some(draft) = self.props.as_mut() else {
            return;
        };
        let Some(info) = find(annots, (draft.page, draft.id)) else {
            self.props = None;
            return;
        };
        let mut open = true;
        self.ops
            .extend(annot_props::window(ui, info, draft, &mut open));
        if !open {
            self.props = None;
        }
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
        // só o botão esquerdo seleciona: o direito (com um tremido) é para o menu de contexto
        if response.drag_started_by(egui::PointerButton::Primary) {
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
            if drag.page == page
                && (moved || response.drag_stopped_by(egui::PointerButton::Primary))
            {
                self.request_selection(worker, page, drag.from, b);
                self.select_drag = Some(SelectDrag { last: b, ..drag });
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(markup) = Markup::for_tool(self.tool) {
                self.finish_tag = Some((self.latest_tag, markup));
            }
            self.select_drag = None;
        }
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
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.stroke.clear();
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            self.stroke.extend(pointer);
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
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

    /// Janelinha para digitar o texto de uma nota nova ou editar o de uma existente.
    pub fn note_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.note_draft.take() else {
            return;
        };
        let (title, ok) = match draft.target {
            NoteTarget::New(_) => ("Nova nota", "Adicionar"),
            NoteTarget::Existing(_) => ("Editar nota", "OK"),
        };
        let mut choice = None; // Some(true) = confirmar, Some(false) = cancelar
        egui::Window::new(title).collapsible(false).show(ui, |ui| {
            ui.text_edit_multiline(&mut draft.text);
            ui.horizontal(|ui| {
                if ui.button(ok).clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancelar").clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                let (page, text) = (draft.page, draft.text);
                self.ops.push(match draft.target {
                    NoteTarget::New(at) => AnnotOp::Create(NewAnnot::Note { page, at, text }),
                    NoteTarget::Existing(id) => AnnotOp::SetContents { page, id, text },
                });
            }
            Some(false) => {}
            None => self.note_draft = Some(draft),
        }
    }

    /// Pede ao worker para gravar o documento em `dest`; ignora se já está salvando. As edições
    /// feitas durante a gravação chegam ao worker depois dela e ficam como alterações novas.
    pub fn save(&mut self, worker: &Worker, dest: PathBuf) {
        if !self.saving {
            self.saving = true;
            worker.send(Request::Save { dest });
        }
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
    fn reset_drops_unsent_edits_and_unlocks_saving() {
        let mut edit = EditState::default();
        edit.add(NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: "a".into(),
        });
        edit.saving = true;
        edit.reset();
        assert!(edit.take_ops().is_empty() && !edit.saving);
    }

    #[test]
    fn a_second_save_waits_for_the_first() {
        let worker = Worker::spawn(|| {});
        let mut edit = EditState::default();
        edit.save(&worker, "x.pdf".into());
        assert!(edit.saving);
        edit.add(NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: "a".into(),
        });
        assert_eq!(
            edit.take_ops().len(),
            1,
            "editar durante a gravação é permitido"
        );
        assert!(edit.take_ops().is_empty(), "cada edição sai uma vez só");
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
            e.take_ops()[..],
            [AnnotOp::Create(NewAnnot::Highlight { page: 2, .. })]
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
            e.take_ops()[..],
            [AnnotOp::Create(NewAnnot::StrikeOut { page: 1, .. })]
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
        assert!(e.take_ops().is_empty());
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

    fn drag(grab: Grab, to: (f32, f32)) -> AnnotDrag {
        AnnotDrag {
            page: 1,
            id: 9,
            rect: PdfRect::new(100.0, 100.0, 200.0, 150.0),
            grab,
            from: (110.0, 110.0),
            to,
        }
    }

    #[test]
    fn moving_previews_the_shifted_rect_and_sends_the_offset() {
        let d = drag(Grab::Move, (140.0, 100.0));
        assert_eq!(d.preview(), PdfRect::new(130.0, 90.0, 230.0, 140.0));
        assert!(matches!(
            d.op(),
            Some(AnnotOp::Move { page: 1, id: 9, dx, dy }) if dx == 30.0 && dy == -10.0
        ));
    }

    #[test]
    fn resizing_moves_the_grabbed_corner_by_the_mouse_offset() {
        let d = AnnotDrag {
            from: (200.0, 150.0),
            ..drag(Grab::Resize(Handle::BottomRight), (260.0, 170.0))
        };
        let r = PdfRect::new(100.0, 100.0, 260.0, 170.0);
        assert_eq!(d.preview(), r);
        assert!(matches!(d.op(), Some(AnnotOp::Resize { id: 9, rect, .. }) if rect == r));
    }

    #[test]
    fn tiny_drags_send_nothing() {
        assert!(drag(Grab::Move, (110.4, 110.3)).op().is_none());
        assert!(drag(Grab::Resize(Handle::TopLeft), (110.2, 110.0))
            .op()
            .is_none());
    }

    fn info(page: i32, id: i32) -> AnnotInfo {
        AnnotInfo {
            id,
            kind: AnnotKind::Note,
            page,
            rect: PdfRect::new(0.0, 0.0, 20.0, 20.0),
            contents: String::new(),
            color: None,
            opacity: 1.0,
            width: None,
        }
    }

    #[test]
    fn deleting_sends_the_selected_annotation_once() {
        let mut e = EditState {
            selected: Some((2, 5)),
            ..Default::default()
        };
        e.delete_selected();
        e.delete_selected();
        assert!(matches!(
            e.take_ops()[..],
            [AnnotOp::Delete { page: 2, id: 5 }]
        ));
        assert!(e.selected.is_none());
    }

    #[test]
    fn selection_survives_only_while_the_annotation_exists() {
        let mut e = EditState {
            selected: Some((0, 3)),
            ..Default::default()
        };
        e.keep_valid_selection(&[info(0, 3)]);
        assert_eq!(e.selected, Some((0, 3)));
        e.keep_valid_selection(&[info(1, 3)]);
        assert!(e.selected.is_none(), "mesmo id em outra página não conta");
    }
}
