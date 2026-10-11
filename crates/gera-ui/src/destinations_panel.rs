//! Painel Destinos (como no Acrobat): Nome e Página, ordenar por um ou outro, novo destino da
//! vista atual, ir até, definir destino, renomear no lugar e excluir.

use crate::links::safe_display;
use pdf_core::DestInfo;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Page,
}

/// Comparação natural: "cap2" antes de "cap10", sem diferenciar maiúsculas.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(ch) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(ch);
                        it.next();
                    }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                let ord = c.to_lowercase().cmp(d.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Índices da lista na ordem escolhida (por página: as sem página no fim).
pub fn sorted(list: &[DestInfo], sort: Sort) -> Vec<usize> {
    let mut out: Vec<usize> = (0..list.len()).collect();
    let by_name = |a: &DestInfo, b: &DestInfo| natural(&a.name, &b.name);
    match sort {
        Sort::Name => out.sort_by(|&a, &b| by_name(&list[a], &list[b])),
        Sort::Page => out.sort_by(|&a, &b| {
            let key = |d: &DestInfo| d.page.unwrap_or(i32::MAX);
            key(&list[a])
                .cmp(&key(&list[b]))
                .then_with(|| by_name(&list[a], &list[b]))
        }),
    }
    out
}

/// O que o painel pede à aba.
#[derive(Debug, Clone, PartialEq)]
pub enum DestAction {
    Go {
        page: i32,
        at: Option<(f32, f32)>,
    },
    /// Novo destino na vista atual.
    New,
    /// Definir destino: a vista atual.
    SetHere(String),
    Rename {
        name: String,
        to: String,
    },
    Remove(String),
}

#[derive(Default)]
pub struct DestinationsPanel {
    pub sort: Sort,
    selected: Option<String>,
    /// Ordem calculada (ordem, índices), refeita só quando a lista ou a ordem mudam.
    order: Option<(Sort, Vec<usize>)>,
    /// Renomeando: (nome, texto).
    renaming: Option<(String, String)>,
}

impl DestinationsPanel {
    /// A lista de destinos mudou: a ordem é refeita no próximo desenho.
    pub fn invalidate(&mut self) {
        self.order = None;
    }

    /// Renomear o destino recém-criado (como no Acrobat, o nome vem selecionado para editar).
    pub fn rename(&mut self, name: String) {
        self.selected = Some(name.clone());
        self.renaming = Some((name.clone(), name));
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        list: &[DestInfo],
        can_edit: bool,
    ) -> Vec<DestAction> {
        let mut out = Vec::new();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_edit, egui::Button::new("Novo destino"))
                .on_hover_text("Cria um destino na vista atual")
                .clicked()
            {
                out.push(DestAction::New);
            }
        });
        ui.horizontal(|ui| {
            for (sort, text) in [(Sort::Name, "Nome"), (Sort::Page, "Página")] {
                if ui
                    .selectable_label(self.sort == sort, text)
                    .on_hover_text("Ordenar")
                    .clicked()
                {
                    self.sort = sort;
                }
            }
        });
        ui.separator();
        if list.is_empty() {
            ui.weak("Este documento não tem destinos.");
            return out;
        }
        if self
            .order
            .as_ref()
            .is_none_or(|(s, o)| *s != self.sort || o.len() != list.len())
        {
            self.order = Some((self.sort, sorted(list, self.sort)));
        }
        let shown: Vec<&DestInfo> = self
            .order
            .as_ref()
            .map(|(_, o)| o.iter().filter_map(|&k| list.get(k)).collect())
            .unwrap_or_default();
        let row_h = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            row_h,
            shown.len(),
            |ui, rows| {
                for d in &shown[rows] {
                    ui.horizontal(|ui| {
                        if let Some((name, text)) =
                            self.renaming.as_mut().filter(|(n, _)| *n == d.name)
                        {
                            let edit = ui.text_edit_singleline(text);
                            edit.request_focus();
                            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if esc {
                                self.renaming = None;
                            } else if edit.lost_focus() || enter {
                                if let Some(to) = crate::bookmarks::renamed(name, text) {
                                    self.selected = Some(to.clone());
                                    out.push(DestAction::Rename {
                                        name: name.clone(),
                                        to,
                                    });
                                }
                                self.renaming = None;
                            }
                            return;
                        }
                        let selected = self.selected.as_deref() == Some(d.name.as_str());
                        let page = d.page.map_or("—".into(), |p| (p + 1).to_string());
                        let row = ui.add(egui::Button::selectable(
                            selected,
                            format!("{}  ·  {page}", safe_display(&d.name)),
                        ));
                        if row.clicked() {
                            self.selected = Some(d.name.clone());
                        }
                        let go = |out: &mut Vec<DestAction>| {
                            if let Some(page) = d.page {
                                out.push(DestAction::Go { page, at: d.at });
                            }
                        };
                        if row.double_clicked() {
                            go(&mut out);
                        }
                        row.context_menu(|ui| {
                            self.selected = Some(d.name.clone());
                            if ui.button("Ir para o destino").clicked() {
                                go(&mut out);
                                ui.close();
                            }
                            if !can_edit {
                                return;
                            }
                            if ui.button("Definir destino").clicked() {
                                out.push(DestAction::SetHere(d.name.clone()));
                                ui.close();
                            }
                            if ui.button("Renomear").clicked() {
                                self.rename(d.name.clone());
                                ui.close();
                            }
                            if ui.button("Excluir").clicked() {
                                out.push(DestAction::Remove(d.name.clone()));
                                ui.close();
                            }
                        });
                    });
                }
            },
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(name: &str, page: Option<i32>) -> DestInfo {
        DestInfo {
            name: name.into(),
            page,
            at: None,
        }
    }

    #[test]
    fn names_sort_naturally() {
        let list = [d("cap10", Some(0)), d("cap2", Some(5)), d("Anexo", Some(1))];
        let names: Vec<_> = sorted(&list, Sort::Name)
            .iter()
            .map(|&k| list[k].name.clone())
            .collect();
        assert_eq!(names, ["Anexo", "cap2", "cap10"]);
    }

    #[test]
    fn page_sort_puts_unknown_pages_last() {
        let list = [d("b", None), d("a", Some(3)), d("c", Some(1))];
        let names: Vec<_> = sorted(&list, Sort::Page)
            .iter()
            .map(|&k| list[k].name.clone())
            .collect();
        assert_eq!(names, ["c", "a", "b"]);
    }
}
