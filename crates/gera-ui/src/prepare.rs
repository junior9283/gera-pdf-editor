//! Preparar formulário (como no Acrobat): os campos aparecem com contorno e nome; clicar ou
//! arrastar com uma ferramenta cria o campo; com Selecionar, clicar seleciona (Shift ou Ctrl junta
//! à seleção), arrastar move a seleção (com guias de alinhamento), arrastar no vazio seleciona por
//! retângulo, as alças redimensionam e o duplo clique abre as Propriedades. O último campo clicado
//! é a âncora de alinhar e igualar o tamanho.
use crate::edit::screen_rect;
use crate::shell::field_props::FieldPropsWindow;
use app_state::annot_geom::{annot_hit, handle_at, resized, Handle};
use app_state::arrange::{self, Axis, Edge, Guide, SizeKind};
use app_state::prepare::{next_name, placed_rect};
use app_state::transform::PageMap;
use pdf_core::{FieldInfo, NewFieldKind, PdfRect, PrepareOp};

/// Arrasto em andamento: mover a seleção (todos os campos da página) ou redimensionar a âncora.
#[derive(Clone)]
struct Drag {
    page: i32,
    /// Os campos arrastados e onde estavam.
    items: Vec<(i32, PdfRect)>,
    /// O campo pego (referência das guias) e a alça, ao redimensionar.
    rect: PdfRect,
    grab: Option<Handle>,
    from: (f32, f32),
    /// Deslocamento já com o encaixe das guias.
    delta: (f32, f32),
}

impl Drag {
    fn preview(&self, rect: PdfRect) -> PdfRect {
        match self.grab {
            None => rect.translated(self.delta.0, self.delta.1),
            Some(h) => {
                let (x, y) = h.point(rect);
                resized(rect, h, (x + self.delta.0, y + self.delta.1), 6.0)
            }
        }
    }
}

/// Arrasto de criação de um campo, ou retângulo de seleção: (página, de, até).
type Placing = (i32, (f32, f32), (f32, f32));

/// Pedido sobre a seleção (menu de contexto, menu Organizar ou tecla).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrangeCmd {
    Align(Edge),
    Center(Axis),
    Distribute(Axis),
    Size(SizeKind),
    Delete,
    Copy,
    Paste,
    Duplicate,
    Copies,
    Props,
}

#[derive(Default)]
pub struct PrepareState {
    /// Ferramenta de campo ativa (`None` = Selecionar).
    pub tool: Option<NewFieldKind>,
    /// Campos selecionados (página, id); o último é a âncora.
    pub selection: Vec<(i32, i32)>,
    drag: Option<Drag>,
    /// Arrasto de criação: (página, de, até).
    place: Option<Placing>,
    /// Retângulo de seleção em andamento (e se junta à seleção, com Shift).
    marquee: Option<(Placing, bool)>,
    /// Linhas de guia do arrasto (página e linha).
    guide_lines: Vec<(i32, Guide)>,
    /// Guias desligadas (`G`).
    pub guides_off: bool,
    /// Visualizar (`P`): preencher sem sair do Preparar.
    pub preview: bool,
    /// Campos copiados (`Ctrl+C`).
    pub clipboard: Vec<(i32, i32)>,
    /// Pedidos do menu de contexto, a executar pela aba.
    pub cmds: Vec<ArrangeCmd>,
    /// Prévia de "Criar várias cópias": (página, retângulo).
    pub ghosts: Vec<(i32, PdfRect)>,
    /// Grupo dos botões de opção colocados enquanto a ferramenta segue ativa.
    radio_group: Option<String>,
    /// Campo recém-criado a selecionar quando a lista de campos chegar: (página, ids que a página
    /// já tinha — o novo é o que aparecer).
    pub pending_select: Option<(i32, Vec<i32>)>,
    /// Janela Propriedades aberta (ou esperando as propriedades do worker).
    pub props: Option<FieldPropsWindow>,
    /// Propriedades pedidas ao worker para este campo.
    pub props_for: Option<(i32, i32)>,
    /// Números da ordem de tabulação sobre os campos (`Shift+N`).
    pub show_order: bool,
    /// Janela Ordem de cálculo: os nomes em edição (`Some(vazio)` = esperando o worker).
    pub calc_order: Option<Vec<String>>,
}

