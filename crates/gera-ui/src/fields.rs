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
    /// Calendário aberto (campo de data): (ano, mês) mostrado.
    calendar: Option<(i32, u32)>,
}

/// O que o usuário pediu nos campos.
pub enum FieldAction {
    Set {
        page: i32,
        id: i32,
        value: String,
    },
    /// Botão com a ação Redefinir formulário.
    Reset,
    /// Campo de assinatura clicado (página e retângulo do campo).
    Sign {
        page: i32,
        id: i32,
        rect: pdf_core::PdfRect,
    },
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
    /// O Tab foi para um campo de outra página: rolar até ele (página, retângulo).
    pub scroll_to: Option<(i32, pdf_core::PdfRect)>,
    /// Texto livre de uma lista suspensa editável.
    combo_text: String,
    /// XFA ou scripts de campo que o app não executa (a barra avisa).
    pub notes: pdf_core::FormNotes,
    /// Valor recusado pelo campo: o aviso (janela, como o alerta do Acrobat).
    pub alert: Option<String>,
}

/// O texto digitado, se a Tecla do campo o aceita; senão o de antes (a tecla não entra).
pub fn keep_typed(keystroke: &pdf_core::FieldFormat, before: &str, now: &str) -> String {
    if pdf_core::keystroke_ok(keystroke, now) {
        now.to_string()
    } else {
        before.to_string()
    }
}

/// O mês `delta` meses depois de (`year`, `month`).
fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let k = year * 12 + month as i32 - 1 + delta;
    (k.div_euclid(12), k.rem_euclid(12) as u32 + 1)
}

/// Dias do mês numa grade que começa no domingo (casas vazias antes do dia 1).
pub fn month_cells(year: i32, month: u32) -> Vec<Option<u32>> {
    use chrono::Datelike;
    let Some(first) = chrono::NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let days = u32::from(first.num_days_in_month());
    let blank = first.weekday().num_days_from_sunday() as usize;
    std::iter::repeat_n(None, blank)
        .chain((1..=days).map(Some))
        .collect()
}

/// A data escolhida no calendário, no formato do campo.
pub fn picked_date(fmt: &pdf_core::FieldFormat, year: i32, month: u32, day: u32) -> String {
    let iso = format!("{year:04}-{month:02}-{day:02}");
    pdf_core::format_value(fmt, &iso).map_or(iso, |f| f.text)
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
            scroll_to: None,
            combo_text: String::new(),
            notes: Default::default(),
            alert: None,
        }
    }
}

impl FormState {
    /// Um campo (caixa, opção, lista) tem o foco do teclado: o Espaço e o Enter são dele.
    pub fn has_focus(&self) -> bool {
        self.focused.is_some()
    }

    #[cfg(test)]
    pub(crate) fn focus_for_test(&mut self, field: Option<(i32, i32)>) {
        self.focused = field;
    }

    /// Há texto em edição num campo (conta como alteração pendente).
    pub fn has_draft(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| d.text != d.original)
    }

    /// Confirma o texto em edição (antes de salvar, imprimir ou abrir outro campo).
    pub fn commit(&mut self) -> Option<FieldAction> {
        let d = self.draft.take()?;
        (d.text != d.original).then_some(FieldAction::Set {
            page: d.page,
            id: d.id,
            value: d.text,
        })
    }

    /// Esc: fecha a lista aberta ou tira o foco do campo. Devolve se havia o que fechar.
    pub fn escape(&mut self) -> bool {
        self.choices.take().is_some() | self.focused.take().is_some()
    }

    /// Passa o foco ao campo seguinte (ou anterior), rolando até ele se estiver em outra página.
    fn tab(&mut self, fields: &[FieldInfo], from: (i32, i32), back: bool) {
        self.pending_focus = next_field(fields, from, back);
        self.scroll_to = self
            .pending_focus
            .filter(|&(page, _)| page != from.0)
            .and_then(|(page, id)| fields.iter().find(|f| f.page == page && f.id == id))
            .map(|f| (f.page, f.rect));
    }
}

/// Ordem de Tab: a de /Annots em cada página (a que o autor do formulário deu, como no Acrobat);
/// um grupo de botões de opção é uma parada só, no lugar do primeiro botão dele.
pub fn tab_order(fields: &[FieldInfo]) -> Vec<(i32, i32)> {
    let mut groups = std::collections::HashSet::new();
    fields
        .iter()
        .filter(|f| !f.readonly)
        .filter(|f| !matches!(f.kind, FieldKind::Radio { .. }) || groups.insert(f.name.as_str()))
        .map(|f| (f.page, f.id))
        .collect()
}

