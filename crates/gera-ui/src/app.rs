use crate::canvas::{self, CanvasFrame};
use crate::search::SearchState;
use crate::view::DocView;
use app_state::zoom;
use pdf_core::{DocSummary, Request, Response, Worker};
use std::path::PathBuf;
use std::time::Instant;

/// Texturas novas por quadro (cada uma custa conversão e envio à GPU na thread da UI).
const MAX_RENDERS_PER_FRAME: usize = 2;

pub struct GeraApp {
    pub(crate) worker: Worker,
    pub(crate) view: Option<DocView>,
    pub(crate) status: String,
    /// Último quadro do canvas (rolagem e página atual).
    pub(crate) last: CanvasFrame,
    pub(crate) jump_to: Option<f32>,
    pub(crate) search: SearchState,
    /// Modo de medição (`--bench-open`): instante de início do processo.
    bench: Option<Instant>,
}

impl GeraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        bench: Option<Instant>,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        let worker = Worker::spawn(move || ctx.request_repaint());
        let mut app = Self {
            worker,
            view: None,
            status: "Arraste um PDF para cá ou execute: gera-pdf-editor arquivo.pdf".into(),
            last: CanvasFrame::default(),
            jump_to: None,
            search: SearchState::default(),
            bench,
        };
        if let Some(path) = path {
            app.open(path);
        }
        app
    }

    pub(crate) fn open(&mut self, path: PathBuf) {
        self.status = format!("Abrindo {}…", path.display());
        self.worker.send(Request::Open {
            path,
            password: None,
        });
    }

    fn on_opened(&mut self, summary: DocSummary) {
        self.status = format!(
            "{} — {} páginas",
            summary.path.display(),
            summary.sizes.len()
        );
        self.view = Some(DocView::new(summary.path, summary.sizes));
        self.search.text.clear(); // descarta a busca (e os destaques) do documento anterior
        self.search.start(&self.worker);
        self.last = CanvasFrame::default();
        self.jump_to = Some(0.0);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut renders = 0;
        while let Ok(response) = self.worker.rx.try_recv() {
            match response {
                Response::Opened(summary) => self.on_opened(summary),
                Response::OpenFailed(e) => self.status = format!("Erro: {e}"),
                Response::Rendered {
                    page,
                    scale,
                    invert,
                    image,
                } => {
                    if let Some(view) = self.view.as_mut() {
                        view.on_rendered(ctx, page, scale, invert, image);
                    }
                    self.finish_bench(ctx);
                    renders += 1;
                    if renders >= MAX_RENDERS_PER_FRAME {
                        ctx.request_repaint(); // o resto das respostas fica para o próximo quadro
                        break;
                    }
                }
                Response::RenderFailed { page, error } => {
                    self.status = format!("Página {}: {error}", page + 1)
                }
                Response::SearchHits { id, page, rects } => {
                    let had_hits = self.search.current().is_some();
                    self.search.on_hits(id, page, rects);
                    // rola até o primeiro resultado (resultados de busca antiga não mudam `current`)
                    let first = self.search.current().filter(|_| !had_hits);
                    if let Some(y) =
                        first.and_then(|(p, r)| self.view.as_ref()?.scroll_target(p, r.y0))
                    {
                        self.jump_to = Some(y);
                    }
                }
                Response::SearchDone { id } => self.search.on_done(id),
                _ => {}
            }
        }
    }

    /// Modo de medição: imprime o tempo até a primeira página e a memória residente, e fecha.
    fn finish_bench(&mut self, ctx: &egui::Context) {
        let Some(started) = self.bench.take() else {
            return;
        };
        let rss_kb = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("VmRSS:"))
                    .and_then(|v| v.split_whitespace().next().map(str::to_owned))
            })
            .unwrap_or_default();
        println!(
            "BENCH open_ms={} rss_kb={rss_kb}",
            started.elapsed().as_millis()
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.iter().map(|f| f.path().to_path_buf()) {
            if self.view.is_none() {
                self.open(file);
            } else if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(file).spawn(); // uma janela por documento
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let Some(view) = self.view.as_mut() else {
                return;
            };
            let mut new_zoom = None;
            if ui.button("−").clicked() {
                new_zoom = Some(zoom::zoom_out(view.zoom));
            }
            ui.label(format!("{:.0}%", view.zoom * 100.0));
            if ui.button("+").clicked() {
                new_zoom = Some(zoom::zoom_in(view.zoom));
            }
            if ui.button("Largura").clicked() {
                view.fit_pending = true;
            }
            if let Some(z) = new_zoom {
                let ratio = view.set_zoom(z);
                self.jump_to = Some(self.last.scroll_y * ratio);
            }
            ui.separator();
            ui.label(format!(
                "Página {} / {}",
                self.last.current_page + 1,
                view.layout.page_count()
            ));
            ui.separator();
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search.text)
                    .hint_text("Buscar…")
                    .desired_width(160.0),
            );
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.search.start(&self.worker);
                edit.request_focus();
            }
            ui.label(self.search.summary());
            let prev = ui.button("▲").clicked();
            let next = ui.button("▼").clicked();
            if prev || next {
                let target = self.search.step(next);
                if let Some(y) = target.and_then(|(page, rect)| view.scroll_target(page, rect.y0)) {
                    self.jump_to = Some(y);
                }
            }
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.as_mut() else {
            ui.centered_and_justified(|ui| {
                ui.label(&self.status);
            });
            return;
        };
        if view.fit_pending {
            let ratio = view.fit_width(ui.available_width() - 24.0);
            view.fit_pending = false;
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
        let zoom = view.zoom;
        let search = &self.search;
        self.last = canvas::show_pages(
            ui,
            view,
            &self.worker,
            self.jump_to.take(),
            &mut |ui, page, page_rect| paint_search_hits(ui, search, page as i32, page_rect, zoom),
        );
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(view) = self.view.as_mut() {
            view.begin_frame();
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
        }
    }
}

/// Realça os resultados da busca na página (o atual em laranja).
fn paint_search_hits(
    ui: &egui::Ui,
    search: &SearchState,
    page: i32,
    page_rect: egui::Rect,
    zoom: f32,
) {
    for (n, r) in search.hits_on(page) {
        let a = egui::pos2(page_rect.min.x + r.x0 * zoom, page_rect.min.y + r.y0 * zoom);
        let b = egui::pos2(page_rect.min.x + r.x1 * zoom, page_rect.min.y + r.y1 * zoom);
        let color = if n == search.current_index() {
            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 230, 0, 90)
        };
        ui.painter()
            .rect_filled(egui::Rect::from_two_pos(a, b), 2.0, color);
    }
}