/// A ordem `ids` com `id` uma posição acima (`up`) ou abaixo; `None` se não dá para mover.
pub fn moved(ids: &[i32], id: i32, up: bool) -> Option<Vec<i32>> {
    let k = ids.iter().position(|&x| x == id)?;
    let other = if up { k.checked_sub(1)? } else { k + 1 };
    if other >= ids.len() {
        return None;
    }
    let mut out = ids.to_vec();
    out.swap(k, other);
    Some(out)
}

/// Cor dos contornos dos campos no modo Preparar.
const OUTLINE: egui::Color32 = egui::Color32::from_rgb(70, 110, 220);
/// Âncora da seleção (mais escura, como no Acrobat) e guias.
const ANCHOR: egui::Color32 = egui::Color32::from_rgb(20, 50, 160);
const GUIDE: egui::Color32 = egui::Color32::from_rgb(220, 40, 160);
/// Distância (em pixels) em que o campo arrastado encaixa numa guia.
const SNAP_PX: f32 = 4.0;

impl PrepareState {
    /// Troca de ferramenta: um botão de opção depois de outra ferramenta começa outro grupo.
    pub fn set_tool(&mut self, tool: Option<NewFieldKind>) {
        if tool != self.tool {
            self.radio_group = None;
        }
        self.tool = tool;
        if tool.is_some() {
            self.selection.clear();
        }
    }

    /// A âncora (último selecionado).
    pub fn anchor(&self) -> Option<(i32, i32)> {
        self.selection.last().copied()
    }

    pub fn is_selected(&self, page: i32, id: i32) -> bool {
        self.selection.contains(&(page, id))
    }

    /// Clique com Selecionar: com `add` (Shift ou Ctrl), junta ou tira o campo da seleção; sem,
    /// um campo já selecionado vira a âncora e um outro passa a ser o único; no vazio, limpa.
    pub fn click(&mut self, hit: Option<(i32, i32)>, add: bool) {
        match (hit, add) {
            (Some(f), true) => {
                if self.is_selected(f.0, f.1) {
                    self.selection.retain(|s| *s != f);
                } else {
                    self.selection.push(f);
                }
            }
            (Some(f), false) if self.is_selected(f.0, f.1) => {
                self.selection.retain(|s| *s != f);
                self.selection.push(f);
            }
            (Some(f), false) => self.selection = vec![f],
            (None, true) => {}
            (None, false) => self.selection.clear(),
        }
    }

    /// Seleção por retângulo: os campos da página inteiros dentro dele (com `add`, juntos aos que
    /// já estavam).
    pub fn marquee_select(&mut self, fields: &[FieldInfo], page: i32, area: PdfRect, add: bool) {
        if !add {
            self.selection.clear();
        }
        for f in fields.iter().filter(|f| f.page == page) {
            let inside = f.rect.x0 >= area.x0
                && f.rect.x1 <= area.x1
                && f.rect.y0 >= area.y0
                && f.rect.y1 <= area.y1;
            if inside && !self.is_selected(page, f.id) {
                self.selection.push((page, f.id));
            }
        }
    }

    /// `Ctrl+A`: todos os campos da página.
    pub fn select_all(&mut self, fields: &[FieldInfo], page: i32) {
        self.selection = fields
            .iter()
            .filter(|f| f.page == page)
            .map(|f| (f.page, f.id))
            .collect();
    }

