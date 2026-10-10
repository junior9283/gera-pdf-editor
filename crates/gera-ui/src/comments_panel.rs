use app_state::comments::{sorted, threads, CommentFilter, SortBy, Thread};
use app_state::pdf_date::format_pdf_date;
use app_state::review::{ReviewState, MARKED, UNMARKED};
use pdf_core::{AnnotInfo, AnnotKind, AnnotOp, StateModel};
use std::collections::HashMap;

/// Estado do painel em cada aba: busca/filtros, ordenação e a resposta sendo digitada.
#[derive(Default)]
pub struct PanelState {
    pub filter: CommentFilter,
    pub sort: SortBy,
    reply: String,
    /// Seleção do quadro anterior: quando muda, o cartão rola até ficar visível.
    last_selected: Option<(i32, i32)>,
}

/// O que o usuário pediu no painel.
pub enum PanelAction {
    /// Seleciona o comentário e leva até ele na página.
    Select {
        page: i32,
        id: i32,
        at: (f32, f32),
    },
    Op(AnnotOp),
    Properties {
        page: i32,
        id: i32,
    },
}

pub fn page_header(page: i32, count: usize) -> String {
    format!("Página {} ({count})", page + 1)
}

pub fn can_reply(text: &str) -> bool {
    !text.trim().is_empty()
}

/// Seleciona o comentário e leva até ele na página.
fn select(t: &Thread) -> PanelAction {
    PanelAction::Select {
        page: t.root.page,
        id: t.root.id,
        at: (t.root.rect.x0, t.root.rect.y0),
    }
}

fn set_state(t: &Thread, model: StateModel, state: &str) -> PanelAction {
    PanelAction::Op(AnnotOp::SetState {
        page: t.root.page,
        parent: t.root.id,
        model,
        state: state.into(),
    })
}

/// Ações do comentário (botão ⋯ e clique direito).
fn card_menu(ui: &mut egui::Ui, t: &Thread, out: &mut Vec<PanelAction>) {
    let (page, id) = (t.root.page, t.root.id);
    if ui.button("Responder").clicked() {
        out.push(select(t));
    }
    ui.menu_button("Definir status", |ui| {
        for s in ReviewState::ALL {
            let current = t.status() == s;
            if ui.radio(current, s.label()).clicked() {
                out.push(set_state(t, StateModel::Review, s.pdf_name()));
            }
        }
    });
    let mut on = t.checked;
    if ui.checkbox(&mut on, "Marcar com visto").changed() {
        let state = if on { MARKED } else { UNMARKED };
        out.push(set_state(t, StateModel::Marked, state));
    }
    ui.separator();
    if ui.button("Excluir").clicked() {
        out.push(PanelAction::Op(AnnotOp::Delete { page, id }));
    }
    if ui.button("Propriedades…").clicked() {
        out.push(PanelAction::Properties { page, id });
    }
}

/// Autor e data de uma anotação, numa linha.
fn byline(ui: &mut egui::Ui, a: &AnnotInfo) {
    ui.horizontal_wrapped(|ui| {
        let author = if a.author.is_empty() {
            "Sem autor"
        } else {
            &a.author
        };
        ui.label(egui::RichText::new(author).strong());
        let date = if a.modified.is_empty() {
            &a.created
        } else {
            &a.modified
        };
        if !date.is_empty() {
            ui.label(egui::RichText::new(format_pdf_date(date)).small().weak());
        }
    });
}

/// Texto curto do cartão fechado (o selecionado mostra tudo).
fn preview(text: &str) -> String {
    const MAX: usize = 140;
    match text.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_owned(),
    }
}

fn card(
    ui: &mut egui::Ui,
    st: &mut PanelState,
    t: &Thread,
    selected: bool,
    scroll: bool,
    out: &mut Vec<PanelAction>,
) {
    let stroke = if selected {
        egui::Stroke::new(1.5, ui.visuals().selection.stroke.color)
    } else {
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    let frame = egui::Frame::group(ui.style())
        .stroke(stroke)
        .inner_margin(8.0);
    let resp = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(crate::sidebar::label_of(&t.root)).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("···", |ui| card_menu(ui, t, out));
                    if t.checked {
                        ui.label(egui::RichText::new("Visto").small().strong());
                    }
                });
            });
            byline(ui, &t.root);
            if !t.root.contents.is_empty() {
                let text = if selected {
                    t.root.contents.clone()
                } else {
                    preview(&t.root.contents)
                };
                ui.label(text);
            }
            if let Some((state, who)) = &t.review {
                if *state != ReviewState::None {
                    ui.label(
                        egui::RichText::new(format!("{} — {who}", state.label()))
                            .small()
                            .italics(),
                    );
                }
            }
            if !selected && !t.replies.is_empty() {
                let n = t.replies.len();
                let label = if n == 1 {
                    "1 resposta".to_owned()
                } else {
                    format!("{n} respostas")
                };
                ui.label(egui::RichText::new(label).small().weak());
            }
            if selected {
                for r in &t.replies {
                    ui.separator();
                    byline(ui, r);
                    ui.label(&r.contents);
                }
                ui.separator();
                let field = ui.add(
                    egui::TextEdit::multiline(&mut st.reply)
                        .hint_text("Adicionar resposta…")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                let send_keys = field.has_focus()
                    && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let ok = can_reply(&st.reply);
                    if ui.add_enabled(ok, egui::Button::new("Responder")).clicked()
                        || (send_keys && ok)
                    {
                        out.push(PanelAction::Op(AnnotOp::Reply {
                            page: t.root.page,
                            parent: t.root.id,
                            text: std::mem::take(&mut st.reply).trim().to_owned(),
                        }));
                    }
                });
            }
        })
        .response;
    if scroll {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
    // só o cartão fechado inteiro é clicável: no aberto, o campo de resposta precisa dos cliques
    // (o menu dele fica no botão ···)
    if !selected {
        let resp = resp.interact(egui::Sense::click());
        if resp.clicked() {
            st.reply.clear();
            out.push(select(t));
        }
        resp.context_menu(|ui| card_menu(ui, t, out));
    }
}

