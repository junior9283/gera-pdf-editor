use app_state::cache::{LruCache, Weighed};
use app_state::layout::{Layout, ViewMode, PAGE_GAP};
use app_state::transform::{self, PageMap};
use app_state::view_history::ViewEntry;
use app_state::zoom::{self, Fit};
use pdf_core::{CancelToken, PageSize, RenderedPage, Request, Rotation, Worker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// (página, escala quantizada, invertido, rotação)
pub type RenderKey = (i32, u32, bool, Rotation);

/// Orçamento de pixels de uma página renderizada na tela (16 Mpx = 64 MB em RGBA); além disso a
/// página é só ampliada a partir de uma imagem com menos detalhe, em vez de gastar memória.
const MAX_PAGE_PIXELS: f32 = 16_000_000.0;
/// Maior lado de textura assumido até a interface informar o limite real da GPU.
const DEFAULT_MAX_SIDE: f32 = 8192.0;

/// Orçamento de texturas de página em memória (RGBA).
const CACHE_BUDGET_BYTES: usize = 96 * 1024 * 1024;
/// Orçamento dos ladrilhos de alta resolução: cabe o teto de um quadro (`MAX_TILES` de 1 MB), para
/// os visíveis nunca se expulsarem, com folga para a borda entre duas páginas. Só é usado em zoom
/// acima do teto da página inteira.
const TILE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
// os ladrilhos de um quadro cabem no cache (senão se expulsam e o worker renderiza sem parar), e o
// teto cobre uma tela 4K inteira (9 × 6 ladrilhos de 512 px)
const _: () = assert!(
    app_state::tiles::MAX_TILES * (pdf_core::TILE * pdf_core::TILE * 4) as usize
        <= TILE_BUDGET_BYTES
);
const _: () = assert!(app_state::tiles::MAX_TILES >= 9 * 6);

/// (página, escala quantizada, invertido, rotação, coluna, linha)
pub type TileKey = (i32, u32, bool, Rotation, u32, u32);

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
    pub fit: Fit,
    pub rotation: Rotation,
    pub mode: ViewMode,
    /// Modo duas páginas: a primeira fica sozinha (capa).
    pub cover: bool,
    /// Modos página a página: linha (página ou par) mostrada.
    pub spread: usize,
    /// Tamanho de cada página na tela com zoom 1, já girada.
    dims: Vec<(f32, f32)>,
    /// Largura e altura da maior linha com zoom 1 (base dos ajustes).
    base: (f32, f32),
    pub layout: Layout,
    pub dark: bool,
    ppp: f32,
    /// Maior lado de textura aceito pela GPU (em pixels).
    max_side: f32,
    cache: LruCache<RenderKey, CachedTexture>,
    requested: HashMap<RenderKey, CancelToken>,
    wanted: HashSet<RenderKey>,
    tiles: LruCache<TileKey, CachedTexture>,
    requested_tiles: HashMap<TileKey, CancelToken>,
    wanted_tiles: HashSet<TileKey>,
    /// Ajustar ao conteúdo visível: caixa da tinta da página (pontos do MuPDF) e a página dela.
    pub visible_box: Option<(usize, pdf_core::PdfRect)>,
}

impl DocView {
    pub fn new(path: PathBuf, sizes: Vec<PageSize>) -> Self {
        let mut view = Self {
            path,
            sizes,
            zoom: zoom::DEFAULT_ZOOM,
            fit: Fit::default(),
            rotation: Rotation::R0,
            mode: ViewMode::default(),
            cover: false,
            spread: 0,
            dims: Vec::new(),
            base: (1.0, 1.0),
            layout: Layout::default(),
            dark: false,
            ppp: 1.0,
            max_side: DEFAULT_MAX_SIDE,
            cache: LruCache::new(CACHE_BUDGET_BYTES),
            requested: HashMap::new(),
            wanted: HashSet::new(),
            tiles: LruCache::new(TILE_BUDGET_BYTES),
            requested_tiles: HashMap::new(),
            wanted_tiles: HashSet::new(),
            visible_box: None,
        };
        view.reshape();
        view
    }

    /// (largura, altura) de cada página, já com a rotação da página.
    pub fn page_dims(&self) -> Vec<(f32, f32)> {
        self.sizes.iter().map(|s| (s.width, s.height)).collect()
    }