    /// A lista de campos chegou: seleciona o recém-criado e tira os que sumiram.
    pub fn on_fields(&mut self, fields: &[FieldInfo]) {
        if let Some((page, before)) = self.pending_select.take() {
            if let Some(f) = fields
                .iter()
                .find(|f| f.page == page && !before.contains(&f.id))
            {
                self.selection = vec![(f.page, f.id)];
            }
        }
        self.selection
            .retain(|&(p, id)| fields.iter().any(|f| f.page == p && f.id == id));
        self.clipboard
            .retain(|&(p, id)| fields.iter().any(|f| f.page == p && f.id == id));
    }

    /// Esc: solta a ferramenta, sai da visualização, depois solta a seleção. Devolve se havia o
    /// que soltar.
    pub fn escape(&mut self) -> bool {
        if self.tool.is_some() {
            self.set_tool(None);
            true
        } else if self.preview {
            self.preview = false;
            true
        } else {
            let had = !self.selection.is_empty();
            self.selection.clear();
            had
        }
    }
}

/// Os campos selecionados na página da âncora (alinhar só vale entre campos da mesma página),
/// com a âncora por último.
fn selected_on_anchor_page<'a>(st: &PrepareState, fields: &'a [FieldInfo]) -> Vec<&'a FieldInfo> {
    let Some((page, _)) = st.anchor() else {
        return Vec::new();
    };
    st.selection
        .iter()
        .filter(|(p, _)| *p == page)
        .filter_map(|&(p, id)| fields.iter().find(|f| f.page == p && f.id == id))
        .collect()
}

/// Os retângulos novos que mudaram, como uma operação (um campo: Move; vários: MoveMany).
fn moves_op(old: &[&FieldInfo], new: &[PdfRect]) -> Option<PrepareOp> {
    let mut moves: Vec<(i32, i32, PdfRect)> = old
        .iter()
        .zip(new)
        .filter(|(f, r)| f.rect != **r)
        .map(|(f, r)| (f.page, f.id, *r))
        .collect();
    match moves.len() {
        0 => None,
        1 => {
            let (page, id, rect) = moves.remove(0);
            Some(PrepareOp::Move { page, id, rect })
        }
        _ => Some(PrepareOp::MoveMany { moves }),
    }
}

/// Alinhar, centralizar, distribuir ou igualar a seleção. `page` = limites da página da âncora
/// (para centralizar).
pub fn arrange_op(
    st: &PrepareState,
    fields: &[FieldInfo],
    cmd: ArrangeCmd,
    page: Option<PdfRect>,
) -> Option<PrepareOp> {
    let sel = selected_on_anchor_page(st, fields);
    let rects: Vec<PdfRect> = sel.iter().map(|f| f.rect).collect();
    let anchor = rects.len().checked_sub(1)?;
    let new = match cmd {
        ArrangeCmd::Align(edge) if rects.len() >= 2 => arrange::align(&rects, anchor, edge),
        ArrangeCmd::Size(kind) if rects.len() >= 2 => arrange::match_size(&rects, anchor, kind),
        ArrangeCmd::Distribute(axis) => arrange::distribute(&rects, axis),
        ArrangeCmd::Center(axis) => arrange::center_on_page(&rects, page?, axis),
        ArrangeCmd::Delete => {
            return (!st.selection.is_empty()).then(|| PrepareOp::DeleteMany {
                ids: st.selection.clone(),
            })
        }
        _ => return None,
    };
    moves_op(&sel, &new)
}

/// Setas: move a seleção (todas as páginas).
pub fn nudge_op(st: &PrepareState, fields: &[FieldInfo], dx: f32, dy: f32) -> Option<PrepareOp> {
    let sel: Vec<&FieldInfo> = st
        .selection
        .iter()
        .filter_map(|&(p, id)| fields.iter().find(|f| f.page == p && f.id == id))
        .collect();
    let rects: Vec<PdfRect> = sel.iter().map(|f| f.rect).collect();
    moves_op(&sel, &arrange::nudge(&rects, dx, dy))
}

