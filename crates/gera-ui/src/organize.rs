//! Modo Organizar páginas (como no Acrobat): grade de miniaturas com seleção, arrasto para
//! reordenar, girar, excluir, extrair, inserir, substituir e recortar. Desenha e devolve ações;
//! quem aplica é a aba.
use crate::view::DocView;
use app_state::organize::{can_delete, insert_at, PageSet, Picker};
use pdf_core::{PageOp, Worker};
use std::path::PathBuf;

/// Largura das miniaturas: padrão e limites do controle de zoom.
const THUMB: f32 = 140.0;
use app_state::prefs::THUMB_WIDTH_RANGE as THUMB_RANGE;
/// Espaço em volta da miniatura dentro da célula e altura do número da página.
const PAD: f32 = 12.0;
const LABEL: f32 = 22.0;
/// Tamanhos de página em branco (pontos).
const A4: (f32, f32) = (595.28, 841.89);
const LETTER: (f32, f32) = (612.0, 792.0);

/// Milímetros → pontos.
fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// Diálogo aberto no modo Organizar.
pub enum Dialog {
    Extract {
        delete_after: bool,
        separate: bool,
    },
    Blank {
        size: BlankSize,
        after: bool,
    },
    InsertFile {
        path: PathBuf,
        after: bool,
        all: bool,
        from: i32,
        to: i32,
        password: String,
    },
    Replace {
        path: PathBuf,
        from: i32,
        password: String,
    },
    Crop {
        mm: [f32; 4],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlankSize {
    SameAsPage,
    A4,
    Letter,
}

/// Para que o arquivo escolhido no seletor vai servir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Insert,
    Replace,
}

/// O que o usuário pediu na grade.
pub enum OrgAction {
    Op(PageOp),
    /// Escolher onde gravar a extração (depois: extrair e, se pedido, excluir).
    Extract {
        pages: Vec<i32>,
        separate: bool,
        delete_after: bool,
    },
    PickFile(Purpose),
    /// Abrir o diálogo Dividir documento.
    Split,
    Undo,
    Redo,
    Close,
}

pub struct OrganizeState {
    pub sel: PageSet,
    thumb: f32,
    /// Arrasto em andamento: página onde começou e onde as páginas cairiam.
    drag: Option<(i32, Option<i32>)>,
    pub dialog: Option<Dialog>,
}

impl Default for OrganizeState {
    fn default() -> Self {
        Self {
            sel: PageSet::default(),
            thumb: THUMB,
            drag: None,
            dialog: None,
        }
    }
}

/// Páginas sobre as quais as ações da barra agem: as selecionadas, ou nenhuma.
fn targets(st: &OrganizeState) -> Vec<i32> {
    st.sel.to_vec()
}

/// Texto do botão de excluir: diz quantas.
pub fn delete_label(n: usize) -> String {
    match n {
        0 | 1 => "Excluir".into(),
        n => format!("Excluir ({n})"),
    }
}

impl OrganizeState {
    /// Ações da barra e do menu de contexto que dependem só da seleção.
    fn rotate(&self, quarter_turns: i32) -> Option<OrgAction> {
        let pages = targets(self);
        (!pages.is_empty()).then_some(OrgAction::Op(PageOp::Rotate {
            pages,
            quarter_turns,
        }))
    }

