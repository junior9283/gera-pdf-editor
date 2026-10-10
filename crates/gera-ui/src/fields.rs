//! Preencher formulários (como no Acrobat): realce dos campos, clique para editar, Tab entre os
//! campos e a barra roxa "Este documento contém campos de formulário".
use crate::edit::screen_rect;
use app_state::transform::PageMap;
use pdf_core::{FieldInfo, FieldKind};

/// Texto em edição num campo.
struct TextDraft {
    page: i32,
    id: i32,
    text: String,
    original: String,
    focused: bool,
}

/// O que o usuário pediu nos campos.
pub enum FieldAction {
    Set {
        page: i32,
        id: i32,
        value: String,
    },
    /// Campo de assinatura clicado.
    Sign,
}

pub struct FormState {
    /// "Realçar campos" ligado (padrão, como no Acrobat).
    pub highlight: bool,
    draft: Option<TextDraft>,
    /// Lista aberta: (página, id).
    choices: Option<(i32, i32)>,
    /// Próximo campo a abrir (Tab): (página, id).
    pub pending_focus: Option<(i32, i32)>,
    /// Campo com o foco do teclado sem estar em edição (caixa, opção, lista): Espaço ou Enter
    /// age, Tab passa adiante.
    focused: Option<(i32, i32)>,
    /// Barra pedindo confirmação para limpar o formulário.
    pub confirm_reset: bool,
}

impl Default for FormState {
    fn default() -> Self {
        Self {
            highlight: true,
            draft: None,
            choices: None,
            pending_focus: None,
            focused: None,
            confirm_reset: false,
        }
    }
}

/// Ordem de Tab: página, de cima para baixo, da esquerda para a direita.
pub fn tab_order(fields: &[FieldInfo]) -> Vec<(i32, i32)> {
    let mut v: Vec<&FieldInfo> = fields.iter().filter(|f| !f.readonly).collect();
    v.sort_by(|a, b| {
        (a.page, a.rect.y0 as i64, a.rect.x0 as i64).cmp(&(
            b.page,
            b.rect.y0 as i64,
            b.rect.x0 as i64,
        ))
    });
    let mut out: Vec<(i32, i32)> = Vec::new();
    for f in v {
        // um grupo de botões de opção é uma parada só
        let same_group = out
            .last()
            .and_then(|&(p, id)| fields.iter().find(|x| x.page == p && x.id == id))
            .is_some_and(|prev| prev.name == f.name && matches!(f.kind, FieldKind::Radio { .. }));
        if !same_group {
            out.push((f.page, f.id));
        }
    }
    out
}

/// Campo seguinte (ou anterior) na ordem de Tab, dando a volta.
pub fn next_field(fields: &[FieldInfo], from: (i32, i32), back: bool) -> Option<(i32, i32)> {
    let order = tab_order(fields);
    let n = order.len();
    if n == 0 {
        return None;
    }
    let at = order.iter().position(|&k| k == from).unwrap_or(0);
    Some(order[if back { (at + n - 1) % n } else { (at + 1) % n }])
}

/// Novo valor ao clicar numa caixa ou num botão de opção (`None`: não é desse tipo).
pub fn toggled(f: &FieldInfo) -> Option<String> {
    match &f.kind {
        FieldKind::Checkbox { on } => Some(if f.value == *on {
            "Off".into()
        } else {
            on.clone()
        }),
        FieldKind::Radio { on } => Some(on.clone()),
        _ => None,
    }
}

/// Texto respeitando o limite de caracteres do campo.
pub fn limited(text: &str, max: Option<usize>) -> String {
    match max {
        Some(n) => text.chars().take(n).collect(),
        None => text.to_owned(),
    }
}