/// Campos da página no modo Preparar: contornos, colocação, seleção e arrastos. Devolve as
/// alterações a mandar ao worker e o campo cujas Propriedades foram pedidas (duplo clique). Os
/// pedidos do menu de contexto ficam em `st.cmds`.
pub fn on_page(
    ui: &mut egui::Ui,
    st: &mut PrepareState,
    fields: &[FieldInfo],
    page: i32,
    rect: egui::Rect,
    map: &PageMap,
) -> (Vec<PrepareOp>, Option<(i32, i32)>) {
    let mut ops = Vec::new();
    let mut open_props = None;
    let response = ui.interact(
        rect,
        ui.id().with(("preparar", page)),
        egui::Sense::click_and_drag(),
    );
    let pointer = response
        .interact_pointer_pos()
        .map(|p| map.to_page(p.into()));
    let on_page: Vec<&FieldInfo> = fields.iter().filter(|f| f.page == page).collect();
    let hit = |p: (f32, f32)| {
        on_page
            .iter()
            .rev()
            .find(|f| annot_hit(&[(f.id, f.rect)], p, 0.0).is_some())
            .map(|f| (f.page, f.id, f.rect))
    };
    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    match st.tool {
        Some(kind) => {
            if response.drag_started() {
                if let Some(p) = crate::edit::press_point(&response, map, pointer) {
                    st.place = Some((page, p, p));
                }
            }
            if let (Some(place), Some(p)) = (st.place.as_mut().filter(|d| d.0 == page), pointer) {
                place.2 = p;
            }
            let finished = if response.drag_stopped() {
                st.place.take().filter(|d| d.0 == page).map(|d| (d.1, d.2))
            } else if response.clicked() {
                pointer.map(|p| (p, p))
            } else {
                None
            };
            if let Some((from, to)) = finished {
                // o nome automático é do pdf-core (cliques rápidos não repetem nome); o grupo de
                // opção é escolhido aqui para os próximos botões entrarem nele
                let name = if kind == NewFieldKind::Radio {
                    let names = fields.iter().map(|f| f.name.as_str());
                    st.radio_group
                        .get_or_insert_with(|| next_name(kind, names))
                        .clone()
                } else {
                    String::new()
                };
                st.pending_select = Some((page, on_page.iter().map(|f| f.id).collect()));
                ops.push(PrepareOp::Add {
                    page,
                    rect: placed_rect(kind, from, to),
                    kind,
                    name,
                });
            }
            if let Some((_, a, b)) = st.place.filter(|d| d.0 == page) {
                paint_box(ui.painter(), &PdfRect::from_corners(a, b), map);
            }
        }
        None => {
            if response.drag_started() {
                start_drag(st, &on_page, page, map, &response, pointer, add, &hit);
            }
            if let (Some(d), Some(p)) = (st.drag.as_mut().filter(|d| d.page == page), pointer) {
                let raw = (p.0 - d.from.0, p.1 - d.from.1);
                d.delta = raw;
                st.guide_lines.retain(|g| g.0 != page);
                if d.grab.is_none() && !st.guides_off {
                    // guias: o campo pego encaixa nas bordas e centros dos outros
                    let moving: Vec<i32> = d.items.iter().map(|i| i.0).collect();
                    let others: Vec<PdfRect> = on_page
                        .iter()
                        .filter(|f| !moving.contains(&f.id))
                        .map(|f| f.rect)
                        .collect();
                    let (snapped, guides) =
                        arrange::snap(d.rect.translated(raw.0, raw.1), &others, SNAP_PX / map.zoom);
                    d.delta = (snapped.x0 - d.rect.x0, snapped.y0 - d.rect.y0);
                    st.guide_lines.extend(guides.into_iter().map(|g| (page, g)));
                }
            }
            if response.drag_stopped() {
                if let Some(d) = st.drag.take().filter(|d| d.page == page) {
                    let old: Vec<FieldInfo> = d
                        .items
                        .iter()
                        .filter_map(|(id, _)| on_page.iter().find(|f| f.id == *id))
                        .map(|f| (*f).clone())
                        .collect();
                    let refs: Vec<&FieldInfo> = old.iter().collect();
                    let new: Vec<PdfRect> = old.iter().map(|f| d.preview(f.rect)).collect();
                    ops.extend(moves_op(&refs, &new));
                }
                st.guide_lines.clear();
                if let Some(((p, a, b), add)) = st.marquee.take().filter(|m| m.0 .0 == page) {
                    st.marquee_select(fields, p, PdfRect::from_corners(a, b), add);
                }
            }
            if let (Some(((_, _, to), _)), Some(p)) =
                (st.marquee.as_mut().filter(|m| m.0 .0 == page), pointer)
            {
                *to = p;
            }
            if response.clicked() {
                st.click(pointer.and_then(hit).map(|(p, id, _)| (p, id)), add);
            }
            if response.double_clicked() {
                open_props = pointer.and_then(hit).map(|(p, id, _)| (p, id));
            }
            if response.secondary_clicked() {
                // o botão direito sobre um campo o torna a âncora (sem perder a seleção)
                let at = response
                    .interact_pointer_pos()
                    .map(|p| map.to_page(p.into()));
                if let Some((p, id, _)) = at.and_then(hit) {
                    if !st.is_selected(p, id) {
                        st.selection = vec![(p, id)];
                    } else {
                        st.click(Some((p, id)), false);
                    }
                }
            }
            let (count, can_paste) = (st.selection.len(), !st.clipboard.is_empty());
            response.context_menu(|ui| arrange_menu(ui, &mut st.cmds, count, can_paste));
            if let Some(((_, a, b), _)) = st.marquee.filter(|m| m.0 .0 == page) {
                paint_box(ui.painter(), &PdfRect::from_corners(a, b), map);
            }
        }
    }
    paint_fields(ui.painter(), st, &on_page, page, map);
    for (_, r) in st.ghosts.iter().filter(|g| g.0 == page) {
        paint_box(ui.painter(), r, map);
    }
    for (_, g) in st.guide_lines.iter().filter(|g| g.0 == page) {
        let (a, b) = match *g {
            Guide::X(x) => (map.to_screen((x, -10_000.0)), map.to_screen((x, 10_000.0))),
            Guide::Y(y) => (map.to_screen((-10_000.0, y)), map.to_screen((10_000.0, y))),
        };
        let clip = ui.painter().with_clip_rect(rect);
        clip.line_segment([a.into(), b.into()], egui::Stroke::new(1.0, GUIDE));
    }
    (ops, open_props)
}

