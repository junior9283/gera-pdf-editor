//! Painel Marcadores (como no Acrobat): árvore com expandir e recolher, novo marcador (Ctrl+B),
//! renomear no lugar (F2, duplo clique), excluir, arrastar para reordenar e aninhar, estilo.

use egui::Key;
use pdf_core::{OutlineNode, OutlineOp};

/// Onde o marcador arrastado cai em relação ao item sob o ponteiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Before,
    /// Último filho do item.
    Child,
    After,
}

/// Terço de cima: antes; do meio: filho; de baixo: depois.
pub fn drop_zone(y_in_row: f32, row_height: f32) -> Zone {
    let t = y_in_row / row_height.max(1.0);
    if t < 1.0 / 3.0 {
        Zone::Before
    } else if t < 2.0 / 3.0 {
        Zone::Child
    } else {
        Zone::After
    }
}

fn node(nodes: &[OutlineNode], id: i32) -> Option<&OutlineNode> {
    nodes.iter().find(|n| n.id == id)
}

/// `id` é `ancestor` ou está dentro dele.
fn is_within(nodes: &[OutlineNode], mut id: i32, ancestor: i32) -> bool {
    for _ in 0..nodes.len() + 1 {
        if id == ancestor {
            return true;
        }
        match node(nodes, id).and_then(|n| n.parent) {
            Some(p) => id = p,
            None => return false,
        }
    }
    false
}

fn siblings(nodes: &[OutlineNode], parent: Option<i32>) -> Vec<i32> {
    nodes
        .iter()
        .filter(|n| n.parent == parent)
        .map(|n| n.id)
        .collect()
}

/// A operação de soltar `dragged` na zona `zone` de `target`; `None` se não faz sentido (sobre si
/// mesmo ou dentro de um descendente).
pub fn drop_op(nodes: &[OutlineNode], dragged: i32, target: i32, zone: Zone) -> Option<OutlineOp> {
    if is_within(nodes, target, dragged) {
        return None;
    }
    let t = node(nodes, target)?;
    let (parent, after) = match zone {
        Zone::Child => (Some(target), siblings(nodes, Some(target)).last().copied()),
        Zone::After => (t.parent, Some(target)),
        Zone::Before => {
            let sibs: Vec<i32> = siblings(nodes, t.parent)
                .into_iter()
                .filter(|&s| s != dragged)
                .collect();
            let pos = sibs.iter().position(|&s| s == target)?;
            (t.parent, pos.checked_sub(1).map(|k| sibs[k]))
        }
    };
    Some(OutlineOp::Move {
        id: dragged,
        parent,
        after,
    })
}

/// Os marcadores à mostra (os filhos de um recolhido ficam escondidos).
pub fn visible(nodes: &[OutlineNode]) -> Vec<&OutlineNode> {
    let mut out = Vec::new();
    let mut hidden_below: Option<usize> = None;
    for n in nodes {
        if let Some(d) = hidden_below {
            if n.depth > d {
                continue;
            }
            hidden_below = None;
        }
        out.push(n);
        if !n.open {
            hidden_below = Some(n.depth);
        }
    }
    out
}

/// Ctrl+B: depois do selecionado (ou no fim do topo), com o texto selecionado na página como
/// título (a primeira linha, até 128 caracteres) ou "Sem título".
pub fn new_bookmark(
    nodes: &[OutlineNode],
    selected: Option<i32>,
    selected_text: &str,
    page: i32,
    at: Option<(f32, f32)>,
) -> OutlineOp {
    let first_line = selected_text.trim().lines().next().unwrap_or("").trim();
    let title: String = if first_line.is_empty() {
        "Sem título".into()
    } else {
        first_line.chars().take(128).collect()
    };
    let (parent, after) = match selected.and_then(|id| node(nodes, id)) {
        Some(n) => (n.parent, Some(n.id)),
        None => (None, siblings(nodes, None).last().copied()),
    };
    OutlineOp::Add {
        parent,
        after,
        title,
        page,
        at,
    }
}

/// O que uma tecla faz no painel.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Select(i32),
    Go(i32),
    Rename(i32),
    Op(OutlineOp),
}