/// Campos da página: realce, cliques e edição. Chamar depois dos gestos da página (os campos
/// ficam por cima e recebem os cliques).
pub fn on_page(
    ui: &mut egui::Ui,
    st: &mut FormState,
    fields: &[FieldInfo],
    page: i32,
    map: &PageMap,
) -> Vec<FieldAction> {
    let mut out = Vec::new();
    let fill = egui::Color32::from_rgba_unmultiplied(120, 140, 255, 45);
    for f in fields.iter().filter(|f| f.page == page) {
        let r = screen_rect(&f.rect, map);
        if st.highlight {
            ui.painter().rect_filled(r, 0.0, fill);
            if f.required {
                ui.painter().rect_stroke(
                    r,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 40, 40)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let editing = st
            .draft
            .as_ref()
            .is_some_and(|d| d.page == page && d.id == f.id);
        if editing || f.readonly {
            continue;
        }
        let resp = ui.interact(r, ui.id().with(("campo", page, f.id)), egui::Sense::click());
        let resp = if f.tooltip.is_empty() {
            resp
        } else {
            resp.on_hover_text(&f.tooltip)
        };
        let resp = resp.on_hover_cursor(match f.kind {
            FieldKind::Text { .. } => egui::CursorIcon::Text,
            _ => egui::CursorIcon::PointingHand,
        });
        let key = (page, f.id);
        if st.pending_focus == Some(key) {
            // pelo Tab: texto abre para digitar; os outros só recebem o foco (como no Acrobat)
            st.pending_focus = None;
            if matches!(f.kind, FieldKind::Text { .. }) {
                open(st, f, &mut out);
            } else {
                st.focused = Some(key);
                // o Tab também andou o foco do egui (até o menu): o teclado fica com o campo
                ui.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
        }
        if resp.clicked() {
            st.focused = Some(key);
            open(st, f, &mut out);
        }
        if st.focused == Some(key) {
            ui.painter().rect_stroke(
                r.expand(2.0),
                2.0,
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Outside,
            );
            let (activate, tab, shift) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Tab),
                    i.modifiers.shift,
                )
            });
            if activate && !resp.clicked() {
                open(st, f, &mut out);
            }
            if tab {
                st.focused = None;
                st.pending_focus = next_field(fields, key, shift);
            }
        }
    }
    text_overlay(ui, st, fields, page, map, &mut out);
    choices_popup(ui, st, fields, page, map, &mut out);
    out
}

/// Começa a editar o campo (ou age, se for caixa/opção/assinatura).
fn open(st: &mut FormState, f: &FieldInfo, out: &mut Vec<FieldAction>) {
    if let Some(value) = toggled(f) {
        out.push(FieldAction::Set {
            page: f.page,
            id: f.id,
            value,
        });
        return;
    }
    match f.kind {
        FieldKind::Text { .. } => {
            st.draft = Some(TextDraft {
                page: f.page,
                id: f.id,
                text: f.value.clone(),
                original: f.value.clone(),
                focused: false,
            })
        }
        FieldKind::Combo { .. } | FieldKind::List { .. } => st.choices = Some((f.page, f.id)),
        FieldKind::Signature => out.push(FieldAction::Sign),
        _ => {}
    }
}

fn text_overlay(
    ui: &mut egui::Ui,
    st: &mut FormState,
    fields: &[FieldInfo],
    page: i32,
    map: &PageMap,
    out: &mut Vec<FieldAction>,
) {
    let Some(d) = st.draft.as_mut().filter(|d| d.page == page) else {
        return;
    };
    let Some(f) = fields.iter().find(|f| f.page == page && f.id == d.id) else {
        st.draft = None;
        return;
    };
    let FieldKind::Text {
        multiline,
        password,
        max_len,
    } = f.kind
    else {
        st.draft = None;
        return;
    };
    let r = screen_rect(&f.rect, map);
    ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
    let size = (r.height() * 0.6).clamp(8.0, 18.0 * map.zoom);
    let mut field = if multiline {
        egui::TextEdit::multiline(&mut d.text)
    } else {
        egui::TextEdit::singleline(&mut d.text)
    }
    .password(password)
    .font(egui::FontId::proportional(size))
    .text_color(egui::Color32::BLACK)
    .frame(egui::Frame::NONE)
    .desired_width(r.width() - 4.0)
    .id(egui::Id::new(("campo-texto", page, f.id)));
    if let Some(n) = max_len {
        field = field.char_limit(n);
    }
    let resp = ui.put(r, field);
    ui.painter().rect_stroke(
        r,
        0.0,
        egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
        egui::StrokeKind::Outside,
    );
    if resp.has_focus() {
        d.focused = true;
        return;
    }
    if !d.focused {
        resp.request_focus();
        return;
    }
    // perdeu o foco: Esc cancela; Tab confirma e vai para o próximo campo; o resto confirma
    let (esc, tab, shift) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Tab),
            i.modifiers.shift,
        )
    });
    let d = st.draft.take().expect("rascunho existe");
    if !esc && d.text != d.original {
        out.push(FieldAction::Set {
            page: d.page,
            id: d.id,
            value: limited(&d.text, max_len),
        });
    }
    if tab {
        st.pending_focus = next_field(fields, (d.page, d.id), shift);
    }
}