/// Começo de um arrasto com Selecionar: alça da âncora (redimensiona), campo (move a seleção da
/// página) ou vazio (retângulo de seleção).
#[allow(clippy::too_many_arguments)]
fn start_drag(
    st: &mut PrepareState,
    on_page: &[&FieldInfo],
    page: i32,
    map: &PageMap,
    response: &egui::Response,
    pointer: Option<(f32, f32)>,
    add: bool,
    hit: &dyn Fn((f32, f32)) -> Option<(i32, i32, PdfRect)>,
) {
    let Some(p) = crate::edit::press_point(response, map, pointer) else {
        return;
    };
    // só um selecionado: as alças dele redimensionam
    let single = match st.selection.as_slice() {
        [(sp, id)] if *sp == page => on_page.iter().find(|f| f.id == *id).copied(),
        _ => None,
    };
    if let Some(f) = single {
        if let Some(h) = handle_at(f.rect, p, 6.0 / map.zoom) {
            st.drag = Some(Drag {
                page,
                items: vec![(f.id, f.rect)],
                rect: f.rect,
                grab: Some(h),
                from: p,
                delta: (0.0, 0.0),
            });
            return;
        }
    }
    match hit(p) {
        Some((_, id, rect)) => {
            if !st.is_selected(page, id) {
                st.click(Some((page, id)), add);
            }
            let items = on_page
                .iter()
                .filter(|f| st.is_selected(page, f.id))
                .map(|f| (f.id, f.rect))
                .collect();
            st.drag = Some(Drag {
                page,
                items,
                rect,
                grab: None,
                from: p,
                delta: (0.0, 0.0),
            });
        }
        None => st.marquee = Some(((page, p, p), add)),
    }
}