    pub fn delete(&self, total: usize) -> Option<OrgAction> {
        let pages = targets(self);
        can_delete(pages.len(), total).then_some(OrgAction::Op(PageOp::Delete { pages }))
    }
}

/// Menu das ações sobre a seleção (botão direito numa miniatura).
pub(crate) fn page_menu(
    ui: &mut egui::Ui,
    st: &mut OrganizeState,
    total: usize,
    out: &mut Vec<OrgAction>,
) {
    if ui.button("Girar à esquerda").clicked() {
        out.extend(st.rotate(-1));
    }
    if ui.button("Girar à direita").clicked() {
        out.extend(st.rotate(1));
    }
    if ui
        .add_enabled(
            can_delete(st.sel.len(), total),
            egui::Button::new(delete_label(st.sel.len())),
        )
        .clicked()
    {
        out.extend(st.delete(total));
    }
    ui.separator();
    if ui.button("Extrair…").clicked() {
        st.dialog = Some(Dialog::Extract {
            delete_after: false,
            separate: false,
        });
    }
    ui.menu_button("Inserir páginas", |ui| insert_menu(ui, st, out));
    if ui.button("Substituir…").clicked() {
        out.push(OrgAction::PickFile(Purpose::Replace));
    }
    if ui.button("Recortar…").clicked() {
        st.dialog = Some(Dialog::Crop { mm: [0.0; 4] });
    }
}

fn insert_menu(ui: &mut egui::Ui, st: &mut OrganizeState, out: &mut Vec<OrgAction>) {
    if ui.button("De arquivo…").clicked() {
        out.push(OrgAction::PickFile(Purpose::Insert));
    }
    if ui.button("Página em branco…").clicked() {
        st.dialog = Some(Dialog::Blank {
            size: BlankSize::SameAsPage,
            after: true,
        });
    }
}

/// Barra secundária do modo Organizar.
fn toolbar(
    ui: &mut egui::Ui,
    st: &mut OrganizeState,
    view: &DocView,
    history: (bool, bool),
    out: &mut Vec<OrgAction>,
) {
    let total = view.sizes.len();
    let has_sel = !st.sel.is_empty();
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("Selecionar", |ui| {
            for picker in Picker::ALL {
                if ui.button(picker.label()).clicked() {
                    st.sel.pick(picker, &view.page_dims());
                }
            }
        });
        ui.separator();
        if ui
            .add_enabled(has_sel, egui::Button::new("⮪ Girar"))
            .on_hover_text("Girar à esquerda (anti-horário)")
            .clicked()
        {
            out.extend(st.rotate(-1));
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Girar ⮫"))
            .on_hover_text("Girar à direita (horário)")
            .clicked()
        {
            out.extend(st.rotate(1));
        }
        let can_del = can_delete(st.sel.len(), total);
        if ui
            .add_enabled(can_del, egui::Button::new(delete_label(st.sel.len())))
            .on_disabled_hover_text("Selecione páginas (o documento precisa ficar com uma)")
            .clicked()
        {
            out.extend(st.delete(total));
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Extrair…"))
            .clicked()
        {
            st.dialog = Some(Dialog::Extract {
                delete_after: false,
                separate: false,
            });
        }
        ui.menu_button("Inserir", |ui| insert_menu(ui, st, out));
        if ui
            .add_enabled(has_sel, egui::Button::new("Substituir…"))
            .clicked()
        {
            out.push(OrgAction::PickFile(Purpose::Replace));
        }
        if ui.button("Recortar…").clicked() {
            st.dialog = Some(Dialog::Crop { mm: [0.0; 4] });
        }
        if ui
            .button("Dividir…")
            .on_hover_text("Separar em vários arquivos")
            .clicked()
        {
            out.push(OrgAction::Split);
        }
        ui.separator();
        if ui
            .add_enabled(history.0, egui::Button::new("⮪"))
            .on_hover_text("Desfazer (Ctrl+Z)")
            .clicked()
        {
            out.push(OrgAction::Undo);
        }
        if ui
            .add_enabled(history.1, egui::Button::new("⮫"))
            .on_hover_text("Refazer (Ctrl+Shift+Z)")
            .clicked()
        {
            out.push(OrgAction::Redo);
        }
        ui.separator();
        ui.add(
            egui::Slider::new(&mut st.thumb, THUMB_RANGE)
                .show_value(false)
                .text("Tamanho"),
        );
        ui.separator();
        if ui
            .button("Fechar")
            .on_hover_text("Voltar ao documento (Esc)")
            .clicked()
        {
            out.push(OrgAction::Close);
        }
    });
}

/// Retângulo da imagem da página dentro da célula, mantendo a proporção.
fn fit_in(cell: egui::Rect, w: f32, h: f32) -> egui::Rect {
    let area = egui::Rect::from_min_max(
        cell.min + egui::vec2(PAD, PAD),
        egui::pos2(cell.max.x - PAD, cell.max.y - LABEL),
    );
    let scale = (area.width() / w.max(1.0)).min(area.height() / h.max(1.0));
    egui::Rect::from_center_size(area.center(), egui::vec2(w * scale, h * scale))
}

/// Desenha a grade e a barra; devolve as ações pedidas.
pub fn show(
    ui: &mut egui::Ui,
    st: &mut OrganizeState,
    view: &mut DocView,
    worker: &Worker,
    history: (bool, bool),
) -> Vec<OrgAction> {
    let mut out = Vec::new();
    toolbar(ui, st, view, history, &mut out);
    ui.separator();
    let total = view.sizes.len();
    let cell = egui::vec2(st.thumb + 2.0 * PAD, st.thumb * 1.3 + PAD + LABEL);
    let gap = ui.spacing().item_spacing.x;
    let cols = app_state::organize::grid_columns(ui.available_width() + gap, cell.x + gap);
    let rows = total.div_ceil(cols);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let released = ui.input(|i| i.pointer.any_released());
    let mut drop_at: Option<i32> = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show_rows(ui, cell.y, rows, |ui, row_range| {
            for row in row_range {
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        let i = row * cols + col;
                        if i >= total {
                            break;
                        }
                        let p = i as i32;
                        let (rect, resp) =
                            ui.allocate_exact_size(cell, egui::Sense::click_and_drag());
                        let size = view.sizes[i];
                        let img = fit_in(rect, size.width, size.height);
                        let scale = img.width() / size.width.max(1.0) * ui.ctx().pixels_per_point();
                        view.request_at(worker, p, scale);
                        let painter = ui.painter_at(rect);
                        let selected = st.sel.contains(p);
                        if selected {
                            painter.rect_filled(
                                rect.shrink(2.0),
                                6.0,
                                ui.visuals().selection.bg_fill.gamma_multiply(0.5),
                            );
                        }
                        match view.texture_at(p, scale).or_else(|| view.any_texture(p)) {
                            Some(id) => {
                                painter.image(
                                    id,
                                    img,
                                    crate::canvas::FULL_UV,
                                    egui::Color32::WHITE,
                                );
                            }
                            None => {
                                painter.rect_filled(img, 0.0, egui::Color32::from_gray(225));
                            }
                        }
                        painter.rect_stroke(
                            img,
                            0.0,
                            egui::Stroke::new(1.0, egui::Color32::from_gray(160)),
                            egui::StrokeKind::Outside,
                        );
                        painter.text(
                            egui::pos2(rect.center().x, rect.max.y - LABEL / 2.0),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            egui::FontId::proportional(13.0),
                            ui.visuals().text_color(),
                        );
                        // seleção como num gerenciador de arquivos
                        if resp.clicked() {
                            let m = ui.input(|i| i.modifiers);
                            if m.command {
                                st.sel.ctrl_click(p);
                            } else if m.shift {
                                st.sel.shift_click(p);
                            } else {
                                st.sel.click(p);
                            }
                        }
                        if resp.secondary_clicked() && !selected {
                            st.sel.click(p);
                        }
                        resp.context_menu(|ui| page_menu(ui, st, total, &mut out));
                        if resp.drag_started_by(egui::PointerButton::Primary) {
                            if !st.sel.contains(p) {
                                st.sel.click(p);
                            }
                            st.drag = Some((p, None));
                        }
                        // onde cairia o arrasto: antes ou depois desta célula, pela metade
                        if let (Some(_), Some(pos)) = (st.drag, pointer) {
                            if rect.contains(pos) {
                                let after = pos.x > rect.center().x;
                                drop_at = Some(p + i32::from(after));
                                let x = if after {
                                    rect.max.x - 1.0
                                } else {
                                    rect.min.x + 1.0
                                };
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(x, rect.min.y + 4.0),
                                        egui::pos2(x, rect.max.y - 4.0),
                                    ],
                                    egui::Stroke::new(3.0, ui.visuals().selection.stroke.color),
                                );
                            }
                        }
                    }
                });
            }
        });
    if let Some((start, _)) = st.drag {
        // fora de uma miniatura (barra, vão, fora da janela) o arrasto não cai em lugar nenhum
        st.drag = Some((start, drop_at));
        if released {
            if let Some((_, Some(to))) = st.drag {
                // soltar em cima da própria seleção não muda nada
                if let Some(op) = crate::thumbnails::move_op(&st.sel.to_vec(), to) {
                    out.push(OrgAction::Op(op));
                    st.sel.after_move(to);
                }
            }
            st.drag = None;
        }
    }
    dialogs(ui, st, view, &mut out);
    out
}