fn choices_popup(
    ui: &mut egui::Ui,
    st: &mut FormState,
    fields: &[FieldInfo],
    page: i32,
    map: &PageMap,
    out: &mut Vec<FieldAction>,
) {
    let Some((p, id)) = st.choices.filter(|c| c.0 == page) else {
        return;
    };
    let Some(f) = fields.iter().find(|f| f.page == p && f.id == id) else {
        st.choices = None;
        return;
    };
    let options = match &f.kind {
        FieldKind::Combo { options, .. } | FieldKind::List { options } => options,
        _ => return,
    };
    let r = screen_rect(&f.rect, map);
    let area = egui::Area::new(egui::Id::new(("opcoes", p, id)))
        .fixed_pos(r.left_bottom())
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(r.width());
                for (export, label) in options {
                    let text = if label.is_empty() { export } else { label };
                    if ui.selectable_label(f.value == *export, text).clicked() {
                        out.push(FieldAction::Set {
                            page: p,
                            id,
                            value: export.clone(),
                        });
                        st.choices = None;
                    }
                }
            });
        });
    // clicar fora fecha a lista
    let clicked_elsewhere = ui.input(|i| i.pointer.any_click())
        && !area.response.hovered()
        && !ui.rect_contains_pointer(r);
    if clicked_elsewhere {
        st.choices = None;
    }
}

/// Barra roxa (como no Acrobat) quando o documento tem campos.
pub fn bar(ui: &mut egui::Ui, st: &mut FormState) -> bool {
    let mut reset = false;
    let purple = egui::Color32::from_rgb(232, 226, 250);
    egui::Frame::new()
        .fill(purple)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Este documento contém campos de formulário.")
                        .color(egui::Color32::from_rgb(60, 40, 110)),
                );
                ui.checkbox(&mut st.highlight, "Realçar campos");
                if st.confirm_reset {
                    ui.label("Limpar todos os campos?");
                    if ui.button("Limpar").clicked() {
                        reset = true;
                        st.confirm_reset = false;
                    }
                    if ui.button("Cancelar").clicked() {
                        st.confirm_reset = false;
                    }
                } else if ui.button("Limpar formulário").clicked() {
                    st.confirm_reset = true;
                }
            });
        });
    reset
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    fn f(id: i32, page: i32, y: f32, x: f32, name: &str, kind: FieldKind) -> FieldInfo {
        FieldInfo {
            id,
            page,
            name: name.into(),
            kind,
            rect: PdfRect::new(x, y, x + 50.0, y + 10.0),
            value: String::new(),
            readonly: false,
            required: false,
            tooltip: String::new(),
        }
    }

    fn text() -> FieldKind {
        FieldKind::Text {
            multiline: false,
            password: false,
            max_len: None,
        }
    }

    #[test]
    fn tab_goes_top_to_bottom_left_to_right_and_wraps() {
        let radio = || FieldKind::Radio { on: "a".into() };
        let fields = vec![
            f(3, 0, 100.0, 10.0, "c", text()),
            f(1, 0, 10.0, 200.0, "b", text()),
            f(2, 0, 10.0, 10.0, "a", text()),
            f(4, 0, 200.0, 10.0, "r", radio()),
            f(5, 0, 200.0, 100.0, "r", radio()),
            f(6, 1, 0.0, 0.0, "d", text()),
        ];
        assert_eq!(tab_order(&fields), [(0, 2), (0, 1), (0, 3), (0, 4), (1, 6)]);
        assert_eq!(
            next_field(&fields, (1, 6), false),
            Some((0, 2)),
            "dá a volta"
        );
        assert_eq!(next_field(&fields, (0, 2), true), Some((1, 6)));
    }

    #[test]
    fn clicks_toggle_checkboxes_and_pick_radios() {
        let mut c = f(
            1,
            0,
            0.0,
            0.0,
            "c",
            FieldKind::Checkbox { on: "Sim".into() },
        );
        assert_eq!(toggled(&c).as_deref(), Some("Sim"));
        c.value = "Sim".into();
        assert_eq!(toggled(&c).as_deref(), Some("Off"));
        assert_eq!(toggled(&f(2, 0, 0.0, 0.0, "t", text())), None);
    }

    #[test]
    fn text_respects_the_character_limit() {
        assert_eq!(limited("Conceição", Some(4)), "Conc");
        assert_eq!(limited("abc", None), "abc");
    }
}