/// Barra do painel: busca, ordenação e filtros.
fn toolbar(ui: &mut egui::Ui, st: &mut PanelState, all: &[Thread]) {
    ui.add(
        egui::TextEdit::singleline(&mut st.filter.text)
            .hint_text("Buscar comentários")
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        ui.menu_button(format!("Ordenar: {}", st.sort.label()), |ui| {
            for by in SortBy::ALL {
                ui.radio_value(&mut st.sort, by, by.label());
            }
        });
        let label = if st.filter.is_active() {
            "Filtrar •"
        } else {
            "Filtrar"
        };
        ui.menu_button(label, |ui| filter_menu(ui, &mut st.filter, all));
    });
}

/// Liga/desliga `v` na lista (filtro de vários valores).
fn toggle<T: PartialEq + Clone>(ui: &mut egui::Ui, list: &mut Vec<T>, v: &T, label: &str) {
    let mut on = list.contains(v);
    if ui.checkbox(&mut on, label).changed() {
        if on {
            list.push(v.clone());
        } else {
            list.retain(|x| x != v);
        }
    }
}

fn filter_menu(ui: &mut egui::Ui, f: &mut CommentFilter, all: &[Thread]) {
    let mut kinds: Vec<AnnotKind> = Vec::new();
    let mut authors: Vec<String> = Vec::new();
    for t in all {
        if !kinds.contains(&t.root.kind) {
            kinds.push(t.root.kind);
        }
        if !t.root.author.is_empty() && !authors.contains(&t.root.author) {
            authors.push(t.root.author.clone());
        }
    }
    ui.menu_button("Tipo", |ui| {
        for k in &kinds {
            toggle(ui, &mut f.kinds, k, crate::sidebar::kind_label(*k));
        }
    });
    ui.menu_button("Autor", |ui| {
        for a in &authors {
            toggle(ui, &mut f.authors, a, a);
        }
    });
    ui.menu_button("Status", |ui| {
        for s in ReviewState::ALL {
            toggle(ui, &mut f.statuses, &s, s.label());
        }
    });
    ui.menu_button("Visto", |ui| {
        ui.radio_value(&mut f.checked, None, "Todos");
        ui.radio_value(&mut f.checked, Some(true), "Marcados");
        ui.radio_value(&mut f.checked, Some(false), "Não marcados");
    });
    ui.separator();
    if ui
        .add_enabled(f.is_active(), egui::Button::new("Limpar filtros"))
        .clicked()
    {
        *f = CommentFilter::default();
    }
}

/// Desenha o painel e devolve o que o usuário pediu.
pub fn show(
    ui: &mut egui::Ui,
    st: &mut PanelState,
    annots: &[AnnotInfo],
    selected: Option<(i32, i32)>,
) -> Vec<PanelAction> {
    let mut out = Vec::new();
    ui.heading("Comentários");
    let all = threads(annots);
    toolbar(ui, st, &all);
    ui.separator();
    let total = all.len();
    let filtered: Vec<Thread> = all.into_iter().filter(|t| st.filter.matches(t)).collect();
    let shown = sorted(filtered, st.sort);
    let mut per_page: HashMap<i32, usize> = HashMap::new();
    for t in &shown {
        *per_page.entry(t.root.page).or_default() += 1;
    }
    // um cartão selecionado na página que ainda não estava: rola até ele
    let scroll_to = (selected != st.last_selected).then_some(selected).flatten();
    st.last_selected = selected;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if total == 0 {
                ui.label("Nenhum comentário. Use as ferramentas da linha Comentar para criar.");
                return;
            }
            if shown.is_empty() {
                ui.label("Nenhum comentário corresponde aos filtros.");
                return;
            }
            let mut last_page = None;
            for t in &shown {
                if st.sort == SortBy::Page && last_page != Some(t.root.page) {
                    last_page = Some(t.root.page);
                    let n = per_page[&t.root.page];
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(page_header(t.root.page, n)).strong());
                }
                let key = Some((t.root.page, t.root.id));
                let scroll = scroll_to.is_some() && scroll_to == key;
                card(ui, st, t, selected == key, scroll, &mut out);
                ui.add_space(4.0);
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_header_counts_comments() {
        assert_eq!(page_header(1, 3), "Página 2 (3)");
    }

    #[test]
    fn reply_button_needs_text() {
        assert!(!can_reply("   "));
        assert!(can_reply(" ok "));
    }
}
