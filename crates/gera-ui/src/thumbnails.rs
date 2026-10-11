//! Painel Miniaturas de página (como no Acrobat): seleção com Ctrl e Shift, arrastar para
//! reordenar ou até outra aba (copia as páginas), menu de contexto com as ações do Organizar
//! páginas, `Delete` (depois de confirmar) e setas.

use crate::organize::{OrgAction, OrganizeState};
use crate::view::DocView;
use app_state::organize::can_delete;
use app_state::transform;
use egui::Key;
use pdf_core::{PageOp, PageSize, Rotation, Worker};

/// Largura das miniaturas: padrão e limites de Ampliar/Reduzir.
pub const THUMB_WIDTH: f32 = 110.0;
const WIDTH_RANGE: std::ops::RangeInclusive<f32> = 80.0..=320.0;
/// Altura do rótulo da página sob a miniatura.
const LABEL: f32 = 22.0;

/// Tamanho da página como aparece na miniatura (com a rotação da vista).
fn shown(size: &PageSize, rotation: Rotation) -> (f32, f32) {
    transform::rotated((size.width.max(1.0), size.height.max(1.0)), rotation)
}

/// Escala para que a miniatura tenha `width` pontos de largura.
pub fn thumb_scale(size: &PageSize, rotation: Rotation, width: f32) -> f32 {
    width / shown(size, rotation).0
}

pub fn thumb_height(size: &PageSize, rotation: Rotation, width: f32) -> f32 {
    let (w, h) = shown(size, rotation);
    width * h / w
}

/// Onde as páginas arrastadas caem: antes da linha (metade de cima) ou depois.
pub fn drop_index(y_in_row: f32, row_h: f32, row: usize) -> i32 {
    if y_in_row < row_h / 2.0 {
        row as i32
    } else {
        row as i32 + 1
    }
}

/// Mover `pages` para antes de `to`; nada quando já estão ali.
pub fn move_op(pages: &[i32], to: i32) -> Option<PageOp> {
    let (&first, &last) = (pages.first()?, pages.last()?);
    let contiguous = pages.windows(2).all(|w| w[1] == w[0] + 1);
    if contiguous && (first..=last + 1).contains(&to) {
        return None;
    }
    Some(PageOp::Move {
        pages: pages.to_vec(),
        to,
    })
}

/// Ampliar ou Reduzir miniaturas (passos de 20 %).
pub fn zoom(width: f32, enlarge: bool) -> f32 {
    let w = if enlarge { width * 1.2 } else { width / 1.2 };
    w.clamp(*WIDTH_RANGE.start(), *WIDTH_RANGE.end())
}