/// Campo seguinte (ou anterior) na ordem de Tab, dando a volta.
pub fn next_field(fields: &[FieldInfo], from: (i32, i32), back: bool) -> Option<(i32, i32)> {
    let order = tab_order(fields);
    let n = order.len();
    if n == 0 {
        return None;
    }
    // um botão de opção que não é o primeiro do grupo conta como a parada do grupo
    let name = fields
        .iter()
        .find(|f| (f.page, f.id) == from)
        .map(|f| f.name.as_str());
    let at = order
        .iter()
        .position(|&k| k == from)
        .or_else(|| {
            order.iter().position(|&(p, id)| {
                fields
                    .iter()
                    .any(|f| f.page == p && f.id == id && Some(f.name.as_str()) == name)
            })
        })
        .unwrap_or(0);
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
                surrender_egui_focus(ui);
            }
        }
        if resp.clicked() {
            st.focused = Some(key);
            open(st, f, &mut out);
        }
        if st.focused == Some(key)
            && ui.input(|i| i.pointer.any_pressed())
            && !resp.contains_pointer()
        {
            // clicou fora: o campo perde o foco (o teclado volta ao resto da janela)
            st.focused = None;
        }
        if st.focused == Some(key) {
            // o teclado é do campo: o Tab do egui não pode levar o foco para o menu
            surrender_egui_focus(ui);
            ui.painter().rect_stroke(
                r.expand(2.0),
                2.0,
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Outside,
            );
            let activate =
                ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
            if activate && !resp.clicked() {
                open(st, f, &mut out);
            }
            // consumido: senão o campo seguinte, no mesmo quadro, veria o mesmo Tab e passaria adiante
            let shift = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
            if shift || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                st.focused = None;
                st.tab(fields, key, shift);
            }
        }
    }
    text_overlay(ui, st, fields, page, map, &mut out);
    choices_popup(ui, st, fields, page, map, &mut out);
    out
}

/// Tira o foco do teclado de qualquer widget do egui (ele fica com o campo do formulário).
fn surrender_egui_focus(ui: &egui::Ui) {
    ui.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
}

/// Começa a editar o campo (ou age, se for caixa/opção/assinatura). O texto que estava sendo
/// digitado em outro campo é confirmado antes.
fn open(st: &mut FormState, f: &FieldInfo, out: &mut Vec<FieldAction>) {
    if st
        .draft
        .as_ref()
        .is_some_and(|d| (d.page, d.id) != (f.page, f.id))
    {
        out.extend(st.commit());
    }
    if let Some(value) = toggled(f) {
        // botão de opção já escolhido: nada muda (nem entra no desfazer)
        if value != f.value {
            out.push(FieldAction::Set {
                page: f.page,
                id: f.id,
                value,
            });
        }
        return;
    }
    match f.kind {
        FieldKind::Text { .. } => {
            // o teclado passa a ser do texto em edição (o Enter de lá não reabre o campo)
            st.focused = None;
            st.draft = Some(TextDraft {
                page: f.page,
                id: f.id,
                text: f.value.clone(),
                original: f.value.clone(),
                focused: false,
                calendar: None,
            })
        }
        FieldKind::Button { reset: true } => out.push(FieldAction::Reset),
        FieldKind::Combo { .. } | FieldKind::List { .. } => {
            st.choices = Some((f.page, f.id));
            st.combo_text = f.value.clone();
        }
        FieldKind::Signature => out.push(FieldAction::Sign {
            page: f.page,
            id: f.id,
            rect: f.rect,
        }),
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
    let before = d.text.clone();
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
    // filtro da Tecla do campo (com Número, letras não entram)
    if resp.changed() {
        d.text = keep_typed(&f.keystroke, &before, &d.text);
    }
    // campo de data: o seletor de calendário ao lado (como no Acrobat)
    if matches!(f.format, pdf_core::FieldFormat::Date(_)) {
        if let Some(value) = calendar(ui, d, &f.format, r) {
            d.text = value;
            let d = st.draft.take().expect("rascunho existe");
            out.push(FieldAction::Set {
                page: d.page,
                id: d.id,
                value: d.text,
            });
            return;
        }
        if d.calendar.is_some() {
            return; // o foco está no calendário: o campo continua em edição
        }
    }
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
        st.tab(fields, (d.page, d.id), shift);
    }
}

