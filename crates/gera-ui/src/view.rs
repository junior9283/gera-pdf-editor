use app_state::cache::{LruCache, Weighed};
use app_state::layout::Layout;
use app_state::zoom;
use pdf_core::{CancelToken, PageSize, RenderedPage, Request, Worker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// (página, escala quantizada, invertido)
pub type RenderKey = (i32, u32, bool);

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

    /// Escala de renderização: zoom do usuário vezes pixels por ponto da tela.
    pub fn page_scale(&self) -> f32 {
        self.zoom * self.ppp
    }

    fn key(&self, page: i32, scale: f32) -> RenderKey {
        (page, zoom::quantize(scale), self.dark)
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
