//! Painel Camadas (como no Acrobat): a árvore do documento com o olho de cada camada, o cadeado
//! das travadas, Expandir/Recolher tudo, Redefinir para visibilidade inicial e Propriedades da
//! camada.

use crate::links::safe_display;
use pdf_core::LayerNode;
use std::collections::HashSet;

/// Uma linha do painel.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub depth: usize,
    pub text: String,
    /// Camada (com olho); `None` num rótulo de grupo.
    pub id: Option<i32>,
    pub visible: bool,
    pub locked: bool,
    pub has_kids: bool,
    /// Chave do item para expandir e recolher.
    pub key: String,
}

/// Chave estável de um item (a camada pelo número; o grupo pela posição e pelo texto).
pub fn row_key(node: &LayerNode, path: &[usize]) -> String {
    match node {
        LayerNode::Layer { id, .. } => format!("L{id}"),
        LayerNode::Label { text, .. } => format!("G{path:?}{text}"),
    }
}

/// Linhas visíveis (os itens dentro de um recolhido ficam de fora).
pub fn rows(tree: &[LayerNode], closed: &HashSet<String>) -> Vec<Row> {
    fn walk(
        nodes: &[LayerNode],
        depth: usize,
        path: &mut Vec<usize>,
        closed: &HashSet<String>,
        out: &mut Vec<Row>,
    ) {
        for (k, node) in nodes.iter().enumerate() {
            path.push(k);
            let key = row_key(node, path);
            let (row, kids) = match node {
                LayerNode::Layer {
                    id,
                    name,
                    visible,
                    locked,
                    kids,
                    ..
                } => (
                    Row {
                        depth,
                        text: if *locked {
                            format!("{} (travada)", safe_display(name))
                        } else {
                            safe_display(name)
                        },
                        id: Some(*id),
                        visible: *visible,
                        locked: *locked,
                        has_kids: !kids.is_empty(),
                        key: key.clone(),
                    },
                    kids,
                ),
                LayerNode::Label { text, kids } => (
                    Row {
                        depth,
                        text: safe_display(text),
                        id: None,
                        visible: true,
                        locked: false,
                        has_kids: !kids.is_empty(),
                        key: key.clone(),
                    },
                    kids,
                ),
            };
            out.push(row);
            if !closed.contains(&key) {
                walk(kids, depth + 1, path, closed, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(tree, 0, &mut Vec::new(), closed, &mut out);
    out
}

/// O que o painel pede à aba.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerAction {
    /// O olho (só a vista).
    View { id: i32, on: bool },
    /// Redefinir para visibilidade inicial.
    Reset,
    /// Propriedades da camada.
    Edit {
        id: i32,
        name: String,
        default_on: bool,
    },
}

impl From<LayerAction> for pdf_core::Request {
    fn from(action: LayerAction) -> Self {
        match action {
            LayerAction::View { id, on } => Self::LayerView { id, on },
            LayerAction::Reset => Self::ResetLayers,
            LayerAction::Edit {
                id,
                name,
                default_on,
            } => Self::EditLayer {
                id,
                name,
                default_on,
            },
        }
    }
}

#[derive(Default)]
pub struct LayersPanel {
    closed: HashSet<String>,
    /// Linhas calculadas, refeitas só quando a árvore ou os recolhidos mudam.
    rows: Option<Vec<Row>>,
    /// Janela Propriedades da camada: (camada, nome, estado padrão).
    props: Option<(i32, String, bool)>,
}

/// Nome e estado padrão da camada `id` na árvore.
fn find(tree: &[LayerNode], wanted: i32) -> Option<(String, bool)> {
    tree.iter().find_map(|n| match n {
        LayerNode::Layer {
            id,
            name,
            default_on,
            kids,
            ..
        } => {
            if *id == wanted {
                Some((name.clone(), *default_on))
            } else {
                find(kids, wanted)
            }
        }
        LayerNode::Label { kids, .. } => find(kids, wanted),
    })
}

impl LayersPanel {
    /// A árvore mudou: as linhas são refeitas no próximo desenho.
    pub fn invalidate(&mut self) {
        self.rows = None;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        tree: &[LayerNode],
        can_edit: bool,
    ) -> Vec<LayerAction> {
        let mut out = Vec::new();
        if tree.is_empty() {
            ui.weak("Este documento não tem camadas.");
            return out;
        }
        ui.horizontal(|ui| {
            if ui
                .small_button("⊞")
                .on_hover_text("Expandir tudo")
                .clicked()
            {
                self.closed.clear();
                self.rows = None;
            }
            if ui
                .small_button("⊟")
                .on_hover_text("Recolher tudo")
                .clicked()
            {
                self.rows = None;
                self.closed = rows(tree, &HashSet::new())
                    .into_iter()
                    .filter(|r| r.has_kids)
                    .map(|r| r.key)
                    .collect();
            }
            if ui
                .small_button("Redefinir")
                .on_hover_text("Redefinir para visibilidade inicial")
                .clicked()
            {
                out.push(LayerAction::Reset);
            }
        });
        ui.separator();
        // as linhas saem do cache durante o desenho e voltam se nada foi aberto ou fechado
        let list = self.rows.take().unwrap_or_else(|| rows(tree, &self.closed));
        let mut toggled = false;
        let row_h = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            row_h,
            list.len(),
            |ui, range| {
                for row in &list[range] {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0 * row.depth as f32);
                        if row.has_kids {
                            let open = !self.closed.contains(&row.key);
                            let arrow = if open { "⏷" } else { "⏵" };
                            if ui
                                .add(egui::Button::new(arrow).frame(false).small())
                                .clicked()
                            {
                                if open {
                                    self.closed.insert(row.key.clone());
                                } else {
                                    self.closed.remove(&row.key);
                                }
                                toggled = true;
                            }
                        } else {
                            ui.add_space(16.0);
                        }
                        let Some(id) = row.id else {
                            ui.strong(&row.text);
                            return;
                        };
                        let mut on = row.visible;
                        let eye = ui
                            .add_enabled(!row.locked, egui::Checkbox::new(&mut on, &row.text))
                            .on_hover_text(if row.locked {
                                "Camada travada"
                            } else {
                                "Mostrar ou ocultar a camada"
                            });
                        if on != row.visible {
                            out.push(LayerAction::View { id, on });
                        }
                        eye.context_menu(|ui| {
                            if ui
                                .add_enabled(can_edit, egui::Button::new("Propriedades da camada…"))
                                .clicked()
                            {
                                if let Some((name, default_on)) = find(tree, id) {
                                    self.props = Some((id, name, default_on));
                                }
                                ui.close();
                            }
                        });
                    });
                }
            },
        );
        if !toggled && self.rows.is_none() {
            self.rows = Some(list);
        }
        self.props_window(ui.ctx(), &mut out);
        out
    }

    fn props_window(&mut self, ctx: &egui::Context, out: &mut Vec<LayerAction>) {
        let Some((id, name, default_on)) = self.props.as_mut() else {
            return;
        };
        let mut done = None;
        egui::Window::new("Propriedades da camada")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Nome:");
                    ui.add(egui::TextEdit::singleline(name).char_limit(128));
                });
                ui.checkbox(default_on, "Estado padrão: visível (ao abrir o documento)");
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
                out.push(LayerAction::Edit {
                    id: *id,
                    name: name.trim().to_owned(),
                    default_on: *default_on,
                });
                self.props = None;
            }
            Some(false) => self.props = None,
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: i32, name: &str, locked: bool, kids: Vec<LayerNode>) -> LayerNode {
        LayerNode::Layer {
            id,
            name: name.into(),
            visible: true,
            default_on: true,
            locked,
            kids,
        }
    }

    #[test]
    fn rows_follow_the_tree_and_hide_closed_groups() {
        let tree = vec![
            layer(1, "A", false, vec![layer(2, "B", false, vec![])]),
            LayerNode::Label {
                text: "Grupo".into(),
                kids: vec![layer(3, "C", true, vec![])],
            },
        ];
        let all: Vec<_> = rows(&tree, &Default::default())
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.text))
            .collect();
        assert_eq!(all, ["A", "  B", "Grupo", "  C (travada)"]);
        let closed = [row_key(&tree[0], &[])].into_iter().collect();
        let some: Vec<_> = rows(&tree, &closed)
            .iter()
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(some, ["A", "Grupo", "C (travada)"]);
    }

    #[test]
    fn names_from_the_file_are_shown_safely() {
        let tree = vec![layer(1, "Cota\u{202E}s", false, vec![])];
        assert!(!rows(&tree, &Default::default())[0]
            .text
            .contains('\u{202E}'));
    }
}