/// Botão do calendário ao lado do campo e o calendário aberto. Devolve a data escolhida.
fn calendar(
    ui: &mut egui::Ui,
    d: &mut TextDraft,
    fmt: &pdf_core::FieldFormat,
    r: egui::Rect,
) -> Option<String> {
    use chrono::Datelike;
    let button = egui::Rect::from_min_size(
        r.right_top() + egui::vec2(2.0, 0.0),
        egui::vec2(18.0, r.height().min(22.0)),
    );
    if ui
        .put(button, egui::Button::new("⏷").small())
        .on_hover_text("Escolher a data")
        .clicked()
    {
        d.calendar = match d.calendar {
            Some(_) => None,
            None => {
                let today = chrono::Local::now().date_naive();
                Some((today.year(), today.month()))
            }
        };
    }
    let (year, month) = d.calendar?;
    let mut picked = None;
    let mut shown = (year, month);
    egui::Area::new(egui::Id::new(("calendario", d.page, d.id)))
        .fixed_pos(r.left_bottom())
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("◀").clicked() {
                        shown = shift_month(year, month, -1);
                    }
                    let name = pdf_core::MONTHS[(month - 1) as usize];
                    let mut chars = name.chars();
                    let first = chars.next().map(|c| c.to_uppercase().to_string());
                    ui.label(format!(
                        "{}{} de {year}",
                        first.unwrap_or_default(),
                        chars.as_str()
                    ));
                    if ui.small_button("▶").clicked() {
                        shown = shift_month(year, month, 1);
                    }
                });
                egui::Grid::new(("dias", d.page, d.id)).show(ui, |ui| {
                    for w in ["D", "S", "T", "Q", "Q", "S", "S"] {
                        ui.label(egui::RichText::new(w).small().strong());
                    }
                    ui.end_row();
                    for (k, cell) in month_cells(year, month).into_iter().enumerate() {
                        match cell {
                            Some(day) if ui.small_button(day.to_string()).clicked() => {
                                picked = Some(picked_date(fmt, year, month, day));
                            }
                            Some(_) => {}
                            None => {
                                ui.label("");
                            }
                        }
                        if k % 7 == 6 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
    d.calendar = if picked.is_some() { None } else { Some(shown) };
    picked
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
    let (options, editable) = match &f.kind {
        FieldKind::Combo { options, editable } => (options, *editable),
        FieldKind::List { options } => (options, false),
        _ => return,
    };
    let r = screen_rect(&f.rect, map);
    let area = egui::Area::new(egui::Id::new(("opcoes", p, id)))
        .fixed_pos(r.left_bottom())
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(r.width());
                if editable {
                    // lista suspensa editável: aceita texto livre (Enter confirma)
                    let typed = ui.add(
                        egui::TextEdit::singleline(&mut st.combo_text).desired_width(r.width()),
                    );
                    if typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        out.push(FieldAction::Set {
                            page: p,
                            id,
                            value: st.combo_text.clone(),
                        });
                        st.choices = None;
                    }
                    ui.separator();
                }
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
    // clicar fora fecha a lista (o Esc chega por `FormState::escape`)
    let clicked_elsewhere = ui.input(|i| i.pointer.any_click())
        && !area.response.hovered()
        && !ui.rect_contains_pointer(r);
    if clicked_elsewhere {
        st.choices = None;
    }
}

/// Aviso do que o formulário tem e o app ainda não executa.
fn notes_warning(n: pdf_core::FormNotes) -> Option<&'static str> {
    match (n.xfa, n.scripts) {
        (false, false) => None,
        (true, _) => {
            Some("Formulário XFA: alguns campos ou regras do autor podem não aparecer aqui.")
        }
        (false, true) => {
            Some("O formulário tem cálculos ou validações em JavaScript, que não são executados.")
        }
    }
}