    /// As páginas mudaram (organizar ou desfazer): tamanhos novos, sem nada do cache antigo.
    pub fn set_pages(&mut self, sizes: Vec<PageSize>) {
        self.sizes = sizes;
        self.clear_cache();
        self.reshape();
    }

    /// Rotação, modo ou capa mudaram: refaz as dimensões giradas e a base dos ajustes.
    fn reshape(&mut self) {
        self.dims = self
            .sizes
            .iter()
            .map(|s| transform::rotated((s.width, s.height), self.rotation))
            .collect();
        let unit = Layout::with_mode(&self.dims, 1.0, self.mode, self.cover);
        self.base = (unit.max_width(), unit.max_row_height());
        self.relayout();
    }

    /// Só o zoom mudou: um layout novo, sem recalcular as dimensões.
    fn relayout(&mut self) {
        self.layout = Layout::with_mode(&self.dims, self.zoom, self.mode, self.cover);
        self.spread = self.spread.min(self.layout.last_row());
    }

    /// Aplica o zoom (limitado) e refaz o layout; devolve a razão novo/antigo.
    pub fn set_zoom(&mut self, z: f32) -> f32 {
        let new = zoom::clamp_zoom(z);
        let ratio = new / self.zoom;
        self.zoom = new;
        self.relayout();
        ratio
    }

    /// Zoom escolhido pelo usuário: desliga o ajuste automático. Devolve a razão novo/antigo.
    pub fn set_manual_zoom(&mut self, z: f32) -> f32 {
        self.fit = Fit::None;
        self.set_zoom(z)
    }

    /// Aplica o ajuste ativo à área visível; devolve a razão do zoom se ele mudou.
    pub fn apply_fit(&mut self, avail_w: f32, avail_h: f32) -> Option<f32> {
        let z = match self.fit {
            Fit::None => return None,
            Fit::Width => zoom::fit_width(self.base.0, avail_w),
            Fit::Page => zoom::fit_page(self.base, (avail_w, avail_h)),
            Fit::Height => zoom::fit_height(self.base.1, avail_h),
            Fit::Visible => {
                let width = self.visible_extent()?.1;
                zoom::fit_width(width, avail_w * 0.96)
            }
        };
        (zoom::quantize(z) != zoom::quantize(self.zoom)).then(|| self.set_zoom(z))
    }

    /// Ajustar ao conteúdo visível: (borda esquerda, largura) da caixa da tinta na tela com zoom 1,
    /// relativas à página dela, já com a rotação da vista.
    fn visible_extent(&self) -> Option<(f32, f32)> {
        let (page, r) = self.visible_box?;
        let map = PageMap {
            zoom: 1.0,
            ..self.page_map(page, (0.0, 0.0))
        };
        let xs = [(r.x0, r.y0), (r.x1, r.y1)].map(|p| map.to_screen(p).0);
        let (left, right) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
        (right - left > 1.0).then_some((left, right - left))
    }

    /// Rolagem horizontal que põe a borda esquerda do conteúdo visível na margem da janela.
    pub fn visible_left(&self, avail_w: f32) -> Option<f32> {
        let page = self.visible_box?.0;
        let (left, _) = self.visible_extent()?;
        Some((self.layout.page_x(page) + left * self.zoom - avail_w * 0.02).max(0.0))
    }

    pub fn set_rotation(&mut self, rotation: Rotation) {
        if rotation != self.rotation {
            self.rotation = rotation;
            self.clear_cache(); // as texturas da rotação antiga não servem mais
            self.reshape();
        }
    }

    /// Troca o modo de exibição mantendo `current_page` na tela.
    pub fn set_mode(&mut self, mode: ViewMode, current_page: usize) {
        self.mode = mode;
        self.reshape();
        self.spread = self.layout.row_of(current_page);
    }

    /// Modos página a página: avança (`+1`) ou volta (`-1`) uma tela; devolve se mudou.
    pub fn turn(&mut self, step: i32) -> bool {
        let last = self.layout.last_row() as i64;
        let new = (self.spread as i64 + i64::from(step)).clamp(0, last) as usize;
        std::mem::replace(&mut self.spread, new) != new
    }

