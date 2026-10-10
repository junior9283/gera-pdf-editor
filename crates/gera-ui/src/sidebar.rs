use crate::view::DocView;
use app_state::prefs::LeftPanel;
use app_state::transform;
use pdf_core::{AnnotInfo, AnnotKind, OutlineItem, PageSize, Rotation, Worker};

pub enum SidebarAction {
    None,
    JumpToPage(usize),
    /// Seleciona a anotação e leva até ela.
    SelectAnnot {
        page: i32,
        id: i32,
        at: (f32, f32),
    },
}

const THUMB_WIDTH: f32 = 110.0;

/// Tamanho da página como aparece na miniatura (com a rotação da vista).
fn shown(size: &PageSize, rotation: Rotation) -> (f32, f32) {
    transform::rotated((size.width.max(1.0), size.height.max(1.0)), rotation)
}

/// Escala para que a miniatura da página tenha `THUMB_WIDTH` pontos de largura.
fn thumb_scale(size: &PageSize, rotation: Rotation) -> f32 {
    THUMB_WIDTH / shown(size, rotation).0
}

fn thumb_height(size: &PageSize, rotation: Rotation) -> f32 {
    let (w, h) = shown(size, rotation);
    THUMB_WIDTH * h / w
}

/// Nome do tipo da anotação, como o Acrobat mostra (a linha com ponta é "Seta").
pub(crate) fn label_of(a: &AnnotInfo) -> &'static str {
    if a.arrow {
        "Seta"
    } else {
        kind_label(a.kind)
    }
}

pub(crate) fn kind_label(kind: AnnotKind) -> &'static str {
    match kind {
        AnnotKind::Highlight => "Destaque",
        AnnotKind::Underline => "Sublinhado",
        AnnotKind::Note => "Nota",
        AnnotKind::Ink => "Desenho",
        AnnotKind::StrikeOut => "Riscado",
        AnnotKind::TextBox => "Caixa de texto",
        AnnotKind::Rect => "Retângulo",
        AnnotKind::Ellipse => "Elipse",
        AnnotKind::Line => "Linha",
        AnnotKind::Stamp => "Carimbo",
        AnnotKind::Redact => "Marca de redação",
        AnnotKind::Other => "Anotação",
    }
}

fn annot_label(a: &AnnotInfo) -> String {
    let mut label = format!("{} — p. {}", label_of(a), a.page + 1);
    if !a.contents.is_empty() {
        label.push_str(&format!(": {}", a.contents));
    }
    label
}

pub fn show(
    ui: &mut egui::Ui,
    panel: LeftPanel,
    view: &mut DocView,
    worker: &Worker,
    outline: &[OutlineItem],
    annots: &[AnnotInfo],
) -> SidebarAction {
    ui.strong(panel.label());
    ui.separator();
    match panel {
        LeftPanel::Thumbnails => thumbnails(ui, view, worker),
        LeftPanel::Outline => outline_list(ui, outline),
        LeftPanel::Annotations => annotation_list(ui, annots),
        // desenhado pela aba (precisa das assinaturas validadas)
        LeftPanel::Signatures => SidebarAction::None,
    }
}

/// Miniaturas só das linhas visíveis (`show_rows`); clicar leva à página.
fn thumbnails(ui: &mut egui::Ui, view: &mut DocView, worker: &Worker) -> SidebarAction {
    let Some(first) = view.sizes.first().copied() else {
        return SidebarAction::None;
    };
    let rotation = view.rotation;
    let thumb_h = thumb_height(&first, rotation);
    let row_h = thumb_h + 22.0;
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_h,
        view.sizes.len(),
        |ui, rows| {
            for i in rows {
                let scale = thumb_scale(&view.sizes[i], rotation);
                view.request_at(worker, i as i32, scale);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(THUMB_WIDTH, row_h), egui::Sense::click());
                let img = egui::Rect::from_min_size(rect.min, egui::vec2(THUMB_WIDTH, thumb_h));
                match view.texture_at(i as i32, scale) {
                    Some(id) => {
                        let uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        ui.painter().image(id, img, uv, egui::Color32::WHITE);
                    }
                    None => {
                        ui.painter()
                            .rect_filled(img, 0.0, egui::Color32::from_gray(200));
                    }
                }
                ui.painter().text(
                    egui::pos2(rect.center().x, img.bottom() + 10.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(12.0),
                    ui.visuals().text_color(),
                );
                if response.clicked() {
                    action = SidebarAction::JumpToPage(i);
                }
            }
        },
    );
    action
}