/// Páginas (a partir de 0) como faixas do diálogo Imprimir: "1-3, 5".
pub fn ranges_text(pages: &[i32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut k = 0;
    while k < pages.len() {
        let start = pages[k];
        let mut end = start;
        while k + 1 < pages.len() && pages[k + 1] == end + 1 {
            k += 1;
            end = pages[k];
        }
        out.push(if start == end {
            format!("{}", start + 1)
        } else {
            format!("{}-{}", start + 1, end + 1)
        });
        k += 1;
    }
    out.join(", ")
}

/// O que uma tecla faz com o painel focado.
#[derive(Debug, Clone, PartialEq)]
pub enum ThumbKey {
    Select(i32),
    /// Pergunta antes de excluir (como no Acrobat).
    ConfirmDelete(Vec<i32>),
}

pub fn key_action(key: Key, sel: &[i32], current: Option<i32>, total: i32) -> Option<ThumbKey> {
    if total <= 0 {
        return None;
    }
    match key {
        Key::Delete => {
            can_delete(sel.len(), total as usize).then(|| ThumbKey::ConfirmDelete(sel.to_vec()))
        }
        Key::ArrowDown => match current {
            None => Some(ThumbKey::Select(0)),
            Some(c) => (c + 1 < total).then_some(ThumbKey::Select(c + 1)),
        },
        Key::ArrowUp => match current {
            None => Some(ThumbKey::Select(0)),
            Some(c) => (c > 0).then_some(ThumbKey::Select(c - 1)),
        },
        Key::Home => Some(ThumbKey::Select(0)),
        Key::End => Some(ThumbKey::Select(total - 1)),
        _ => None,
    }
}

/// O que o painel pede à aba.
pub enum ThumbAction {
    Go(i32),
    /// Ação do Organizar páginas (girar, excluir, extrair, inserir, substituir, recortar).
    Org(OrgAction),
    /// Imprimir páginas…: o diálogo Imprimir com estas páginas.
    Print(Vec<i32>),
    /// Nova largura das miniaturas (vai para as preferências).
    Zoom(f32),
    /// Copiar para ▸ outro documento aberto (o caminho identifica a aba).
    CopyTo {
        pages: Vec<i32>,
        target: std::path::PathBuf,
    },
    /// Arrastadas e soltas fora do painel: talvez sobre outra aba.
    DragOut(Vec<i32>),
    /// Rótulos de página… com estas páginas selecionadas.
    Labels(Vec<i32>),
}

pub struct ThumbnailsPanel {
    /// Seleção e diálogo aberto (os mesmos do Organizar páginas).
    pub org: OrganizeState,
    pub width: f32,
    /// Teclado no painel (depois de um clique numa miniatura).
    pub focused: bool,
    /// Página do último clique (as setas andam a partir dela).
    current: Option<i32>,
    /// Arraste em andamento: as páginas e onde cairiam.
    drag: Option<(Vec<i32>, Option<i32>)>,
    /// Excluir pedido pelo teclado, esperando a confirmação.
    confirm: Option<Vec<i32>>,
}

impl Default for ThumbnailsPanel {
    fn default() -> Self {
        Self {
            org: OrganizeState::default(),
            width: THUMB_WIDTH,
            focused: false,
            current: None,
            drag: None,
            confirm: None,
        }
    }
}

impl ThumbnailsPanel {
    fn select(&mut self, page: i32, out: &mut Vec<ThumbAction>) {
        self.org.sel.click(page);
        self.current = Some(page);
        out.push(ThumbAction::Go(page));
    }

    /// Excluir (Delete ou Editar → Excluir com o painel focado): pergunta antes.
    pub fn ask_delete(&mut self, total: i32) {
        let sel = self.org.sel.to_vec();
        if let Some(ThumbKey::ConfirmDelete(pages)) =
            key_action(Key::Delete, &sel, self.current, total)
        {
            self.confirm = Some(pages);
        }
    }

    /// Páginas esperando a confirmação de excluir.
    #[cfg(test)]
    pub fn confirming(&self) -> Option<&[i32]> {
        self.confirm.as_deref()
    }

    /// Teclas com o painel focado.
    pub fn keys(&mut self, ctx: &egui::Context, total: i32) -> Vec<ThumbAction> {
        let mut out = Vec::new();
        let sel = self.org.sel.to_vec();
        // Delete chega como o comando Excluir (ask_delete)
        for key in [Key::ArrowDown, Key::ArrowUp, Key::Home, Key::End] {
            if !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                continue;
            }
            if let Some(ThumbKey::Select(p)) = key_action(key, &sel, self.current, total) {
                self.select(p, &mut out);
            }
        }
        out
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &mut DocView,
        worker: &Worker,
        labels: &[String],
        can_edit: bool,
        targets: &[(std::path::PathBuf, String)],
    ) -> Vec<ThumbAction> {
        let mut out = Vec::new();
        let total = view.sizes.len();
        self.org.sel.clamp(total as i32);
        ui.horizontal(|ui| {
            for (text, enlarge, tip) in [
                ("−", false, "Reduzir miniaturas"),
                ("+", true, "Ampliar miniaturas"),
            ] {
                if ui.small_button(text).on_hover_text(tip).clicked() {
                    self.width = zoom(self.width, enlarge);
                    out.push(ThumbAction::Zoom(self.width));
                }
            }
        });
        let Some(first) = view.sizes.first().copied() else {
            return out;
        };
        let panel = ui.max_rect();
        let rotation = view.rotation;
        let width = self.width.min(ui.available_width().max(40.0));
        let thumb_h = thumb_height(&first, rotation, width);
        let row_h = thumb_h + LABEL;
        let pointer = ui.ctx().pointer_interact_pos();
        let mut menu_pages = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show_rows(ui, row_h, total, |ui, rows| {
                for i in rows {
                    let page = i as i32;
                    let scale = thumb_scale(&view.sizes[i], rotation, width);
                    view.request_at(worker, page, scale);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Sense::click_and_drag(),
                    );
                    let img = egui::Rect::from_center_size(
                        egui::pos2(rect.center().x, rect.top() + thumb_h / 2.0),
                        egui::vec2(width, thumb_h),
                    );
                    let painter = ui.painter();
                    match view.texture_at(page, scale) {
                        Some(id) => {
                            let uv = egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            );
                            painter.image(id, img, uv, egui::Color32::WHITE);
                        }
                        None => {
                            painter.rect_filled(img, 0.0, egui::Color32::from_gray(200));
                        }
                    }
                    if self.org.sel.contains(page) {
                        painter.rect_stroke(
                            img.expand(2.0),
                            2.0,
                            egui::Stroke::new(3.0, ui.visuals().selection.bg_fill),
                            egui::StrokeKind::Outside,
                        );
                    }
                    let label = labels
                        .get(i)
                        .map(|l| crate::links::safe_display(l))
                        .unwrap_or_else(|| (i + 1).to_string());
                    painter.text(
                        egui::pos2(rect.center().x, img.bottom() + LABEL / 2.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    if resp.clicked() {
                        self.focused = true;
                        let mods = ui.input(|inp| inp.modifiers);
                        if mods.command {
                            self.org.sel.ctrl_click(page);
                            self.current = Some(page);
                        } else if mods.shift {
                            self.org.sel.shift_click(page);
                            self.current = Some(page);
                        } else {
                            self.select(page, &mut out);
                        }
                    }
                    // arrastar também copia para outra aba, mesmo sem poder alterar este
                    if resp.drag_started() {
                        if !self.org.sel.contains(page) {
                            self.org.sel.click(page);
                        }
                        self.drag = Some((self.org.sel.to_vec(), None));
                    }
                    // alvo do arraste: antes ou depois da miniatura sob o ponteiro
                    if let (Some((_, target)), Some(p)) = (self.drag.as_mut(), pointer) {
                        if rect.contains(p) {
                            let to = drop_index(p.y - rect.top(), row_h, i);
                            *target = Some(to);
                            let y = if to == page {
                                rect.top()
                            } else {
                                rect.bottom()
                            };
                            ui.painter().hline(
                                rect.x_range(),
                                y,
                                egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                            );
                        }
                    }
                    resp.context_menu(|ui| {
                        if !self.org.sel.contains(page) {
                            self.org.sel.click(page);
                            self.current = Some(page);
                        }
                        menu_pages = Some(self.org.sel.to_vec());
                        self.menu(ui, total, can_edit, targets, &mut out);
                    });
                }
            });
        let _ = menu_pages;
        // soltou: move dentro do painel (fora dele, nada por enquanto)
        if self.drag.is_some() && ui.input(|i| !i.pointer.any_down()) {
            if let Some((pages, target)) = self.drag.take() {
                let inside = pointer.is_some_and(|p| panel.contains(p));
                if !inside {
                    out.push(ThumbAction::DragOut(pages));
                } else if let (true, Some(op)) =
                    (can_edit, target.and_then(|to| move_op(&pages, to)))
                {
                    // as páginas movidas continuam selecionadas, no lugar novo
                    if let Some(to) = target {
                        self.org.sel.after_move(to);
                        self.current = self.org.sel.to_vec().first().copied();
                    }
                    out.push(ThumbAction::Org(OrgAction::Op(op)));
                }
            }
        }
        self.confirm_window(ui.ctx(), &mut out);
        let mut orgs = Vec::new();
        crate::organize::dialogs(ui, &mut self.org, view, &mut orgs);
        out.extend(orgs.into_iter().map(ThumbAction::Org));
        out
    }

    /// Menu de contexto de uma miniatura (as ações agem sobre a seleção).
    fn menu(
        &mut self,
        ui: &mut egui::Ui,
        total: usize,
        can_edit: bool,
        targets: &[(std::path::PathBuf, String)],
        out: &mut Vec<ThumbAction>,
    ) {
        if can_edit {
            let mut orgs = Vec::new();
            crate::organize::page_menu(ui, &mut self.org, total, &mut orgs);
            out.extend(orgs.into_iter().map(ThumbAction::Org));
            ui.separator();
        }
        if can_edit && ui.button("Rótulos de página…").clicked() {
            out.push(ThumbAction::Labels(self.org.sel.to_vec()));
            ui.close();
        }
        ui.add_enabled_ui(!targets.is_empty(), |ui| {
            ui.menu_button("Copiar para", |ui| {
                for (path, title) in targets {
                    if ui.button(title).clicked() {
                        out.push(ThumbAction::CopyTo {
                            pages: self.org.sel.to_vec(),
                            target: path.clone(),
                        });
                        ui.close();
                    }
                }
            });
        });
        if ui.button("Imprimir páginas…").clicked() {
            out.push(ThumbAction::Print(self.org.sel.to_vec()));
            ui.close();
        }
        ui.separator();
        for (text, enlarge) in [("Ampliar miniaturas", true), ("Reduzir miniaturas", false)] {
            if ui.button(text).clicked() {
                self.width = zoom(self.width, enlarge);
                out.push(ThumbAction::Zoom(self.width));
                ui.close();
            }
        }
    }

    /// "Excluir N páginas?" (Delete no painel).
    fn confirm_window(&mut self, ctx: &egui::Context, out: &mut Vec<ThumbAction>) {
        let Some(pages) = self.confirm.as_ref() else {
            return;
        };
        let mut answer = None;
        let text = match pages.len() {
            1 => format!("Excluir a página {}?", pages[0] + 1),
            n => format!("Excluir {n} páginas?"),
        };
        egui::Window::new("Excluir páginas")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        answer = Some(true);
                    }
                    if ui.button("Cancelar").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        answer = Some(false);
                    }
                });
            });
        if let Some(ok) = answer {
            if let (true, Some(pages)) = (ok, self.confirm.take()) {
                out.push(ThumbAction::Org(OrgAction::Op(PageOp::Delete { pages })));
            }
            self.confirm = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_lands_before_or_after_the_row() {
        assert_eq!(drop_index(10.0, 100.0, 0), 0, "metade de cima: antes");
        assert_eq!(drop_index(60.0, 100.0, 0), 1, "metade de baixo: depois");
        assert_eq!(drop_index(99.0, 100.0, 4), 5, "depois da última");
    }

    #[test]
    fn moving_pages_onto_themselves_does_nothing() {
        assert_eq!(move_op(&[2, 3], 2), None);
        assert_eq!(move_op(&[2, 3], 4), None);
        assert_eq!(
            move_op(&[2, 3], 0),
            Some(PageOp::Move {
                pages: vec![2, 3],
                to: 0
            })
        );
        assert_eq!(move_op(&[], 1), None);
    }

    #[test]
    fn delete_never_takes_every_page() {
        assert_eq!(key_action(Key::Delete, &[0, 1, 2], Some(0), 3), None);
        assert_eq!(
            key_action(Key::Delete, &[1], Some(1), 3),
            Some(ThumbKey::ConfirmDelete(vec![1]))
        );
    }

    #[test]
    fn arrows_move_the_selection() {
        assert_eq!(
            key_action(Key::ArrowDown, &[1], Some(1), 3),
            Some(ThumbKey::Select(2))
        );
        assert_eq!(key_action(Key::ArrowDown, &[2], Some(2), 3), None, "última");
        assert_eq!(
            key_action(Key::ArrowUp, &[], None, 3),
            Some(ThumbKey::Select(0)),
            "sem seleção: a primeira"
        );
        assert_eq!(
            key_action(Key::Home, &[2], Some(2), 3),
            Some(ThumbKey::Select(0))
        );
        assert_eq!(
            key_action(Key::End, &[0], Some(0), 3),
            Some(ThumbKey::Select(2))
        );
    }

    #[test]
    fn print_ranges_merge_neighbours() {
        assert_eq!(ranges_text(&[0, 1, 2, 4, 6, 7]), "1-3, 5, 7-8");
        assert_eq!(ranges_text(&[3]), "4");
    }

    #[test]
    fn thumbnails_keep_the_page_aspect_ratio_and_rotation() {
        let size = PageSize {
            width: 100.0,
            height: 200.0,
        };
        assert_eq!(thumb_height(&size, Rotation::R0, 110.0), 220.0);
        assert_eq!(thumb_height(&size, Rotation::R90, 110.0), 55.0);
        let zero = PageSize {
            width: 0.0,
            height: 0.0,
        };
        assert!(thumb_scale(&zero, Rotation::R0, 110.0).is_finite());
    }

    #[test]
    fn zoom_steps_stay_in_range() {
        assert!((zoom(100.0, true) - 120.0).abs() < 0.01);
        assert!((zoom(300.0, true) - 320.0).abs() < 0.01);
        assert!((zoom(85.0, false) - 80.0).abs() < 0.01);
    }
}