    /// Faixa vertical do layout mostrada na área de rolagem: `(topo, altura)`. Tudo, ou só a
    /// linha atual nos modos página a página.
    pub fn window(&self) -> (f32, f32) {
        if self.mode.is_paged() && self.layout.row_count() > 0 {
            (
                self.layout.row_top(self.spread) - PAGE_GAP,
                self.layout.row_height(self.spread) + 2.0 * PAGE_GAP,
            )
        } else {
            (0.0, self.layout.total_height())
        }
    }

    /// Mapa tela ↔ PDF da página `page` desenhada a partir de `origin`.
    pub fn page_map(&self, page: usize, origin: (f32, f32)) -> PageMap {
        let size = self
            .sizes
            .get(page)
            .map_or((1.0, 1.0), |s| (s.width, s.height));
        PageMap {
            origin,
            zoom: self.zoom,
            size,
            rotation: self.rotation,
        }
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
        (page, zoom::quantize(scale), self.dark, self.rotation)
    }

    /// Posição de rolagem que mostra a página `page` (no ponto `at` dela, ou no topo), com folga
    /// acima. Nos modos página a página também troca a tela.
    pub fn go_to(&mut self, page: i32, at: Option<(f32, f32)>) -> Option<f32> {
        let i = usize::try_from(page)
            .ok()
            .filter(|&i| i < self.layout.page_count())?;
        if self.mode.is_paged() {
            self.spread = self.layout.row_of(i);
        }
        let inside = at.map_or(0.0, |p| self.page_map(i, (0.0, 0.0)).to_screen(p).1);
        Some((self.layout.page_top(i) - self.window().0 + inside - 80.0).max(0.0))
    }

    /// Primeira página da linha `step` linhas adiante (ou atrás) da que tem `page`; `None` se não
    /// houver. Uma linha é uma página, ou um par nos modos de duas páginas.
    pub fn page_step(&self, page: usize, step: i32) -> Option<usize> {
        let row = self
            .layout
            .row_of(page.min(self.layout.page_count().checked_sub(1)?));
        let target = row.checked_add_signed(step as isize)?;
        (target != row && target <= self.layout.last_row())
            .then(|| self.layout.row_pages(target).start)
    }

    /// A vista atual (`scroll_y` da área de rolagem, `page` em cima), para o histórico.
    pub fn view_entry(&self, scroll_y: f32, page: usize) -> ViewEntry {
        let top = self
            .layout
            .page_top(page.min(self.layout.page_count().saturating_sub(1)));
        ViewEntry {
            page,
            y: (scroll_y + self.window().0 - top) / self.zoom,
            zoom: self.zoom,
            fit: self.fit,
        }
    }

    /// Volta à vista `e` (zoom, ajuste e posição); devolve a rolagem. `None` se a página não existe
    /// mais.
    pub fn restore(&mut self, e: &ViewEntry) -> Option<f32> {
        (e.page < self.layout.page_count()).then_some(())?;
        self.fit = e.fit;
        self.set_zoom(e.zoom);
        if self.mode.is_paged() {
            self.spread = self.layout.row_of(e.page);
        }
        let y = self.layout.page_top(e.page) - self.window().0 + e.y * self.zoom;
        Some(y.max(0.0))
    }

    /// Escala da página inteira menor que a do zoom: o canvas cobre a parte visível com ladrilhos.
    pub fn needs_tiles(&self, page: usize) -> bool {
        self.scale_for(page) < self.page_scale() * 0.999
    }

    /// Ladrilhos da página desenhada em `rect` (tela) que aparecem em `clip`; vazio se a imagem da
    /// página inteira já tem a nitidez do zoom.
    pub fn wanted_tiles(&self, page: usize, rect: egui::Rect, clip: egui::Rect) -> Vec<(u32, u32)> {
        if !self.needs_tiles(page) {
            return Vec::new();
        }
        let px = |v: f32| v * self.ppp;
        let vis = rect.intersect(clip);
        app_state::tiles::visible_tiles(
            (px(rect.width()), px(rect.height())),
            (
                px(vis.min.x - rect.min.x),
                px(vis.min.y - rect.min.y),
                px(vis.max.x - rect.min.x),
                px(vis.max.y - rect.min.y),
            ),
            pdf_core::TILE as f32,
        )
    }

