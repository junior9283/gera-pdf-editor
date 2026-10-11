use crate::view::DocView;
use app_state::nav::paged_step;
use app_state::transform::PageMap;
use pdf_core::Worker;

#[derive(Default)]
pub struct CanvasFrame {
    pub scroll_y: f32,
    pub current_page: usize,
    /// Altura visível da área de rolagem (para PageUp/PageDown).
    pub viewport_h: f32,
    /// Modos página a página: a roda pediu a tela seguinte (`+1`) ou a anterior (`-1`).
    pub turn: i32,
}

/// Rola e desenha só as páginas visíveis (mais as vizinhas, pré-carregadas). Nos modos página a
/// página mostra só a linha atual. `on_page` roda depois de pintar cada página visível, para
/// sobrepor anotações e tratar o mouse.
pub fn show_pages(
    ui: &mut egui::Ui,
    view: &mut DocView,
    worker: &Worker,
    jump_to: Option<f32>,
    jump_x: Option<f32>,
    on_page: &mut dyn FnMut(&mut egui::Ui, usize, egui::Rect, &PageMap),
) -> CanvasFrame {
    view.set_ppp(ui.ctx().pixels_per_point());
    view.set_max_side(ui.ctx().input(|i| i.max_texture_side) as f32);
    let paged = view.mode.is_paged();
    let (win_top, win_h) = view.window();
    let mut area = egui::ScrollArea::both().auto_shrink(false);
    if let Some(y) = jump_to {
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    if let Some(x) = jump_x {
        area = area.horizontal_scroll_offset(x.max(0.0));
    }
    let out = area.show_viewport(ui, |ui, viewport| {
        let total = egui::vec2(view.layout.max_width(), win_h);
        ui.set_min_size(total);
        let origin = ui.max_rect().min;
        let left = origin.x + (ui.max_rect().width() - total.x).max(0.0) / 2.0;
        let (visible, around) = if paged {
            let row = view.layout.row_pages(view.spread);
            let around = view.layout.expand(row.clone(), view.mode.columns());
            (row, around)
        } else {
            let visible = view.layout.visible_range(viewport.min.y, viewport.height());
            let around = view.layout.expand(visible.clone(), 1);
            (visible, around)
        };
        for i in around {
            let scale = view.scale_for(i);
            view.request_at(worker, i as i32, scale);
        }
        for i in visible.clone() {
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    left + view.layout.page_x(i),
                    origin.y + view.layout.page_top(i) - win_top,
                ),
                egui::vec2(view.layout.page_width(i), view.layout.page_height(i)),
            );
            let scale = view.scale_for(i);
            paint_page(ui, view, i as i32, rect, scale);
            paint_tiles(ui, view, worker, i, rect);
            let map = view.page_map(i, rect.min.into());
            on_page(ui, i, rect, &map);
        }
        if paged {
            visible.start
        } else {
            view.layout
                .page_at(viewport.min.y + viewport.height() * 0.3)
        }
    });
    let turn = if paged && ui.rect_contains_pointer(out.inner_rect) {
        let max_offset = (out.content_size.y - out.inner_rect.height()).max(0.0);
        paged_step(out.state.offset.y, max_offset, wheel_y(ui))
    } else {
        0
    };
    CanvasFrame {
        scroll_y: out.state.offset.y,
        current_page: out.inner,
        viewport_h: out.inner_rect.height(),
        turn,
    }
}

/// Giro da roda neste quadro (sem Ctrl, que é zoom). Lido dos eventos brutos porque a área de
/// rolagem consome o delta suavizado.
fn wheel_y(ui: &egui::Ui) -> f32 {
    ui.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel {
                    delta, modifiers, ..
                } if !modifiers.command => Some(delta.y),
                _ => None,
            })
            .sum()
    })
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

/// Acima do teto da página inteira, cobre a parte visível com ladrilhos na escala exata do zoom
/// (os que ainda não chegaram deixam ver a página ampliada por baixo).
fn paint_tiles(ui: &egui::Ui, view: &mut DocView, worker: &Worker, page: usize, rect: egui::Rect) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    for tile in view.wanted_tiles(page, rect, ui.clip_rect()) {
        view.request_tile(worker, page as i32, tile);
        if let Some((id, offset, size)) = view.tile_texture(page as i32, tile) {
            let at = egui::Rect::from_min_size(rect.min + offset, size);
            ui.painter().image(id, at, uv, egui::Color32::WHITE);
        }
    }
}
