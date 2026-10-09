use crate::view::DocView;
use pdf_core::Worker;

#[derive(Default)]
pub struct CanvasFrame {
    pub scroll_y: f32,
    pub current_page: usize,
    /// Altura visível da área de rolagem (para PageUp/PageDown).
    pub viewport_h: f32,
}

/// Rola e desenha só as páginas visíveis (mais uma de cada lado, pré-carregadas).
/// `on_page` roda depois de pintar cada página visível, para sobrepor anotações e tratar o mouse.
pub fn show_pages(
    ui: &mut egui::Ui,
    view: &mut DocView,
    worker: &Worker,
    jump_to: Option<f32>,
    on_page: &mut dyn FnMut(&mut egui::Ui, usize, egui::Rect),
) -> CanvasFrame {
    view.set_ppp(ui.ctx().pixels_per_point());
    view.set_max_side(ui.ctx().input(|i| i.max_texture_side) as f32);
    let mut area = egui::ScrollArea::both().auto_shrink(false);
    if let Some(y) = jump_to {
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    let out = area.show_viewport(ui, |ui, viewport| {
        let total = egui::vec2(view.layout.max_width(), view.layout.total_height());
        ui.set_min_size(total);
        let origin = ui.max_rect().min;
        let content_w = ui.max_rect().width().max(total.x);
        let visible = view.layout.visible_range(viewport.min.y, viewport.height());
        for i in view.layout.expand(visible.clone(), 1) {
            let scale = view.scale_for(i);
            view.request_at(worker, i as i32, scale);
        }
        for i in visible {
            let w = view.layout.page_width(i);
            let x = origin.x + (content_w - w).max(0.0) / 2.0;
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, origin.y + view.layout.page_top(i)),
                egui::vec2(w, view.layout.page_height(i)),
            );
            let scale = view.scale_for(i);
            paint_page(ui, view, i as i32, rect, scale);
            on_page(ui, i, rect);
        }
        view.layout
            .page_at(viewport.min.y + viewport.height() * 0.3)
    });
    CanvasFrame {
        scroll_y: out.state.offset.y,
        current_page: out.inner,
        viewport_h: out.inner_rect.height(),
    }
}

fn paint_page(ui: &egui::Ui, view: &mut DocView, page: i32, rect: egui::Rect, scale: f32) {
    let texture = view
        .texture_at(page, scale)
        .or_else(|| view.any_texture(page));
    let painter = ui.painter();
    match texture {
        Some(id) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(id, rect, uv, egui::Color32::WHITE);
        }
        None => {
            let gray = if view.dark { 30 } else { 245 };
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(gray));
        }
    }
}