    /// Ladrilho da página: pede se ainda não pediu e devolve a textura e onde desenhá-la dentro da
    /// página (`offset`, `size` em pontos de tela), se já chegou.
    pub fn tile(
        &mut self,
        worker: &Worker,
        page: i32,
        tile: (u32, u32),
    ) -> Option<(egui::TextureId, egui::Vec2, egui::Vec2)> {
        let scale = self.page_scale();
        let key = (
            page,
            zoom::quantize(scale),
            self.dark,
            self.rotation,
            tile.0,
            tile.1,
        );
        self.wanted_tiles.insert(key);
        if let Some(t) = self.tiles.get(&key) {
            let side = pdf_core::TILE as f32 / self.ppp;
            let offset = egui::vec2(tile.0 as f32 * side, tile.1 as f32 * side);
            return Some((t.handle.id(), offset, t.handle.size_vec2() / self.ppp));
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = self.requested_tiles.entry(key) {
            let cancel = slot.insert(CancelToken::new()).clone();
            worker.send(Request::RenderTile {
                page,
                scale,
                invert: self.dark,
                rotation: self.rotation,
                tile,
                cancel,
            });
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn on_tile(
        &mut self,
        ctx: &egui::Context,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        tile: (u32, u32),
        img: RenderedPage,
    ) {
        let key = (
            page,
            zoom::quantize(scale),
            invert,
            rotation,
            tile.0,
            tile.1,
        );
        // como em `on_rendered`: só o que ainda está sendo esperado
        if self.requested_tiles.remove(&key).is_none()
            || invert != self.dark
            || rotation != self.rotation
        {
            return;
        }
        let name = format!("t{page}-{}-{}-{}", key.1, tile.0, tile.1);
        self.tiles.insert(key, upload(ctx, name, &img));
    }

    pub fn begin_frame(&mut self) {
        self.wanted.clear();
        self.wanted_tiles.clear();
    }

    /// Cancela os pedidos de render que ninguém pediu neste quadro.
    pub fn end_frame(&mut self) {
        let (wanted, wanted_tiles) = (&self.wanted, &self.wanted_tiles);
        cancel_where(&mut self.requested, |k| !wanted.contains(k));
        cancel_where(&mut self.requested_tiles, |k| !wanted_tiles.contains(k));
    }

    pub fn texture_at(&mut self, page: i32, scale: f32) -> Option<egui::TextureId> {
        let key = self.key(page, scale);
        self.cache.get(&key).map(|t| t.handle.id())
    }

    /// Qualquer textura da página (de outra escala): serve de placeholder enquanto a nítida não chega.
    pub fn any_texture(&self, page: i32) -> Option<egui::TextureId> {
        let (dark, rotation) = (self.dark, self.rotation);
        let key = self
            .cache
            .keys()
            .find(|k| k.0 == page && k.2 == dark && k.3 == rotation)?;
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
            rotation: self.rotation,
            cancel,
        });
    }

    pub fn on_rendered(
        &mut self,
        ctx: &egui::Context,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        img: RenderedPage,
    ) {
        let key = (page, zoom::quantize(scale), invert, rotation);
        // Só entra no cache o que ainda está sendo esperado: respostas de pedidos cancelados
        // (outra rotação, outro tema, aba suspensa) seriam memória desperdiçada.
        if self.requested.remove(&key).is_none() || invert != self.dark || rotation != self.rotation
        {
            return;
        }
        let name = format!("p{}-{}", key.0, key.1);
        self.cache.insert(key, upload(ctx, name, &img));
    }

    /// Descarta as texturas e os pedidos da página (o conteúdo dela mudou).
    pub fn invalidate_page(&mut self, page: i32) {
        cancel_where(&mut self.requested, |k| k.0 == page);
        self.cache.retain(|k| k.0 != page);
        cancel_where(&mut self.requested_tiles, |k| k.0 == page);
        self.tiles.retain(|k| k.0 != page);
    }

    pub fn clear_cache(&mut self) {
        cancel_where(&mut self.requested, |_| true);
        self.cache.clear();
        cancel_where(&mut self.requested_tiles, |_| true);
        self.tiles.clear();
    }
}

/// Cancela e esquece os pedidos de render em que `drop` é verdadeiro.
fn cancel_where<K>(requested: &mut HashMap<K, CancelToken>, drop: impl Fn(&K) -> bool) {
    requested.retain(|k, token| {
        let gone = drop(k);
        if gone {
            token.cancel();
        }
        !gone
    });
}

/// Sobe a imagem renderizada para a GPU, com o peso dela para o orçamento do cache.
fn upload(ctx: &egui::Context, name: String, img: &RenderedPage) -> CachedTexture {
    let image = egui::ColorImage::from_rgb([img.width as usize, img.height as usize], &img.rgb);
    let handle = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
    let bytes = img.width as usize * img.height as usize * 4;
    CachedTexture { handle, bytes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_state::layout::{ViewMode, PAGE_GAP};
    use app_state::zoom::Fit;
    use pdf_core::{RenderedPage, Rotation};

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
    fn go_to_points_slightly_above_the_position_in_the_page() {
        let mut v = view();
        v.set_zoom(2.0);
        // página 1 começa em 12 + 400 + 12 = 424; y = 50pt * 2 = 100 => 524 - 80 de folga
        assert_eq!(v.go_to(1, Some((0.0, 50.0))), Some(444.0));
        assert_eq!(v.go_to(0, None), Some(0.0), "nunca negativo");
    }

    #[test]
    fn go_to_rejects_pages_outside_the_document() {
        let mut v = view();
        assert_eq!(v.go_to(-1, None), None);
        assert_eq!(v.go_to(3, None), None);
    }

    #[test]
    fn rotation_swaps_the_layout_and_points_follow_it() {
        let mut v = view(); // 3 páginas 100x200
        v.set_rotation(Rotation::R90);
        assert_eq!(
            (v.layout.page_width(0), v.layout.page_height(0)),
            (200.0, 100.0)
        );
        // a 90°, o y do PDF vira x na tela: o ponto (30, 50) fica 30pt abaixo do topo
        let m = v.page_map(0, (0.0, 0.0));
        assert_eq!(m.to_screen((30.0, 50.0)), (150.0, 30.0));
    }

    #[test]
    fn tiles_are_wanted_only_above_the_cap() {
        let mut v = view(); // 100 × 200 pt
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 200.0));
        let clip = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        assert!(
            v.wanted_tiles(0, rect, clip).is_empty(),
            "a página inteira basta"
        );
        v.set_max_side(256.0); // GPU pequena: 200 pt viram no máximo 256 px
        v.set_manual_zoom(8.0);
        let big = egui::Rect::from_min_size(egui::pos2(-300.0, -900.0), egui::vec2(800.0, 1600.0));
        let tiles = v.wanted_tiles(0, big, clip);
        // visível: x 300..800 e y 900..1500 da página → colunas 0..=1, linhas 1..=2
        assert_eq!(tiles, vec![(0, 1), (1, 1), (0, 2), (1, 2)]);
    }

