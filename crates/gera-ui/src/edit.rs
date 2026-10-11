use crate::create::ShapeDrag;
use crate::shell::annot_props::{self, PropsDraft};
use app_state::annot_geom::{
    annot_hit, dist_to_segment, handle_at, line_handle_at, resized, snap_45, Handle,
};
use app_state::stamps::StampKind;
use app_state::tools::Tool;
use app_state::transform::PageMap;
use pdf_core::{
    AnnotInfo, AnnotKind, AnnotOp, LineEnd, NewAnnot, PdfRect, Request, Segment, TextSelection,
    Worker,
};
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
    /// Uma ponta da linha; `fixed` é a outra.
    LinePoint {
        end: LineEnd,
        fixed: (f32, f32),
    },
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
    /// Pontos da linha arrastada (o contorno de uma linha é ela mesma, não o retângulo).
    line: Option<Segment>,
    /// Redimensionar mantém a proporção (carimbos e assinaturas, como no Acrobat).
    keep_aspect: bool,
    /// Pode ser uma marca do Preencher e assinar (o tamanho dela vira o das próximas).
    mark_like: bool,
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
            Grab::Move => r.translated(dx, dy),
            Grab::Resize(h) => {
                let (x, y) = h.point(r);
                let to = (x + dx, y + dy);
                if self.keep_aspect {
                    app_state::annot_geom::resized_keep_aspect(r, h, to, MIN_SIZE)
                } else {
                    resized(r, h, to, MIN_SIZE)
                }
            }
            Grab::LinePoint { .. } => r,
        }
    }

    /// Onde a linha ficaria se o mouse fosse solto agora (só para linhas).
    fn preview_line(&self) -> Option<Segment> {
        let (dx, dy) = self.offset();
        match (self.grab, self.line) {
            (Grab::LinePoint { fixed, .. }, _) => Some((fixed, self.to)),
            (Grab::Move, Some((a, b))) => Some(((a.0 + dx, a.1 + dy), (b.0 + dx, b.1 + dy))),
            _ => None,
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
            Grab::LinePoint { end, .. } => AnnotOp::MoveLinePoint {
                page,
                id,
                end,
                to: self.to,
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
    /// Marca de redação sobre o texto (Redigir).
    Redact,
}

impl Markup {
    /// A marca que uma ferramenta de arrastar cria ao soltar (Selecionar não cria nenhuma).
    fn for_tool(tool: Tool) -> Option<Self> {
        match tool {
            Tool::Highlight => Some(Self::Highlight),
            Tool::Underline => Some(Self::Underline),
            Tool::RedactText => Some(Self::Redact),
            _ => None,
        }
    }

    fn annot(self, page: i32, rects: Vec<PdfRect>) -> NewAnnot {
        match self {
            Self::Highlight => NewAnnot::Highlight { page, rects },
            Self::Underline => NewAnnot::Underline { page, rects },
            Self::StrikeOut => NewAnnot::StrikeOut { page, rects },
            Self::Redact => NewAnnot::Redact { page, rects },
        }
    }
}

/// Ferramenta ativa, gestos em andamento e as edições ainda não enviadas ao worker.
#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    /// Edições de anotação geradas neste quadro; a aba as envia ao worker (`take_ops`).
    pub(crate) ops: Vec<AnnotOp>,
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
    /// Carimbo escolhido no menu da ferramenta Carimbo.
    pub stamp: StampKind,
    pub(crate) shape_drag: Option<ShapeDrag>,
    /// Caixa de texto sendo digitada sobre a página.
    pub(crate) text_draft: Option<TextDraft>,
    /// Nome do usuário (Identidade), para os carimbos dinâmicos.
    pub author: Option<String>,
    /// Preencher e assinar: ferramenta, tamanhos e janela de assinatura.
    pub fill: crate::fill_sign::FillState,
    /// Permissões do documento protegido (a aba atualiza a cada quadro).
    pub can_copy: bool,
    pub can_annotate: bool,
    /// Carimbo dinâmico esperando a Identidade: (página, ponto).
    pub identity_for: Option<(i32, (f32, f32))>,
}

/// O que a caixa de texto em digitação vai virar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextTarget {
    New,
    Existing { id: i32, original: String },
}

/// Caixa de texto em digitação sobre a página.
#[derive(Debug, Clone)]
pub(crate) struct TextDraft {
    pub(crate) page: i32,
    pub(crate) rect: PdfRect,
    pub(crate) target: TextTarget,
    pub(crate) text: String,
    pub(crate) size: f32,
    pub(crate) color: [f32; 3],
    /// O campo já teve o foco (perdê-lo depois termina a digitação).
    pub(crate) focused: bool,
    /// Texto solto do Preencher e assinar (sem caixa), em vez de caixa de texto.
    pub(crate) fill: bool,
}