pub fn key_action(nodes: &[OutlineNode], selected: Option<i32>, key: Key) -> Option<KeyAction> {
    let shown = visible(nodes);
    let pos = selected.and_then(|id| shown.iter().position(|n| n.id == id));
    let Some(pos) = pos else {
        return matches!(key, Key::ArrowDown | Key::ArrowUp)
            .then(|| shown.first().map(|n| KeyAction::Select(n.id)))
            .flatten();
    };
    let n = shown[pos];
    let has_kids = nodes.iter().any(|k| k.parent == Some(n.id));
    match key {
        Key::ArrowDown => shown.get(pos + 1).map(|k| KeyAction::Select(k.id)),
        Key::ArrowUp => pos.checked_sub(1).map(|p| KeyAction::Select(shown[p].id)),
        Key::ArrowRight if has_kids && !n.open => Some(KeyAction::Op(OutlineOp::SetOpen {
            id: n.id,
            open: true,
        })),
        Key::ArrowLeft if has_kids && n.open => Some(KeyAction::Op(OutlineOp::SetOpen {
            id: n.id,
            open: false,
        })),
        Key::ArrowLeft => n.parent.map(KeyAction::Select),
        Key::Delete => Some(KeyAction::Op(OutlineOp::Delete { id: n.id })),
        Key::F2 => Some(KeyAction::Rename(n.id)),
        Key::Enter => Some(KeyAction::Go(n.id)),
        _ => None,
    }
}

/// O que o painel pede à aba.
#[derive(Debug, Clone, PartialEq)]
pub enum BookmarkAction {
    /// Ir ao destino do marcador.
    Go {
        page: i32,
        at: Option<(f32, f32)>,
    },
    Op(OutlineOp),
    /// Novo marcador (o botão; a aba completa com a vista e o texto selecionado).
    New,
    /// Definir o destino do marcador na vista atual.
    DestinationHere(i32),
}

/// Estilo em edição (Propriedades do marcador).
#[derive(Debug, Clone)]
struct StyleDraft {
    id: i32,
    color: [f32; 3],
    bold: bool,
    italic: bool,
}

/// Estado do painel.
#[derive(Debug, Default)]
pub struct BookmarksPanel {
    pub selected: Option<i32>,
    /// Renomeação no lugar: (marcador, texto).
    pub renaming: Option<(i32, String)>,
    dragging: Option<i32>,
    /// Renomeação que já recebeu o "selecionar tudo" inicial.
    rename_selected: Option<i32>,
    /// O painel recebeu um clique: as setas, Enter, Delete e F2 são dele.
    pub focused: bool,
    style: Option<StyleDraft>,
}

impl BookmarksPanel {
    /// Depois de criar um marcador: seleciona e abre a renomeação (como o Acrobat).
    pub fn created(&mut self, id: i32, nodes: &[OutlineNode]) {
        self.selected = Some(id);
        self.focused = true;
        self.renaming = node(nodes, id).map(|n| (id, n.title.clone()));
    }

    /// Teclas do painel focado (fora da renomeação).
    pub fn keys(&mut self, ctx: &egui::Context, nodes: &[OutlineNode]) -> Vec<BookmarkAction> {
        if !self.focused || self.renaming.is_some() || ctx.egui_wants_keyboard_input() {
            return Vec::new();
        }
        let keys = [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::Enter,
            Key::Delete,
            Key::F2,
        ];
        let pressed: Vec<Key> = ctx.input_mut(|i| {
            keys.into_iter()
                .filter(|&k| i.consume_key(egui::Modifiers::NONE, k))
                .collect()
        });
        let mut out = Vec::new();
        for key in pressed {
            match key_action(nodes, self.selected, key) {
                Some(KeyAction::Select(id)) => self.selected = Some(id),
                Some(KeyAction::Rename(id)) => {
                    self.renaming = node(nodes, id).map(|n| (id, n.title.clone()))
                }
                Some(KeyAction::Go(id)) => {
                    if let Some(n) = node(nodes, id).filter(|n| n.page.is_some()) {
                        out.push(BookmarkAction::Go {
                            page: n.page.unwrap_or(0),
                            at: n.at,
                        });
                    }
                }
                Some(KeyAction::Op(op)) => out.push(BookmarkAction::Op(op)),
                None => {}
            }
        }
        out
    }