fn outline_list(ui: &mut egui::Ui, outline: &[OutlineItem]) -> SidebarAction {
    if outline.is_empty() {
        ui.label("Este documento não tem índice.");
        return SidebarAction::None;
    }
    let row_h = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_h,
        outline.len(),
        |ui, rows| {
            for item in &outline[rows] {
                ui.horizontal(|ui| {
                    ui.add_space(12.0 * item.depth as f32);
                    let button = egui::Button::new(&item.title).frame(false);
                    if let (true, Some(page)) = (
                        ui.add_enabled(item.page.is_some(), button).clicked(),
                        item.page,
                    ) {
                        action = SidebarAction::JumpToPage(page.max(0) as usize);
                    }
                });
            }
        },
    );
    action
}

fn annotation_list(ui: &mut egui::Ui, annots: &[AnnotInfo]) -> SidebarAction {
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // respostas e status aparecem dentro do comentário, no painel Comentários
            let mut roots = annots.iter().filter(|a| !a.is_reply()).peekable();
            if roots.peek().is_none() {
                ui.label("Nenhuma anotação.");
            }
            for a in roots {
                if ui.button(annot_label(a)).clicked() {
                    action = SidebarAction::SelectAnnot {
                        page: a.page,
                        id: a.id,
                        at: (a.rect.x0, a.rect.y0),
                    };
                }
            }
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    #[test]
    fn thumbnails_keep_the_page_aspect_ratio() {
        let size = PageSize {
            width: 100.0,
            height: 200.0,
        };
        assert_eq!(thumb_height(&size, Rotation::R0), 2.0 * THUMB_WIDTH);
        assert!((thumb_scale(&size, Rotation::R0) - THUMB_WIDTH / 100.0).abs() < 1e-6);
    }

    #[test]
    fn degenerate_page_sizes_do_not_produce_non_finite_thumbnails() {
        let zero = PageSize {
            width: 0.0,
            height: 0.0,
        };
        assert!(
            thumb_height(&zero, Rotation::R0).is_finite()
                && thumb_scale(&zero, Rotation::R0).is_finite()
        );
    }

    #[test]
    fn labels_mention_kind_page_and_text() {
        let mut a = AnnotInfo {
            id: 1,
            color: None,
            opacity: 1.0,
            width: None,
            kind: AnnotKind::Underline,
            page: 1,
            rect: PdfRect::new(0.0, 0.0, 1.0, 1.0),
            contents: String::new(),
            ..Default::default()
        };
        assert_eq!(annot_label(&a), "Sublinhado — p. 2");
        a.kind = AnnotKind::Note;
        a.contents = "olá".into();
        assert_eq!(annot_label(&a), "Nota — p. 2: olá");
    }

    #[test]
    fn thumbnails_follow_the_view_rotation() {
        let size = PageSize {
            width: 100.0,
            height: 200.0,
        };
        // a 90° a página fica deitada: a miniatura também (e a escala usa o lado que vira largura)
        assert_eq!(thumb_height(&size, Rotation::R90), THUMB_WIDTH / 2.0);
        assert!((thumb_scale(&size, Rotation::R90) - THUMB_WIDTH / 200.0).abs() < 1e-6);
        assert_eq!(thumb_height(&size, Rotation::R180), 2.0 * THUMB_WIDTH);
    }
}