/// Diálogos do modo Organizar.
pub(crate) fn dialogs(
    ui: &mut egui::Ui,
    st: &mut OrganizeState,
    view: &DocView,
    out: &mut Vec<OrgAction>,
) {
    let total = view.sizes.len();
    let Some(dialog) = st.dialog.as_mut() else {
        return;
    };
    let mut close = false;
    let mut action = None;
    let pages = st.sel.to_vec();
    let title = match dialog {
        Dialog::Extract { .. } => "Extrair páginas",
        Dialog::Blank { .. } => "Inserir página em branco",
        Dialog::InsertFile { .. } => "Inserir páginas",
        Dialog::Replace { .. } => "Substituir páginas",
        Dialog::Crop { .. } => "Recortar páginas",
    };
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .show(ui, |ui| {
            match dialog {
                Dialog::Extract {
                    delete_after,
                    separate,
                } => {
                    ui.label(format!("Páginas selecionadas: {}", pages.len()));
                    ui.checkbox(delete_after, "Excluir páginas após extrair");
                    ui.checkbox(separate, "Extrair páginas como arquivos separados");
                    if ui.button("Extrair…").clicked() {
                        action = Some(OrgAction::Extract {
                            pages: pages.clone(),
                            separate: *separate,
                            delete_after: *delete_after && can_delete(pages.len(), total),
                        });
                    }
                }
                Dialog::Blank { size, after } => {
                    ui.label("Tamanho");
                    ui.radio_value(size, BlankSize::SameAsPage, "Igual à página selecionada");
                    ui.radio_value(size, BlankSize::A4, "A4 (210 × 297 mm)");
                    ui.radio_value(size, BlankSize::Letter, "Carta (216 × 279 mm)");
                    ui.label("Posição");
                    ui.radio_value(after, false, "Antes da seleção");
                    ui.radio_value(after, true, "Depois da seleção");
                    if ui.button("Inserir").clicked() {
                        let at = insert_at(&pages, *after, total);
                        let reference = pages.first().copied().unwrap_or(0).min(total as i32 - 1);
                        let s = view.sizes.get(reference.max(0) as usize);
                        let size = match (size, s) {
                            (BlankSize::SameAsPage, Some(s)) => (s.width, s.height),
                            (BlankSize::Letter, _) => LETTER,
                            _ => A4,
                        };
                        action = Some(OrgAction::Op(PageOp::InsertBlank { at, size }));
                    }
                }
                Dialog::InsertFile {
                    path,
                    after,
                    all,
                    from,
                    to,
                    password,
                } => {
                    ui.label(format!("Arquivo: {}", path.display()));
                    ui.radio_value(all, true, "Todas as páginas");
                    ui.horizontal(|ui| {
                        ui.radio_value(all, false, "Páginas de");
                        ui.add(egui::DragValue::new(from).range(1..=9999));
                        ui.label("a");
                        ui.add(egui::DragValue::new(to).range(1..=9999));
                    });
                    ui.radio_value(after, false, "Antes da seleção");
                    ui.radio_value(after, true, "Depois da seleção (sem seleção: no fim)");
                    ui.horizontal(|ui| {
                        ui.label("Senha (se protegido):");
                        ui.add(
                            egui::TextEdit::singleline(password)
                                .password(true)
                                .desired_width(140.0),
                        );
                    });
                    if ui.button("Inserir").clicked() {
                        action = Some(OrgAction::Op(PageOp::InsertFrom {
                            at: insert_at(&pages, *after, total),
                            path: path.clone(),
                            password: (!password.is_empty()).then(|| password.clone()),
                            range: (!*all).then_some((*from - 1, *to - 1)),
                        }));
                    }
                }
                Dialog::Replace {
                    path,
                    from,
                    password,
                } => {
                    ui.label(format!("Arquivo: {}", path.display()));
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Substituir as {} páginas selecionadas pelas páginas a partir da",
                            pages.len()
                        ));
                        ui.add(egui::DragValue::new(from).range(1..=9999));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Senha (se protegido):");
                        ui.add(
                            egui::TextEdit::singleline(password)
                                .password(true)
                                .desired_width(140.0),
                        );
                    });
                    if ui
                        .add_enabled(!pages.is_empty(), egui::Button::new("Substituir"))
                        .clicked()
                    {
                        action = Some(OrgAction::Op(PageOp::Replace {
                            pages: pages.clone(),
                            path: path.clone(),
                            password: (!password.is_empty()).then(|| password.clone()),
                            from: *from - 1,
                        }));
                    }
                }
                Dialog::Crop { mm } => {
                    let what = if pages.is_empty() {
                        "todas as páginas".to_owned()
                    } else {
                        format!("{} páginas selecionadas", pages.len())
                    };
                    ui.label(format!("Margens a cortar (mm), em {what}"));
                    egui::Grid::new("margens").show(ui, |ui| {
                        for (n, label) in ["Em cima", "Embaixo", "À esquerda", "À direita"]
                            .iter()
                            .enumerate()
                        {
                            ui.label(*label);
                            ui.add(
                                egui::DragValue::new(&mut mm[n])
                                    .range(0.0..=200.0)
                                    .suffix(" mm"),
                            );
                            ui.end_row();
                        }
                    });
                    if ui.button("Recortar").clicked() {
                        let pages = if pages.is_empty() {
                            (0..total as i32).collect()
                        } else {
                            pages.clone()
                        };
                        action = Some(OrgAction::Op(PageOp::Crop {
                            pages,
                            margins: mm.map(mm_to_pt),
                        }));
                    }
                }
            }
            if ui.button("Cancelar").clicked() {
                close = true;
            }
        });
    if let Some(a) = action {
        out.push(a);
        close = true;
    }
    if close {
        st.dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_label_counts_pages() {
        assert_eq!(delete_label(1), "Excluir");
        assert_eq!(delete_label(3), "Excluir (3)");
    }

    #[test]
    fn millimeters_become_points() {
        assert!((mm_to_pt(25.4) - 72.0).abs() < 1e-4);
    }
}