    #[test]
    fn fit_visible_follows_the_content_width() {
        let mut v = view(); // páginas de 100 pt
        v.fit = Fit::Visible;
        assert_eq!(v.apply_fit(500.0, 500.0), None, "sem a caixa, espera");
        v.visible_box = Some((
            1,
            pdf_core::PdfRect {
                x0: 20.0,
                y0: 10.0,
                x1: 70.0,
                y1: 50.0,
            },
        ));
        v.apply_fit(500.0, 500.0);
        assert!((v.zoom - 500.0 * 0.96 / 50.0).abs() < 1e-3, "{}", v.zoom);
        let left = v.visible_left(500.0).unwrap();
        assert!((left - (v.layout.page_x(1) + 20.0 * v.zoom - 10.0)).abs() < 1e-3);
        v.fit = Fit::Height;
        v.apply_fit(500.0, 400.0);
        assert!((v.zoom - 2.0).abs() < 1e-3, "200 pt em 400");
    }

    #[test]
    fn page_steps_move_one_row_in_every_mode() {
        let mut v = view();
        assert_eq!(v.page_step(0, 1), Some(1));
        assert_eq!(v.page_step(2, 1), None, "já na última");
        assert_eq!(v.page_step(0, -1), None);
        v.set_mode(ViewMode::TwoUpContinuous, 0);
        assert_eq!(v.page_step(0, 1), Some(2), "o par seguinte");
        assert_eq!(v.page_step(3, -1), Some(0));
    }