/// Menu de contexto do Preparar (como o do Acrobat).
pub fn arrange_menu(ui: &mut egui::Ui, cmds: &mut Vec<ArrangeCmd>, count: usize, can_paste: bool) {
    let mut item = |ui: &mut egui::Ui, on: bool, label: &str, cmd: ArrangeCmd| {
        if ui.add_enabled(on, egui::Button::new(label)).clicked() {
            cmds.push(cmd);
            ui.close();
        }
    };
    let (one, two, three) = (count >= 1, count >= 2, count >= 3);
    item(ui, count == 1, "Propriedades…", ArrangeCmd::Props);
    ui.separator();
    item(ui, one, "Copiar", ArrangeCmd::Copy);
    item(ui, can_paste, "Colar", ArrangeCmd::Paste);
    item(ui, one, "Excluir", ArrangeCmd::Delete);
    ui.separator();
    ui.menu_button("Alinhar, distribuir ou centralizar", |ui| {
        for (edge, label) in ALIGN_ITEMS {
            item(ui, two, label, ArrangeCmd::Align(edge));
        }
        ui.separator();
        item(
            ui,
            three,
            "Distribuir verticalmente",
            ArrangeCmd::Distribute(Axis::V),
        );
        item(
            ui,
            three,
            "Distribuir horizontalmente",
            ArrangeCmd::Distribute(Axis::H),
        );
        ui.separator();
        for (axis, label) in CENTER_ITEMS {
            item(ui, one, label, ArrangeCmd::Center(axis));
        }
    });
    ui.menu_button("Definir campos com o mesmo tamanho", |ui| {
        for (kind, label) in SIZE_ITEMS {
            item(ui, two, label, ArrangeCmd::Size(kind));
        }
    });
    ui.separator();
    item(ui, count == 1, "Duplicar…", ArrangeCmd::Duplicate);
    item(ui, count == 1, "Criar várias cópias…", ArrangeCmd::Copies);
}

/// Itens de alinhar (com as teclas do Acrobat).
pub const ALIGN_ITEMS: [(Edge, &str); 6] = [
    (Edge::Left, "Alinhar à esquerda (L)"),
    (Edge::Right, "Alinhar à direita (R)"),
    (Edge::Top, "Alinhar no topo (T)"),
    (Edge::Bottom, "Alinhar na base (B)"),
    (Edge::CenterV, "Alinhar verticalmente (V)"),
    (Edge::CenterH, "Alinhar horizontalmente (H)"),
];
pub const CENTER_ITEMS: [(Axis, &str); 3] = [
    (Axis::H, "Centralizar horizontalmente (Shift+H)"),
    (Axis::V, "Centralizar verticalmente (Shift+V)"),
    (Axis::Both, "Centralizar nos dois sentidos"),
];
pub const SIZE_ITEMS: [(SizeKind, &str); 3] = [
    (SizeKind::Height, "Altura"),
    (SizeKind::Width, "Largura"),
    (SizeKind::Both, "Ambos"),
];

/// Retângulo de colocação ou de seleção.
fn paint_box(painter: &egui::Painter, r: &PdfRect, map: &PageMap) {
    painter.rect_stroke(
        screen_rect(r, map),
        0.0,
        egui::Stroke::new(1.5, OUTLINE),
        egui::StrokeKind::Middle,
    );
}