    /// Desenha o painel; devolve o que foi pedido.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        nodes: &[OutlineNode],
        can_edit: bool,
    ) -> Vec<BookmarkAction> {
        let mut out = Vec::new();
        // marcadores com filhos (uma passada por quadro, não uma por linha)
        let parents: std::collections::HashSet<i32> =
            nodes.iter().filter_map(|n| n.parent).collect();
        ui.horizontal(|ui| {
            let new = ui
                .add_enabled(can_edit, egui::Button::new("+").small())
                .on_hover_text("Novo marcador (Ctrl+B)");
            if new.clicked() {
                out.push(BookmarkAction::New);
            }
            let del = ui
                .add_enabled(
                    can_edit && self.selected.is_some(),
                    egui::Button::new("🗑").small(),
                )
                .on_hover_text("Excluir marcador (Delete)");
            if let (true, Some(id)) = (del.clicked(), self.selected) {
                out.push(BookmarkAction::Op(OutlineOp::Delete { id }));
            }
            for (icon, open, tip) in [("⊞", true, "Expandir tudo"), ("⊟", false, "Recolher tudo")]
            {
                if ui.small_button(icon).on_hover_text(tip).clicked() {
                    out.extend(
                        nodes
                            .iter()
                            .filter(|n| n.open != open)
                            .filter(|n| parents.contains(&n.id))
                            .map(|n| BookmarkAction::Op(OutlineOp::SetOpen { id: n.id, open })),
                    );
                }
            }
        });
        ui.separator();
        if nodes.is_empty() {
            ui.weak("Este documento não tem marcadores. Use Ctrl+B para criar um.");
            return out;
        }
        let shown = visible(nodes);
        let mut drop: Option<(i32, Zone)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for n in &shown {
                    let row = ui.horizontal(|ui| {
                        ui.add_space(14.0 * n.depth as f32);
                        if parents.contains(&n.id) {
                            let arrow = if n.open { "⏷" } else { "⏵" };
                            if ui
                                .add(egui::Button::new(arrow).frame(false).small())
                                .clicked()
                            {
                                out.push(BookmarkAction::Op(OutlineOp::SetOpen {
                                    id: n.id,
                                    open: !n.open,
                                }));
                            }
                        } else {
                            ui.add_space(16.0);
                        }
                        if let Some((id, text)) =
                            self.renaming.as_mut().filter(|(id, _)| *id == n.id)
                        {
                            let mut out_edit = egui::TextEdit::singleline(text).show(ui);
                            // ao começar a renomear, o nome vem todo selecionado (como no Acrobat)
                            if self.rename_selected != Some(*id) {
                                self.rename_selected = Some(*id);
                                let all = egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(text.chars().count()),
                                );
                                out_edit.state.cursor.set_char_range(Some(all));
                                out_edit.state.store(ui.ctx(), out_edit.response.id);
                            }
                            let edit = out_edit.response;
                            edit.request_focus();
                            let enter = ui.input(|i| i.key_pressed(Key::Enter));
                            let esc = ui.input(|i| i.key_pressed(Key::Escape));
                            if esc {
                                self.renaming = None;
                            } else if edit.lost_focus() || enter {
                                let title = text.trim().to_owned();
                                if !title.is_empty() {
                                    out.push(BookmarkAction::Op(OutlineOp::Rename {
                                        id: *id,
                                        title,
                                    }));
                                }
                                self.renaming = None;
                            }
                            return None;
                        }
                        let mut text = egui::RichText::new(&n.title);
                        if let Some(c) = n.color {
                            text = text.color(crate::shell::annot_props::color32(c));
                        }
                        if n.bold {
                            text = text.strong();
                        }
                        if n.italic {
                            text = text.italics();
                        }
                        let selected = self.selected == Some(n.id);
                        Some(
                            ui.add(
                                egui::Button::selectable(selected, text)
                                    .sense(egui::Sense::click_and_drag()),
                            ),
                        )
                    });
                    let Some(resp) = row.inner else { continue };
                    if resp.clicked() {
                        self.selected = Some(n.id);
                        self.focused = true;
                        if let Some(page) = n.page {
                            out.push(BookmarkAction::Go { page, at: n.at });
                        }
                    }
                    if resp.double_clicked() && can_edit {
                        self.renaming = Some((n.id, n.title.clone()));
                    }
                    if resp.drag_started() && can_edit {
                        self.dragging = Some(n.id);
                    }
                    // alvo do arraste: a linha sob o ponteiro e o terço dela
                    if let (Some(_), Some(p)) = (self.dragging, ui.ctx().pointer_interact_pos()) {
                        let r = row.response.rect;
                        if r.contains(p) {
                            let zone = drop_zone(p.y - r.top(), r.height());
                            drop = Some((n.id, zone));
                            let y = match zone {
                                Zone::Before => r.top(),
                                Zone::Child => r.center().y,
                                Zone::After => r.bottom(),
                            };
                            let x0 = r.left()
                                + 14.0
                                    * (n.depth as f32 + f32::from(u8::from(zone == Zone::Child)));
                            ui.painter().hline(
                                x0..=r.right(),
                                y,
                                egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                            );
                        }
                    }
                    resp.context_menu(|ui| {
                        self.selected = Some(n.id);
                        if !can_edit {
                            ui.label("O documento não permite alterar os marcadores.");
                            return;
                        }
                        if ui.button("Novo marcador").clicked() {
                            out.push(BookmarkAction::New);
                            ui.close();
                        }
                        if ui.button("Renomear").clicked() {
                            self.renaming = Some((n.id, n.title.clone()));
                            ui.close();
                        }
                        if ui.button("Excluir").clicked() {
                            out.push(BookmarkAction::Op(OutlineOp::Delete { id: n.id }));
                            ui.close();
                        }
                        if ui
                            .button("Definir destino")
                            .on_hover_text("A página e a posição da vista atual")
                            .clicked()
                        {
                            out.push(BookmarkAction::DestinationHere(n.id));
                            ui.close();
                        }
                        if ui.button("Propriedades…").clicked() {
                            self.style = Some(StyleDraft {
                                id: n.id,
                                color: n.color.unwrap_or([0.0; 3]),
                                bold: n.bold,
                                italic: n.italic,
                            });
                            ui.close();
                        }
                    });
                }
            });
        if let Some(dragged) = self.dragging {
            if ui.input(|i| !i.pointer.any_down()) {
                self.dragging = None;
                if let Some((target, zone)) = drop {
                    if let Some(op) = drop_op(nodes, dragged, target, zone) {
                        out.push(BookmarkAction::Op(op));
                        // solto como filho: o pai se abre, para o item continuar à vista
                        if zone == Zone::Child && node(nodes, target).is_some_and(|t| !t.open) {
                            out.push(BookmarkAction::Op(OutlineOp::SetOpen {
                                id: target,
                                open: true,
                            }));
                        }
                    }
                }
            }
        }
        if self.renaming.is_none() {
            self.rename_selected = None;
        }
        self.style_window(ui.ctx(), &mut out);
        out
    }

    fn style_window(&mut self, ctx: &egui::Context, out: &mut Vec<BookmarkAction>) {
        let Some(draft) = self.style.as_mut() else {
            return;
        };
        let mut done = None;
        egui::Window::new("Propriedades do marcador")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Cor:");
                    ui.color_edit_button_rgb(&mut draft.color);
                });
                ui.checkbox(&mut draft.bold, "Negrito");
                ui.checkbox(&mut draft.italic, "Itálico");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                let d = self.style.take().expect("aberta");
                let color = (d.color != [0.0; 3]).then_some(d.color);
                out.push(BookmarkAction::Op(OutlineOp::Style {
                    id: d.id,
                    color,
                    bold: d.bold,
                    italic: d.italic,
                }));
            }
            Some(false) => self.style = None,
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::OutlineNode;

    fn node(id: i32, parent: Option<i32>, depth: usize, open: bool) -> OutlineNode {
        OutlineNode {
            id,
            parent,
            depth,
            title: format!("m{id}"),
            page: Some(0),
            at: None,
            open,
            color: None,
            bold: false,
            italic: false,
        }
    }

    /// 1
    ///   2
    ///   3
    /// 4 (fechado)
    ///   5
    fn tree() -> Vec<OutlineNode> {
        vec![
            node(1, None, 0, true),
            node(2, Some(1), 1, false),
            node(3, Some(1), 1, false),
            node(4, None, 0, false),
            node(5, Some(4), 1, false),
        ]
    }

    #[test]
    fn drop_zone_by_thirds() {
        assert_eq!(drop_zone(1.0, 30.0), Zone::Before);
        assert_eq!(drop_zone(15.0, 30.0), Zone::Child);
        assert_eq!(drop_zone(29.0, 30.0), Zone::After);
    }

    #[test]
    fn dropping_builds_the_move() {
        let t = tree();
        // antes do 3: depois do irmão anterior (2), mesmo pai
        assert_eq!(
            drop_op(&t, 4, 3, Zone::Before),
            Some(OutlineOp::Move {
                id: 4,
                parent: Some(1),
                after: Some(2)
            })
        );
        // antes do primeiro: no começo
        assert_eq!(
            drop_op(&t, 3, 2, Zone::Before),
            Some(OutlineOp::Move {
                id: 3,
                parent: Some(1),
                after: None
            })
        );
        // filho do 4: no fim dos filhos dele
        assert_eq!(
            drop_op(&t, 2, 4, Zone::Child),
            Some(OutlineOp::Move {
                id: 2,
                parent: Some(4),
                after: Some(5)
            })
        );
        assert_eq!(
            drop_op(&t, 4, 1, Zone::After),
            Some(OutlineOp::Move {
                id: 4,
                parent: None,
                after: Some(1)
            })
        );
        // sobre si mesmo ou dentro de um descendente: nada
        assert_eq!(drop_op(&t, 1, 1, Zone::Child), None);
        assert_eq!(drop_op(&t, 1, 3, Zone::After), None);
    }

    #[test]
    fn closed_bookmarks_hide_their_children() {
        let ids: Vec<i32> = visible(&tree()).iter().map(|n| n.id).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
    }

    #[test]
    fn ctrl_b_adds_after_the_selected_with_the_selected_text() {
        let t = tree();
        let op = new_bookmark(&t, Some(2), "  Introdução\nresto ", 3, Some((0.0, 40.0)));
        assert_eq!(
            op,
            OutlineOp::Add {
                parent: Some(1),
                after: Some(2),
                title: "Introdução".into(),
                page: 3,
                at: Some((0.0, 40.0)),
            }
        );
        // sem seleção: no fim do topo, "Sem título"
        let op = new_bookmark(&t, None, "", 0, None);
        assert!(
            matches!(op, OutlineOp::Add { parent: None, after: Some(4), ref title, .. } if title == "Sem título")
        );
        // título longo é cortado em 128
        let long = "x".repeat(300);
        let OutlineOp::Add { title, .. } = new_bookmark(&t, None, &long, 0, None) else {
            unreachable!()
        };
        assert_eq!(title.chars().count(), 128);
    }

    #[test]
    fn panel_keys() {
        let t = tree();
        assert_eq!(
            key_action(&t, Some(1), Key::ArrowDown),
            Some(KeyAction::Select(2))
        );
        assert_eq!(
            key_action(&t, Some(3), Key::ArrowDown),
            Some(KeyAction::Select(4))
        );
        assert_eq!(
            key_action(&t, Some(4), Key::ArrowDown),
            None,
            "o 5 está escondido"
        );
        assert_eq!(
            key_action(&t, Some(2), Key::ArrowUp),
            Some(KeyAction::Select(1))
        );
        assert_eq!(
            key_action(&t, Some(4), Key::ArrowRight),
            Some(KeyAction::Op(OutlineOp::SetOpen { id: 4, open: true }))
        );
        assert_eq!(
            key_action(&t, Some(1), Key::ArrowLeft),
            Some(KeyAction::Op(OutlineOp::SetOpen { id: 1, open: false }))
        );
        assert_eq!(
            key_action(&t, Some(2), Key::ArrowLeft),
            Some(KeyAction::Select(1)),
            "folha: vai ao pai"
        );
        assert_eq!(
            key_action(&t, Some(2), Key::Delete),
            Some(KeyAction::Op(OutlineOp::Delete { id: 2 }))
        );
        assert_eq!(key_action(&t, Some(2), Key::F2), Some(KeyAction::Rename(2)));
        assert_eq!(key_action(&t, Some(2), Key::Enter), Some(KeyAction::Go(2)));
        assert_eq!(
            key_action(&t, None, Key::ArrowDown),
            Some(KeyAction::Select(1))
        );
    }
}