    #[test]
    fn a_saved_view_comes_back_to_the_same_place() {
        let mut v = view();
        v.set_manual_zoom(2.0);
        let scroll = v.layout.page_top(1) + 30.0; // 15 pt abaixo do topo da página 1
        let e = v.view_entry(scroll, 1);
        assert_eq!((e.page, e.fit), (1, Fit::None));
        assert!((e.y - 15.0).abs() < 1e-3 && (e.zoom - 2.0).abs() < 1e-6);
        v.set_manual_zoom(1.0);
        v.fit = Fit::Page;
        let y = v.restore(&e).unwrap();
        assert!((v.zoom - 2.0).abs() < 1e-6 && v.fit == Fit::None);
        assert!((y - scroll).abs() < 1e-3, "{y} != {scroll}");
        // modo página a página: troca a tela e conta a partir dela
        v.set_mode(ViewMode::Single, 0);
        let y = v.restore(&e).unwrap();
        assert_eq!(v.spread, 1);
        assert!((y - (PAGE_GAP + 30.0)).abs() < 1e-3, "{y}");
        assert_eq!(v.restore(&ViewEntry { page: 9, ..e }), None);
    }

    #[test]
    fn paged_modes_turn_one_row_at_a_time() {
        let mut v = view();
        v.set_mode(ViewMode::Single, 1);
        assert_eq!(v.spread, 1, "abre na página que estava na tela");
        assert!(v.turn(1));
        assert!(!v.turn(1), "não passa da última");
        assert_eq!(v.spread, 2);
        let (top, h) = v.window();
        assert_eq!(
            (top, h),
            (v.layout.row_top(2) - PAGE_GAP, 200.0 + 2.0 * PAGE_GAP)
        );
        assert_eq!(v.go_to(0, None), Some(0.0));
        assert_eq!(v.spread, 0, "ir para uma página troca a tela");
        v.go_to(2, None);
        assert_eq!(v.spread, 2);
    }

    #[test]
    fn fit_follows_the_window_until_the_user_zooms() {
        let mut v = view(); // páginas de 100pt de largura
        assert_eq!(v.fit, Fit::Width);
        assert!(v.apply_fit(400.0, 1000.0).is_some());
        assert!((v.zoom - 4.0).abs() < 1e-3);
        assert!(
            v.apply_fit(400.0, 1000.0).is_none(),
            "mesmo tamanho: nada muda"
        );
        v.fit = Fit::Page; // página 100x200 em 400x300: a altura manda (1.5)
        v.apply_fit(400.0, 300.0);
        assert!((v.zoom - 1.5).abs() < 1e-3);
        v.set_manual_zoom(2.0);
        assert_eq!(v.fit, Fit::None);
        assert!(v.apply_fit(800.0, 800.0).is_none());
    }

    #[test]
    fn renders_nobody_is_waiting_for_are_dropped() {
        let ctx = egui::Context::default();
        let mut v = view();
        let img = RenderedPage {
            width: 1,
            height: 1,
            rgb: vec![255, 255, 255],
        };
        // nenhum pedido feito: a resposta (atrasada, de uma aba suspensa) não entra no cache
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R0, img.clone());
        assert!(v.texture_at(0, 1.0).is_none());
        let worker = Worker::spawn(|| {});
        v.request_at(&worker, 0, 1.0);
        // resposta de outra rotação (anterior a girar a vista) também é descartada
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R90, img.clone());
        assert!(v.texture_at(0, 1.0).is_none());
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R0, img);
        assert!(v.texture_at(0, 1.0).is_some());
    }

    #[test]
    fn invalidate_page_drops_only_that_page() {
        let ctx = egui::Context::default();
        let worker = Worker::spawn(|| {});
        let mut v = view();
        let img = RenderedPage {
            width: 1,
            height: 1,
            rgb: vec![255, 255, 255],
        };
        for p in [0, 1] {
            v.request_at(&worker, p, 1.0);
            v.on_rendered(&ctx, p, 1.0, false, Rotation::R0, img.clone());
        }
        v.invalidate_page(0);
        assert!(v.texture_at(0, 1.0).is_none());
        assert!(v.texture_at(1, 1.0).is_some(), "as outras páginas ficam");
    }
}