/// Contornos e nomes dos campos; a seleção com contorno forte, a âncora mais escura e com alças
/// (e a prévia do arrasto).
fn paint_fields(
    painter: &egui::Painter,
    st: &PrepareState,
    fields: &[&FieldInfo],
    page: i32,
    map: &PageMap,
) {
    let fill = egui::Color32::from_rgba_unmultiplied(70, 110, 220, 28);
    let anchor = st.anchor();
    for (k, f) in fields.iter().enumerate() {
        let dragging = st
            .drag
            .as_ref()
            .filter(|d| d.page == page && d.items.iter().any(|i| i.0 == f.id));
        let rect = dragging.map_or(f.rect, |d| d.preview(f.rect));
        let r = screen_rect(&rect, map);
        painter.rect_filled(r, 0.0, fill);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, OUTLINE),
            egui::StrokeKind::Inside,
        );
        // oculto (Mostrar/ocultar): continua editável, esmaecido, como no Acrobat
        let (label, color) = if f.hidden {
            (format!("{} (oculto)", f.name), OUTLINE.gamma_multiply(0.5))
        } else {
            (f.name.clone(), OUTLINE)
        };
        painter.text(
            r.left_top() + egui::vec2(2.0, 1.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(10.0),
            color,
        );
        if st.show_order {
            // número da ordem de tabulação, como o Acrobat mostra com Shift+N
            let at = r.right_top() + egui::vec2(-9.0, 9.0);
            painter.circle_filled(at, 8.0, OUTLINE);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                (k + 1).to_string(),
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        }
        if st.is_selected(page, f.id) {
            let is_anchor = anchor == Some((page, f.id));
            painter.rect_stroke(
                r.expand(1.0),
                0.0,
                egui::Stroke::new(2.0, if is_anchor { ANCHOR } else { OUTLINE }),
                egui::StrokeKind::Outside,
            );
            if is_anchor && st.selection.len() == 1 {
                for h in Handle::ALL {
                    crate::edit::paint_handle(
                        painter,
                        map.to_screen(h.point(rect)).into(),
                        OUTLINE,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_a_field_in_the_tab_order() {
        assert_eq!(moved(&[1, 2, 3], 3, true), Some(vec![1, 3, 2]));
        assert_eq!(moved(&[1, 2, 3], 1, false), Some(vec![2, 1, 3]));
        assert_eq!(moved(&[1, 2, 3], 1, true), None, "o primeiro não sobe");
        assert_eq!(moved(&[1, 2, 3], 9, true), None);
    }

    #[test]
    fn a_new_field_is_selected_when_the_list_arrives() {
        // antes do campo novo havia só o 1: o que aparecer na página é o novo
        let mut st = PrepareState {
            pending_select: Some((0, vec![1])),
            ..Default::default()
        };
        let field = |id, name: &str| FieldInfo {
            id,
            page: 0,
            name: name.into(),
            kind: pdf_core::FieldKind::Signature,
            rect: PdfRect::new(0.0, 0.0, 10.0, 10.0),
            value: String::new(),
            readonly: false,
            required: false,
            tooltip: String::new(),
            keystroke: Default::default(),
            hidden: false,
            actions: Vec::new(),
            format: Default::default(),
            validate: Default::default(),
            calc: Default::default(),
        };
        st.on_fields(&[field(1, "Texto1"), field(7, "Texto2")]);
        assert_eq!(st.selection, vec![(0, 7)]);
        st.on_fields(&[field(1, "Texto1")]);
        assert!(st.selection.is_empty(), "excluído sai da seleção");
    }

    #[test]
    fn radios_share_a_group_until_the_tool_changes() {
        let mut st = PrepareState::default();
        st.set_tool(Some(NewFieldKind::Radio));
        st.radio_group = Some("Grupo1".into());
        st.set_tool(Some(NewFieldKind::Radio));
        assert_eq!(st.radio_group.as_deref(), Some("Grupo1"));
        st.set_tool(Some(NewFieldKind::Text));
        assert!(st.radio_group.is_none());
    }

    #[test]
    fn escape_drops_the_tool_then_the_selection() {
        let mut st = PrepareState {
            tool: Some(NewFieldKind::Text),
            selection: vec![(0, 1)],
            ..Default::default()
        };
        assert!(st.escape() && st.tool.is_none());
        st.selection = vec![(0, 1)];
        assert!(st.escape() && st.selection.is_empty());
        assert!(!st.escape());
    }

    fn field_at(id: i32, x: f32, y: f32) -> FieldInfo {
        FieldInfo {
            id,
            page: 0,
            name: format!("C{id}"),
            kind: pdf_core::FieldKind::Signature,
            rect: PdfRect::new(x, y, x + 40.0, y + 20.0),
            value: String::new(),
            readonly: false,
            required: false,
            tooltip: String::new(),
            keystroke: Default::default(),
            hidden: false,
            actions: Vec::new(),
            format: Default::default(),
            validate: Default::default(),
            calc: Default::default(),
        }
    }

    #[test]
    fn shift_click_adds_and_removes() {
        let mut st = PrepareState::default();
        st.click(Some((0, 1)), false);
        st.click(Some((0, 2)), true);
        assert_eq!(st.selection, vec![(0, 1), (0, 2)]);
        assert_eq!(st.anchor(), Some((0, 2)));
        st.click(Some((0, 1)), false);
        assert_eq!(
            st.selection,
            vec![(0, 2), (0, 1)],
            "clicar num selecionado o faz âncora"
        );
        st.click(Some((0, 1)), true);
        assert_eq!(st.selection, vec![(0, 2)], "Shift+clique tira");
        st.click(Some((0, 3)), false);
        assert_eq!(st.selection, vec![(0, 3)]);
        st.click(None, false);
        assert!(st.selection.is_empty());
    }

    #[test]
    fn marquee_selects_inside() {
        let fields = [
            field_at(1, 10.0, 10.0),
            field_at(2, 100.0, 10.0),
            field_at(3, 10.0, 300.0),
        ];
        let mut st = PrepareState::default();
        st.marquee_select(&fields, 0, PdfRect::new(0.0, 0.0, 200.0, 100.0), false);
        assert_eq!(st.selection, vec![(0, 1), (0, 2)]);
        st.marquee_select(&fields, 0, PdfRect::new(0.0, 250.0, 60.0, 400.0), true);
        assert_eq!(st.selection.len(), 3, "com Shift junta");
        st.marquee_select(&fields, 0, PdfRect::new(0.0, 0.0, 30.0, 30.0), false);
        assert!(st.selection.is_empty(), "só os inteiros dentro");
    }

    #[test]
    fn align_key_sends_move_many() {
        let fields = [
            field_at(1, 10.0, 10.0),
            field_at(2, 100.0, 50.0),
            field_at(3, 60.0, 90.0),
        ];
        let mut st = PrepareState {
            selection: vec![(0, 1), (0, 3), (0, 2)],
            ..Default::default()
        };
        let op = arrange_op(&st, &fields, ArrangeCmd::Align(Edge::Left), None);
        let Some(PrepareOp::MoveMany { moves }) = op else {
            panic!("{op:?}");
        };
        assert_eq!(moves.len(), 2, "a âncora (2) não se move");
        assert!(moves.iter().all(|(_, _, r)| r.x0 == 100.0));
        assert_eq!(
            arrange_op(&st, &fields, ArrangeCmd::Delete, None),
            Some(PrepareOp::DeleteMany {
                ids: vec![(0, 1), (0, 3), (0, 2)]
            })
        );
        st.selection = vec![(0, 1)];
        assert_eq!(
            arrange_op(&st, &fields, ArrangeCmd::Align(Edge::Left), None),
            None
        );
    }

    #[test]
    fn arrow_nudges_the_selection() {
        let fields = [field_at(1, 10.0, 10.0), field_at(2, 100.0, 50.0)];
        let mut st = PrepareState {
            selection: vec![(0, 1), (0, 2)],
            ..Default::default()
        };
        let Some(PrepareOp::MoveMany { moves }) = nudge_op(&st, &fields, 0.0, 10.0) else {
            panic!();
        };
        assert_eq!(moves[0].2, PdfRect::new(10.0, 20.0, 50.0, 40.0));
        st.selection = vec![(0, 2)];
        assert!(matches!(
            nudge_op(&st, &fields, 1.0, 0.0),
            Some(PrepareOp::Move { id: 2, .. })
        ));
    }
}