/// Barra roxa (como no Acrobat) quando o documento tem campos.
pub fn bar(ui: &mut egui::Ui, st: &mut FormState) -> bool {
    let mut reset = false;
    // roxo claro no tema claro e escuro no tema escuro (o texto do tema continua legível)
    let dark = ui.visuals().dark_mode;
    let purple = if dark {
        egui::Color32::from_rgb(52, 42, 82)
    } else {
        egui::Color32::from_rgb(232, 226, 250)
    };
    let ink = if dark {
        egui::Color32::from_rgb(225, 215, 250)
    } else {
        egui::Color32::from_rgb(60, 40, 110)
    };
    egui::Frame::new()
        .fill(purple)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Este documento contém campos de formulário.").color(ink),
                );
                ui.checkbox(&mut st.highlight, "Realçar campos");
                if let Some(warning) = notes_warning(st.notes) {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {warning}"));
                }
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

    #[test]
    fn keystroke_filter_blocks_letters_in_numbers() {
        let money = pdf_core::FieldFormat::Number {
            dec: 2,
            sep: 2,
            neg: 0,
            currency: "R$ ".into(),
            prepend: true,
        };
        assert_eq!(keep_typed(&money, "12", "12,5"), "12,5");
        assert_eq!(keep_typed(&money, "12", "12a"), "12", "a letra não entra");
        assert_eq!(keep_typed(&pdf_core::FieldFormat::None, "a", "ab"), "ab");
    }

    #[test]
    fn date_picker_writes_the_format() {
        // março de 2026 começa num domingo
        let cells = month_cells(2026, 3);
        assert_eq!(cells[0], Some(1));
        assert_eq!(cells.iter().flatten().count(), 31);
        // fevereiro de 2026 começa num domingo também; abril, numa quarta
        assert_eq!(month_cells(2026, 4)[3], Some(1));
        let fmt = pdf_core::FieldFormat::Date("dd/mm/yyyy".into());
        assert_eq!(picked_date(&fmt, 2026, 3, 5), "05/03/2026");
    }

    #[test]
    fn editing_a_text_field_does_not_keep_the_key_focus() {
        // o Enter digitado no campo seguinte (depois do Tab) não pode reabrir este
        let mut st = FormState::default();
        st.focus_for_test(Some((0, 1)));
        let mut out = Vec::new();
        open(&mut st, &f(1, 0, 0.0, 0.0, "t", text()), &mut out);
        assert!(!st.has_focus());
        assert!(st.draft.is_some());
    }

    #[test]
    fn a_reset_button_asks_to_clear_the_form() {
        let mut st = FormState::default();
        let mut out = Vec::new();
        let b = f(1, 0, 0.0, 0.0, "Limpar", FieldKind::Button { reset: true });
        open(&mut st, &b, &mut out);
        assert!(matches!(out[..], [FieldAction::Reset]));
    }

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
            keystroke: Default::default(),
            format: Default::default(),
            validate: Default::default(),
            calc: Default::default(),
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
    fn tab_follows_the_form_order_with_one_stop_per_radio_group() {
        let radio = || FieldKind::Radio { on: "a".into() };
        // na ordem de /Annots (a do autor), não na geométrica
        let fields = vec![
            f(3, 0, 100.0, 10.0, "c", text()),
            f(4, 0, 200.0, 10.0, "r", radio()),
            f(1, 0, 10.0, 200.0, "b", text()),
            f(5, 0, 200.0, 100.0, "r", radio()),
            f(6, 1, 0.0, 0.0, "d", text()),
        ];
        assert_eq!(tab_order(&fields), [(0, 3), (0, 4), (0, 1), (1, 6)]);
        assert_eq!(
            next_field(&fields, (1, 6), false),
            Some((0, 3)),
            "dá a volta"
        );
        assert_eq!(next_field(&fields, (0, 3), true), Some((1, 6)));
        assert_eq!(
            next_field(&fields, (0, 5), false),
            Some((0, 1)),
            "do segundo botão do grupo, segue depois do grupo"
        );
    }

    #[test]
    fn opening_another_field_keeps_what_was_typed() {
        let mut st = FormState::default();
        let mut out = Vec::new();
        let (a, b) = (
            f(1, 0, 0.0, 0.0, "a", text()),
            f(2, 0, 50.0, 0.0, "b", text()),
        );
        open(&mut st, &a, &mut out);
        st.draft.as_mut().unwrap().text = "digitado".into();
        open(&mut st, &b, &mut out);
        assert!(matches!(&out[..], [FieldAction::Set { id: 1, value, .. }] if value == "digitado"));
        assert!(!st.has_draft(), "aberto sem mudar nada");
        st.draft.as_mut().unwrap().text = "outro".into();
        assert!(st.has_draft());
        assert!(matches!(st.commit(), Some(FieldAction::Set { id: 2, .. })));
        assert!(!st.has_draft() && st.commit().is_none());
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
    fn clicking_the_chosen_radio_changes_nothing() {
        let mut r = f(1, 0, 0.0, 0.0, "r", FieldKind::Radio { on: "a".into() });
        r.value = "a".into();
        let mut out = Vec::new();
        open(&mut FormState::default(), &r, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn escape_closes_the_list_then_releases_the_field() {
        let mut st = FormState {
            choices: Some((0, 1)),
            focused: Some((0, 1)),
            ..Default::default()
        };
        assert!(st.escape());
        assert!(
            !st.escape(),
            "nada mais a fechar: Esc segue para as ferramentas"
        );
    }

    #[test]
    fn text_respects_the_character_limit() {
        assert_eq!(limited("Conceição", Some(4)), "Conc");
        assert_eq!(limited("abc", None), "abc");
    }
}
