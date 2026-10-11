//! Preparar formulário (como no Acrobat): os campos aparecem com contorno e nome; clicar ou
//! arrastar com uma ferramenta cria o campo; com Selecionar, clicar seleciona, arrastar move, as
//! alças redimensionam e o duplo clique abre as Propriedades.
use crate::edit::screen_rect;
use crate::shell::field_props::FieldPropsWindow;
use app_state::annot_geom::{annot_hit, handle_at, resized, Handle};
use app_state::prepare::{next_name, placed_rect};
use app_state::transform::PageMap;
use pdf_core::{FieldInfo, NewFieldKind, PdfRect, PrepareOp};

/// Arrasto em andamento: mover ou redimensionar o campo selecionado.
#[derive(Clone, Copy)]
struct Drag {
    page: i32,
    id: i32,
    rect: PdfRect,
    grab: Option<Handle>,
    from: (f32, f32),
    to: (f32, f32),
}

impl Drag {
    fn preview(&self) -> PdfRect {
        let (dx, dy) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        match self.grab {
            None => self.rect.translated(dx, dy),
            Some(h) => {
                let (x, y) = h.point(self.rect);
                resized(self.rect, h, (x + dx, y + dy), 6.0)
            }
        }
    }
}

/// Arrasto de criação de um campo: (página, de, até).
type Placing = (i32, (f32, f32), (f32, f32));

#[derive(Default)]
pub struct PrepareState {
    /// Ferramenta de campo ativa (`None` = Selecionar).
    pub tool: Option<NewFieldKind>,
    pub selected: Option<(i32, i32)>,
    drag: Option<Drag>,
    /// Arrasto de criação: (página, de, até).
    place: Option<Placing>,
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

impl PrepareState {
    /// Troca de ferramenta: um botão de opção depois de outra ferramenta começa outro grupo.
    pub fn set_tool(&mut self, tool: Option<NewFieldKind>) {
        if tool != self.tool {
            self.radio_group = None;
        }
        self.tool = tool;
        if tool.is_some() {
            self.selected = None;
        }
    }

    /// A lista de campos chegou: seleciona o recém-criado.
    pub fn on_fields(&mut self, fields: &[FieldInfo]) {
        if let Some((page, before)) = self.pending_select.take() {
            self.selected = fields
                .iter()
                .find(|f| f.page == page && !before.contains(&f.id))
                .map(|f| (f.page, f.id));
        }
        if let Some((p, id)) = self.selected {
            if !fields.iter().any(|f| f.page == p && f.id == id) {
                self.selected = None;
            }
        }
    }

    /// Esc: solta a ferramenta, depois a seleção. Devolve se havia o que soltar.
    pub fn escape(&mut self) -> bool {
        if self.tool.is_some() {
            self.set_tool(None);
            true
        } else {
            self.selected.take().is_some()
        }
    }
}

/// Campos da página no modo Preparar: contornos, colocação, seleção e arrastos. Devolve as
/// alterações a mandar ao worker e o campo cujas Propriedades foram pedidas (duplo clique).
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
                ui.painter().rect_stroke(
                    screen_rect(&PdfRect::from_corners(a, b), map),
                    0.0,
                    egui::Stroke::new(1.5, OUTLINE),
                    egui::StrokeKind::Middle,
                );
            }
        }
        None => {
            if response.drag_started() {
                let p = crate::edit::press_point(&response, map, pointer);
                let selected_rect = st
                    .selected
                    .filter(|s| s.0 == page)
                    .and_then(|(_, id)| on_page.iter().find(|f| f.id == id))
                    .map(|f| f.rect);
                let grab = p.and_then(|p| {
                    let handle = selected_rect.and_then(|r| handle_at(r, p, 6.0 / map.zoom));
                    match (handle, selected_rect) {
                        (Some(h), Some(r)) => Some((st.selected?.1, r, Some(h), p)),
                        _ => hit(p).map(|(_, id, r)| (id, r, None, p)),
                    }
                });
                if let Some((id, r, grab, p)) = grab {
                    st.selected = Some((page, id));
                    st.drag = Some(Drag {
                        page,
                        id,
                        rect: r,
                        grab,
                        from: p,
                        to: p,
                    });
                }
            }
            if let Some(d) = st.drag.as_mut().filter(|d| d.page == page) {
                if let Some(p) = pointer {
                    d.to = p;
                }
                if response.drag_stopped() {
                    let rect = d.preview();
                    if rect != d.rect {
                        ops.push(PrepareOp::Move {
                            page,
                            id: d.id,
                            rect,
                        });
                    }
                    st.drag = None;
                }
            } else if response.clicked() {
                st.selected = pointer.and_then(hit).map(|(p, id, _)| (p, id));
            }
            if response.double_clicked() {
                open_props = pointer.and_then(hit).map(|(p, id, _)| (p, id));
            }
        }
    }
    paint_fields(ui.painter(), st, &on_page, page, map);
    (ops, open_props)
}

/// Contornos e nomes dos campos; o selecionado com alças (e a prévia do arrasto).
fn paint_fields(
    painter: &egui::Painter,
    st: &PrepareState,
    fields: &[&FieldInfo],
    page: i32,
    map: &PageMap,
) {
    let fill = egui::Color32::from_rgba_unmultiplied(70, 110, 220, 28);
    for (k, f) in fields.iter().enumerate() {
        let dragging = st.drag.filter(|d| d.page == page && d.id == f.id);
        let rect = dragging.map_or(f.rect, |d| d.preview());
        let r = screen_rect(&rect, map);
        painter.rect_filled(r, 0.0, fill);
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, OUTLINE),
            egui::StrokeKind::Inside,
        );
        painter.text(
            r.left_top() + egui::vec2(2.0, 1.0),
            egui::Align2::LEFT_TOP,
            &f.name,
            egui::FontId::proportional(10.0),
            OUTLINE,
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
        if st.selected == Some((page, f.id)) {
            painter.rect_stroke(
                r.expand(1.0),
                0.0,
                egui::Stroke::new(2.0, OUTLINE),
                egui::StrokeKind::Outside,
            );
            for h in Handle::ALL {
                crate::edit::paint_handle(painter, map.to_screen(h.point(rect)).into(), OUTLINE);
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
            display: String::new(),
            format: Default::default(),
            validate: Default::default(),
            calc: Default::default(),
        };
        st.on_fields(&[field(1, "Texto1"), field(7, "Texto2")]);
        assert_eq!(st.selected, Some((0, 7)));
        st.on_fields(&[field(1, "Texto1")]);
        assert_eq!(st.selected, None, "excluído sai da seleção");
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
            selected: Some((0, 1)),
            ..Default::default()
        };
        assert!(st.escape() && st.tool.is_none());
        st.selected = Some((0, 1));
        assert!(st.escape() && st.selected.is_none());
        assert!(!st.escape());
    }
}