/// Onde o arrasto começou, na página. O arrasto só "começa" depois de o mouse andar alguns pixels:
/// o gesto parte de onde o botão foi apertado (senão a primeira letra ou o primeiro canto ficaria
/// deslocado); sem essa informação, vale o ponteiro atual.
pub(crate) fn press_point(
    response: &egui::Response,
    map: &PageMap,
    pointer: Option<(f32, f32)>,
) -> Option<(f32, f32)> {
    let pressed = response.ctx.input(|i| i.pointer.press_origin());
    pressed.map(|p| map.to_page(p.into())).or(pointer)
}

/// Folga do clique sobre uma anotação, em pixels da tela.
const HIT_TOL: f32 = 3.0;

/// A anotação da página sob `p` (a de cima), como (página, id). Só roda quando há clique.
/// Linhas contam pela distância ao segmento (o retângulo de uma diagonal é quase todo vazio).
pub(crate) fn hit_at(
    annots: &[AnnotInfo],
    page: i32,
    p: (f32, f32),
    map: &PageMap,
) -> Option<(i32, i32)> {
    let tol = HIT_TOL / map.zoom;
    annots
        .iter()
        .rev()
        // respostas e status ficam no painel Comentários, não na página
        .filter(|a| a.page == page && !a.is_reply())
        .find(|a| match a.line {
            Some((s, e)) => dist_to_segment(p, s, e) <= tol + a.width.unwrap_or(1.0) / 2.0,
            None => annot_hit(&[(a.id, a.rect)], p, tol).is_some(),
        })
        .map(|a| (page, a.id))
}

fn find(annots: &[AnnotInfo], (page, id): (i32, i32)) -> Option<&AnnotInfo> {
    annots.iter().find(|a| a.page == page && a.id == id)
}

const SELECTED: egui::Color32 = egui::Color32::from_rgb(30, 110, 230);

