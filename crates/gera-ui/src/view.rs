use app_state::cache::{LruCache, Weighed};
use app_state::layout::Layout;
use app_state::zoom;
use pdf_core::{CancelToken, PageSize, RenderedPage, Request, Worker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// (página, escala quantizada, invertido)
pub type RenderKey = (i32, u32, bool);

/// Orçamento de pixels de uma página renderizada na tela (16 Mpx = 64 MB em RGBA); além disso a
/// página é só ampliada a partir de uma imagem com menos detalhe, em vez de gastar memória.
const MAX_PAGE_PIXELS: f32 = 16_000_000.0;
/// Maior lado de textura assumido até a interface informar o limite real da GPU.
const DEFAULT_MAX_SIDE: f32 = 8192.0;

/// Orçamento de texturas de página em memória (RGBA).
const CACHE_BUDGET_BYTES: usize = 96 * 1024 * 1024;

struct CachedTexture {
    handle: egui::TextureHandle,
    bytes: usize,
}

impl Weighed for CachedTexture {
    fn weight(&self) -> usize {
        self.bytes
    }
}

pub struct DocView {
    pub path: PathBuf,
    pub sizes: Vec<PageSize>,
    pub zoom: f32,
    pub fit_pending: bool,
    pub layout: Layout,
    pub dark: bool,
    ppp: f32,
    /// Maior lado de textura aceito pela GPU (em pixels).
    max_side: f32,
    cache: LruCache<RenderKey, CachedTexture>,
    requested: HashMap<RenderKey, CancelToken>,
    wanted: HashSet<RenderKey>,
}

impl DocView {
    pub fn new(path: PathBuf, sizes: Vec<PageSize>) -> Self {
        let mut view = Self {
            path,
            sizes,
            zoom: zoom::DEFAULT_ZOOM,
            fit_pending: true,
            layout: Layout::default(),
            dark: false,
            ppp: 1.0,
            max_side: DEFAULT_MAX_SIDE,
            cache: LruCache::new(CACHE_BUDGET_BYTES),
            requested: HashMap::new(),
            wanted: HashSet::new(),
        };
        view.relayout();
        view
    }

    fn relayout(&mut self) {
        let dims: Vec<(f32, f32)> = self.sizes.iter().map(|s| (s.width, s.height)).collect();
        self.layout = Layout::new(&dims, self.zoom);
    }

    /// Aplica o zoom (limitado) e refaz o layout; devolve a razão novo/antigo.
    pub fn set_zoom(&mut self, z: f32) -> f32 {
        let new = zoom::clamp_zoom(z);
        let ratio = new / self.zoom;
        self.zoom = new;
        self.relayout();
        ratio
    }

    pub fn fit_width(&mut self, avail: f32) -> f32 {
        let page_w = self.sizes.first().map_or(1.0, |s| s.width);
        self.set_zoom(zoom::fit_width(page_w, avail))
    }

    pub fn set_ppp(&mut self, ppp: f32) {
        if ppp.is_finite() && ppp > 0.0 {
            self.ppp = ppp;
        }
    }

    /// Limite de textura da GPU (`ctx.input(|i| i.max_texture_side)`).
    pub fn set_max_side(&mut self, side: f32) {
        if side.is_finite() && side >= 1.0 {
            self.max_side = side;
        }
    }

    /// Escala de renderização da página `page`: a do zoom, limitada pelo que a GPU, a memória e o
    /// `pdf-core` aceitam para o tamanho dela (a página é ampliada a partir da imagem menor).
    pub fn scale_for(&self, page: usize) -> f32 {
        let Some(size) = self.sizes.get(page) else {
            return self.page_scale();
        };
        let (w, h) = (size.width.max(1.0), size.height.max(1.0));
        let cap = (self.max_side / w.max(h))
            .min((MAX_PAGE_PIXELS / (w * h)).sqrt())
            .min(pdf_core::max_render_scale(size));
        self.page_scale().min(cap)
    }

    /// Escala de renderização: zoom do usuário vezes pixels por ponto da tela.
    pub fn page_scale(&self) -> f32 {
        self.zoom * self.ppp
    }

    /// `true` se `scale` é a escala em que o canvas mostra a página `page` (e não a de uma miniatura).
    pub fn is_page_scale(&self, page: usize, scale: f32) -> bool {
        zoom::quantize(scale) == zoom::quantize(self.scale_for(page))
    }

    fn key(&self, page: i32, scale: f32) -> RenderKey {
        (page, zoom::quantize(scale), self.dark)
    }

    /// Posição de rolagem (em pontos da tela) que mostra `y_pts` da página `page`, com folga acima.
    pub fn scroll_target(&self, page: i32, y_pts: f32) -> Option<f32> {
        let i = usize::try_from(page)
            .ok()
            .filter(|&i| i < self.layout.page_count())?;
        Some((self.layout.page_top(i) + y_pts * self.zoom - 80.0).max(0.0))
    }

    pub fn begin_frame(&mut self) {
        self.wanted.clear();
    }

    /// Cancela os pedidos de render que ninguém pediu neste quadro.
    pub fn end_frame(&mut self) {
        let wanted = &self.wanted;
        self.requested.retain(|k, token| {
            let keep = wanted.contains(k);
            if !keep {
                token.cancel();
            }
            keep
        });
    }

    pub fn texture_at(&mut self, page: i32, scale: f32) -> Option<egui::TextureId> {
        let key = self.key(page, scale);
        self.cache.get(&key).map(|t| t.handle.id())
    }

    /// Qualquer textura da página (de outra escala): serve de placeholder enquanto a nítida não chega.
    pub fn any_texture(&self, page: i32) -> Option<egui::TextureId> {
        let dark = self.dark;
        let key = self.cache.keys().find(|k| k.0 == page && k.2 == dark)?;
        self.cache.peek(key).map(|t| t.handle.id())
    }

    pub fn request_at(&mut self, worker: &Worker, page: i32, scale: f32) {
        let key = self.key(page, scale);
        self.wanted.insert(key);
        if self.cache.contains(&key) || self.requested.contains_key(&key) {
            return;
        }
        let cancel = CancelToken::new();
        self.requested.insert(key, cancel.clone());
        worker.send(Request::Render {
            page,
            scale,
            invert: self.dark,
            rotation: pdf_core::Rotation::R0,
            cancel,
        });
    }

    pub fn on_rendered(
        &mut self,
        ctx: &egui::Context,
        page: i32,
        scale: f32,
        invert: bool,
        img: RenderedPage,
    ) {
        let key = (page, zoom::quantize(scale), invert);
        self.requested.remove(&key);
        if invert != self.dark {
            return; // resposta atrasada de antes de trocar o tema
        }
        let image = egui::ColorImage::from_rgb([img.width as usize, img.height as usize], &img.rgb);
        let handle = ctx.load_texture(
            format!("p{}-{}", key.0, key.1),
            image,
            egui::TextureOptions::LINEAR,
        );
        let bytes = img.width as usize * img.height as usize * 4;
        self.cache.insert(key, CachedTexture { handle, bytes });
    }

    pub fn clear_cache(&mut self) {
        for token in self.requested.values() {
            token.cancel();
        }
        self.requested.clear();
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> DocView {
        let sizes = vec![
            PageSize {
                width: 100.0,
                height: 200.0
            };
            3
        ];
        DocView::new(PathBuf::from("x.pdf"), sizes)
    }

    #[test]
    fn scroll_target_points_slightly_above_the_position_in_the_page() {
        let mut v = view();
        v.set_zoom(2.0);
        // página 1 começa em 12 + 400 + 12 = 424; y = 50pt * 2 = 100 => 524 - 80 de folga
        assert_eq!(v.scroll_target(1, 50.0), Some(444.0));
        assert_eq!(v.scroll_target(0, 0.0), Some(0.0), "nunca negativo");
    }

    #[test]
    fn render_scale_never_exceeds_what_the_gpu_and_memory_allow() {
        let sizes = vec![
            PageSize {
                width: 200.0,
                height: 3000.0,
            },
            PageSize {
                width: 612.0,
                height: 792.0,
            },
        ];
        let mut v = DocView::new(PathBuf::from("rolo.pdf"), sizes);
        v.set_ppp(2.0); // tela HiDPI
        v.set_zoom(8.0); // zoom máximo
                         // página 0: o lado maior não pode passar do limite de textura (8192 por padrão)
        assert!(
            3000.0 * v.scale_for(0) <= 8192.0 + 0.5,
            "{}",
            v.scale_for(0)
        );
        // página 1: o orçamento de pixels da interface (16 Mpx) manda
        let px = 612.0 * 792.0 * v.scale_for(1).powi(2);
        assert!(px <= 16_000_000.0 * 1.001, "{px}");
        // limite maior da GPU permite escala maior
        v.set_max_side(16384.0);
        assert!(v.scale_for(0) > 8192.0 / 3000.0);
        // zoom normal não é afetado
        v.set_zoom(1.0);
        assert_eq!(v.scale_for(1), v.page_scale());
    }

    #[test]
    fn page_renders_are_told_apart_from_thumbnails_by_scale() {
        let mut v = view();
        v.set_zoom(1.5);
        assert!(v.is_page_scale(0, v.scale_for(0)));
        assert!(
            !v.is_page_scale(0, 110.0 / 100.0),
            "miniatura tem outra escala"
        );
    }

    #[test]
    fn a_capped_page_render_is_still_recognised_as_the_page_render() {
        let sizes = vec![PageSize {
            width: 200.0,
            height: 3000.0,
        }];
        let mut v = DocView::new(PathBuf::from("rolo.pdf"), sizes);
        v.set_zoom(8.0);
        assert!(v.scale_for(0) < v.page_scale(), "a escala foi limitada");
        assert!(v.is_page_scale(0, v.scale_for(0)));
    }

    #[test]
    fn scroll_target_rejects_pages_outside_the_document() {
        let v = view();
        assert_eq!(v.scroll_target(-1, 0.0), None);
        assert_eq!(v.scroll_target(3, 0.0), None);
    }
}