/// Retângulo da página (pontos) -> retângulo na tela.
/// Alça de redimensionar (quadrado branco com contorno) centrada em `c`.
pub(crate) fn paint_handle(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let square = egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0));
    painter.rect_filled(square, 0.0, egui::Color32::WHITE);
    painter.rect_stroke(
        square,
        0.0,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
}

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
    /// Vazia (nota em branco, clique sem traço) é ignorada sem aviso.
    pub fn add(&mut self, annot: NewAnnot) {
        if !annot.is_empty() {
            self.ops.push(AnnotOp::Create(annot));
        }
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

    /// Arrasto de anotação cujo soltar não foi visto (a página saiu da tela, a ferramenta mudou):
    /// é descartado em vez de virar um movimento fora de hora. Chamar depois das páginas.
    pub fn drop_stale_drag(&mut self, any_button_down: bool) {
        if !any_button_down {
            self.annot_drag = None;
        }
    }

    /// Abre a janela de propriedades da anotação (menu do painel Comentários).
    pub fn open_props(&mut self, annots: &[AnnotInfo], target: (i32, i32)) {
        self.selected = Some(target);
        self.props = find(annots, target).map(PropsDraft::new);
    }

    /// Esc (como no Acrobat): volta para Selecionar, tira a seleção e cancela gestos em andamento.
    pub fn escape(&mut self) {
        self.tool = Tool::Select;
        self.fill.tool = None;
        self.selected = None;
        self.annot_drag = None;
        self.shape_drag = None;
    }

    pub(crate) fn push_op(&mut self, op: AnnotOp) {
        self.ops.push(op);
    }

    /// Apaga a anotação selecionada (tecla Delete ou menu).
    pub fn delete_selected(&mut self) {
        if let Some((page, id)) = self.selected.take() {
            self.ops.push(AnnotOp::Delete { page, id });
        }
    }

    /// Lista nova do worker: a seleção só fica se a anotação ainda existe (a janela de
    /// propriedades se fecha sozinha em `props_window`).
    pub fn keep_valid_selection(&mut self, annots: &[AnnotInfo]) {
        if self.selected.is_some_and(|s| find(annots, s).is_none()) {
            self.selected = None;
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
        if !self.can_copy {
            return;
        }
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
    ) -> egui::Response {
        let response = ui.interact(
            rect,
            ui.id().with(("page", page)),
            egui::Sense::click_and_drag(),
        );
        let pointer = response
            .interact_pointer_pos()
            .map(|p| map.to_page(p.into()));
        match self.tool {
            Tool::Select | Tool::Highlight | Tool::Underline | Tool::RedactText => {
                let grabbed = self.tool == Tool::Select
                    && self.annot_gestures(&response, pointer, annots, page, map);
                if !grabbed {
                    self.drag_text(worker, &response, pointer, page, map);
                }
                self.context_menu(&response, pointer, annots, page, map);
            }
            // Mão e Zoom com marca: os gestos ficam na aba (rolagem e zoom)
            Tool::Hand | Tool::Zoom => {}
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
            Tool::Fill => self.fill_gestures(ui, &response, pointer, page, map),
            Tool::TextBox
            | Tool::Rect
            | Tool::Ellipse
            | Tool::Line
            | Tool::Arrow
            | Tool::Stamp
            | Tool::RedactArea => {
                if let Some(at) = self.create_gestures(ui, &response, pointer, page, map) {
                    let author = self.author.clone();
                    if !self.place_stamp(page, at, author.as_deref(), crate::create::local_now()) {
                        self.identity_for = Some((page, at));
                    }
                }
                self.paint_creation(ui.painter(), page, map);
            }
        }
        self.text_overlay(ui, page, map);
        self.paint_selection(ui.painter(), page, map);
        self.paint_selected(ui.painter(), annots, page, map);
        response
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
        let hit = |p| hit_at(annots, page, p, map);
        if response.double_clicked() {
            match pointer.and_then(hit).and_then(|s| find(annots, s)) {
                Some(note) if note.kind == AnnotKind::Note => {
                    self.note_draft = Some(NoteDraft {
                        page,
                        target: NoteTarget::Existing(note.id),
                        text: note.contents.clone(),
                    });
                    return true;
                }
                Some(b) if b.kind == AnnotKind::TextBox => {
                    let (size, color) = b.font.unwrap_or((crate::create::TEXT_SIZE, [0.0; 3]));
                    self.text_draft = Some(TextDraft {
                        page,
                        rect: b.rect,
                        target: TextTarget::Existing {
                            id: b.id,
                            original: b.contents.clone(),
                        },
                        text: b.contents.clone(),
                        size,
                        color,
                        focused: false,
                        fill: false,
                    });
                    return true;
                }
                _ => {}
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let Some(p) = press_point(response, map, pointer) else {
                return false;
            };
            let size = 5.0 / map.zoom;
            let selected = self
                .selected
                .and_then(|s| find(annots, s))
                .filter(|a| a.page == page);
            let line_end = selected.and_then(|a| {
                let (s, e) = a.line?;
                let end = line_handle_at(s, e, p, size)?;
                let fixed = if end == LineEnd::Start { e } else { s };
                Some((a, Grab::LinePoint { end, fixed }))
            });
            let handle = selected
                .filter(|a| a.resizable())
                .and_then(|a| Some((a, Grab::Resize(handle_at(a.rect, p, size)?))));
            let grab = match line_end.or(handle) {
                Some(found) => Some(found),
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
                line: a.line,
                keep_aspect: a.kind == AnnotKind::Stamp,
                // as marcas do Preencher e assinar são pretas (desenhos de comentário, não)
                mark_like: matches!(a.kind, AnnotKind::Ink | AnnotKind::Ellipse)
                    && a.color == Some([0.0; 3]),
            });
            return true;
        }
        if let Some(drag) = self.annot_drag.as_mut().filter(|d| d.page == page) {
            if let Some(p) = pointer {
                drag.to = match drag.grab {
                    // Shift prende a ponta em múltiplos de 45°, como ao desenhar
                    Grab::LinePoint { fixed, .. } if response.ctx.input(|i| i.modifiers.shift) => {
                        snap_45(fixed, p)
                    }
                    _ => p,
                };
            }
            if !response.dragged() {
                let op = drag.op();
                // Preencher e assinar: a marca redimensionada define o tamanho das próximas
                if let (true, true, Some(AnnotOp::Resize { rect, .. })) =
                    (self.fill.active, drag.mark_like, &op)
                {
                    self.fill.mark_size = rect.height().clamp(4.0, 72.0);
                }
                self.ops.extend(op);
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
            let hit = pointer.and_then(|p| hit_at(annots, page, p, map));
            self.menu_on_annot = hit.is_some();
            if hit.is_some() {
                self.selected = hit;
            }
        }
        let on_annot = self.menu_on_annot && self.selected.is_some_and(|(p, _)| p == page);
        let on_text = self.selection.as_ref().and_then(|s| s.page) == Some(page);
        if !on_annot && !on_text {
            return;
        }
        let (can_copy, can_annotate) = (self.can_copy, self.can_annotate);
        response.context_menu(|ui| {
            if on_annot {
                // documento protegido contra comentários: o menu não oferece alterar
                if !can_annotate {
                    ui.label("O documento não permite alterar comentários.");
                    return;
                }
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
            if ui
                .add_enabled(can_copy, egui::Button::new("Copiar"))
                .clicked()
            {
                self.copy(ui.ctx());
            }
            if !can_annotate {
                return;
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
        let dashed = egui::Stroke::new(1.5, SELECTED);
        if let Some((a, b)) = self
            .annot_drag
            .filter(|d| d.page == page)
            .and_then(|d| d.preview_line())
        {
            let ends = [map.to_screen(a).into(), map.to_screen(b).into()];
            painter.extend(egui::Shape::dashed_line(&ends, dashed, 6.0, 4.0));
            return;
        }
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
        let handle = |c: (f32, f32)| paint_handle(painter, map.to_screen(c).into(), SELECTED);
        if let Some((s, e)) = a.line {
            // linha selecionada: alças nas duas pontas, como no Acrobat
            handle(s);
            handle(e);
            return;
        }
        let r = screen_rect(&a.rect, map).expand(2.0);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.5, SELECTED),
            egui::StrokeKind::Outside,
        );
        if a.resizable() {
            for h in Handle::ALL {
                handle(h.point(a.rect));
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
            if let Some(p) = press_point(response, map, pointer) {
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
                match draft.target {
                    NoteTarget::New(at) => self.add(NewAnnot::Note { page, at, text }),
                    NoteTarget::Existing(id) => {
                        self.ops.push(AnnotOp::SetContents { page, id, text })
                    }
                }
            }
            Some(false) => {}
            None => self.note_draft = Some(draft),
        }
    }

    /// Pede ao worker para gravar o documento em `dest`; ignora se já está salvando. As edições
    /// feitas durante a gravação chegam ao worker depois dela e ficam como alterações novas.
    /// Devolve se o pedido foi enviado.
    pub fn save(&mut self, worker: &Worker, dest: PathBuf) -> bool {
        if self.saving {
            return false;
        }
        self.saving = true;
        worker.send(Request::Save { dest });
        true
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
        assert!(edit.save(&worker, "x.pdf".into()));
        assert!(!edit.save(&worker, "y.pdf".into()), "um salvamento por vez");
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
            line: None,
            keep_aspect: false,
            mark_like: false,
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
            ..Default::default()
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

    #[test]
    fn a_drag_whose_release_was_missed_is_dropped_without_an_edit() {
        let mut e = EditState {
            annot_drag: Some(drag(Grab::Move, (200.0, 200.0))),
            ..Default::default()
        };
        e.drop_stale_drag(true);
        assert!(
            e.annot_drag.is_some(),
            "botão ainda apertado: segue arrastando"
        );
        e.drop_stale_drag(false);
        assert!(e.annot_drag.is_none() && e.take_ops().is_empty());
    }

    #[test]
    fn hit_on_a_diagonal_line_uses_the_distance_to_the_segment() {
        let map = PageMap {
            origin: (0.0, 0.0),
            zoom: 1.0,
            size: (612.0, 792.0),
            rotation: pdf_core::Rotation::R0,
        };
        let mut l = info(0, 7);
        l.kind = AnnotKind::Line;
        l.rect = PdfRect::new(0.0, 0.0, 200.0, 200.0);
        l.line = Some(((0.0, 0.0), (200.0, 200.0)));
        assert_eq!(hit_at(&[l.clone()], 0, (100.0, 101.0), &map), Some((0, 7)));
        assert_eq!(
            hit_at(&[l], 0, (180.0, 20.0), &map),
            None,
            "dentro do retângulo, longe da linha"
        );
    }

    #[test]
    fn dragging_a_line_end_sends_the_new_point() {
        let d = AnnotDrag {
            page: 0,
            id: 3,
            rect: PdfRect::new(0.0, 0.0, 100.0, 50.0),
            grab: Grab::LinePoint {
                end: LineEnd::End,
                fixed: (0.0, 0.0),
            },
            from: (100.0, 50.0),
            to: (130.0, 70.0),
            line: Some(((0.0, 0.0), (100.0, 50.0))),
            keep_aspect: false,
            mark_like: false,
        };
        assert!(matches!(
            d.op(),
            Some(AnnotOp::MoveLinePoint { id: 3, end: LineEnd::End, to, .. }) if to == (130.0, 70.0)
        ));
    }

    #[test]
    fn escape_returns_to_select_and_drops_selection_and_drags() {
        let mut e = EditState {
            tool: Tool::Rect,
            selected: Some((0, 1)),
            annot_drag: Some(drag(Grab::Move, (200.0, 200.0))),
            ..Default::default()
        };
        e.escape();
        assert_eq!(e.tool, Tool::Select);
        assert!(e.selected.is_none() && e.annot_drag.is_none());
        assert!(e.take_ops().is_empty(), "cancelar não manda edição");
    }

    #[test]
    fn hit_ignores_replies() {
        let map = PageMap {
            origin: (0.0, 0.0),
            zoom: 1.0,
            size: (612.0, 792.0),
            rotation: pdf_core::Rotation::R0,
        };
        let parent = info(0, 1);
        let mut reply = info(0, 2);
        reply.reply_to = Some(1);
        assert_eq!(hit_at(&[parent, reply], 0, (5.0, 5.0), &map), Some((0, 1)));
    }
}
