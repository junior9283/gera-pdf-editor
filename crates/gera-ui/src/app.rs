use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::search::SearchState;
use crate::sidebar::{self, SidebarAction, SidebarTab};
use crate::view::DocView;
use app_state::tools::Tool;
use app_state::zoom;
use pdf_core::{AnnotInfo, DocSummary, OutlineItem, PdfError, Request, Response, Worker};
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
    pub(crate) edit: EditState,
    /// Anotações já gravadas no PDF aberto.
    pub(crate) saved_annots: Vec<AnnotInfo>,
    /// Janela "descartar alterações?" aberta (o usuário tentou fechar com anotações pendentes).
    confirm_close: bool,
    tab: SidebarTab,
    outline: Vec<OutlineItem>,
    /// Caminho que está sendo aberto (para pedir a senha de um PDF protegido).
    opening: Option<PathBuf>,
    /// Pedido de senha: (caminho, senha digitada).
    password_prompt: Option<(PathBuf, String)>,
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
            edit: EditState::default(),
            saved_annots: Vec::new(),
            confirm_close: false,
            tab: SidebarTab::default(),
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
            bench,
        };
        if let Some(path) = path {
            app.open(path);
        }
        app
    }

    pub(crate) fn open(&mut self, path: PathBuf) {
        self.open_with(path, None);
    }

    fn open_with(&mut self, path: PathBuf, password: Option<String>) {
        self.status = format!("Abrindo {}…", path.display());
        self.opening = Some(path.clone());
        self.worker.send(Request::Open { path, password });
    }

    fn on_opened(&mut self, summary: DocSummary) {
        self.saved_annots = summary.annots;
        self.outline = summary.outline;
        self.password_prompt = None;
        self.edit.reset();
        self.status = format!(
            "{} — {} páginas",
            summary.path.display(),
            summary.sizes.len()
        );
        let dark = self.view.as_ref().is_some_and(|v| v.dark); // o tema escuro vale para a sessão
        let mut view = DocView::new(summary.path, summary.sizes);
        view.dark = dark;
        self.view = Some(view);
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
                Response::OpenFailed(
                    e @ (PdfError::PasswordRequired | PdfError::WrongPassword),
                ) => {
                    self.status = e.to_string();
                    self.password_prompt = self.opening.clone().map(|p| (p, String::new()));
                }
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
                Response::Words { tag, page, words } => self.edit.on_words(ctx, tag, page, words),
                Response::Saved { annots } => {
                    self.edit.reset();
                    self.saved_annots = annots;
                    if let Some(view) = self.view.as_mut() {
                        view.clear_cache(); // as páginas precisam ser renderizadas de novo, agora com as anotações
                    }
                    self.status = "Salvo.".into();
                }
                Response::SaveFailed(msg) => {
                    self.edit.saving = false;
                    self.status = format!("Não foi possível salvar: {msg}");
                }
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
            for tool in Tool::ALL {
                ui.selectable_value(&mut self.edit.tool, tool, tool.label());
            }
            ui.separator();
            if ui
                .add_enabled(self.edit.pending.is_dirty(), egui::Button::new("↶"))
                .clicked()
            {
                self.edit.pending.undo();
            }
            if ui.button("↷").clicked() {
                self.edit.pending.redo();
            }
            let save_label = if self.edit.pending.is_dirty() {
                "Salvar •"
            } else {
                "Salvar"
            };
            if ui
                .add_enabled(!self.edit.saving, egui::Button::new(save_label))
                .clicked()
            {
                self.edit.save(&self.worker, view.path.clone());
            }
            ui.separator();
            if ui
                .button(if view.dark { "Claro" } else { "Escuro" })
                .clicked()
            {
                view.dark = !view.dark;
                view.clear_cache();
                ui.ctx().set_visuals(if view.dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
            }
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

    /// Painel lateral (miniaturas, índice, anotações); clicar leva à página.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .default_size(150.0)
            .show(ui, |ui| {
                let Some(view) = self.view.as_mut() else {
                    return;
                };
                let action = sidebar::show(
                    ui,
                    &mut self.tab,
                    view,
                    &self.worker,
                    &self.outline,
                    &self.saved_annots,
                    self.edit.pending.items(),
                );
                if let SidebarAction::JumpToPage(page) = action {
                    if let Some(y) = view.scroll_target(page as i32, 0.0) {
                        self.jump_to = Some(y);
                    }
                }
            });
    }

    /// Pedido de senha para abrir um PDF protegido (a senha não é registrada em lugar nenhum).
    fn password_window(&mut self, ui: &mut egui::Ui) {
        let Some((path, mut password)) = self.password_prompt.take() else {
            return;
        };
        let (mut submit, mut cancel) = (false, false);
        egui::Window::new("PDF protegido por senha")
            .collapsible(false)
            .show(ui, |ui| {
                ui.label(&self.status); // "protegido por senha" ou "senha incorreta"
                let edit = ui.add(egui::TextEdit::singleline(&mut password).password(true));
                submit = ui.button("Abrir").clicked()
                    || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                cancel = ui.button("Cancelar").clicked();
            });
        if submit {
            self.open_with(path, Some(password));
        } else if !cancel {
            self.password_prompt = Some((path, password));
        }
    }

    /// Confirmação de saída com anotações não salvas.
    fn confirm_close_window(&mut self, ui: &mut egui::Ui) {
        if !self.confirm_close {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Window::new("Alterações não salvas")
            .collapsible(false)
            .show(ui, |ui| {
                ui.label(format!(
                    "Há {} anotação(ões) que ainda não foram salvas no PDF.",
                    self.edit.pending.items().len()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Sair sem salvar").clicked() {
                        self.edit.reset(); // sem pendentes, o fechamento não pede confirmação de novo
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Voltar").clicked() {
                        self.confirm_close = false;
                    }
                });
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
        let (worker, edit, search) = (&self.worker, &mut self.edit, &self.search);
        self.last = canvas::show_pages(
            ui,
            view,
            worker,
            self.jump_to.take(),
            &mut |ui, page, page_rect| {
                paint_search_hits(ui, search, page as i32, page_rect, zoom);
                edit.on_page(ui, worker, page as i32, page_rect, zoom);
            },
        );
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
        // Fechar com anotações pendentes pede confirmação, em vez de perdê-las em silêncio.
        if ctx.input(|i| i.viewport().close_requested()) && self.edit.pending.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        // refazer (Ctrl+Shift+Z) antes de desfazer (Ctrl+Z), para um atalho não consumir o outro
        let (redo, undo, save) = ctx.input_mut(|i| {
            (
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
            )
        });
        if redo {
            self.edit.pending.redo();
        } else if undo {
            self.edit.pending.undo();
        }
        if let (true, Some(view)) = (save, self.view.as_ref()) {
            self.edit.save(&self.worker, view.path.clone());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(view) = self.view.as_mut() {
            view.begin_frame();
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            let unsaved = self.edit.pending.items().len();
            let text = match unsaved {
                0 => self.status.clone(),
                n => format!("{} — {n} alterações não salvas", self.status),
            };
            ui.label(text);
        });
        if self.view.is_some() {
            self.sidebar(ui);
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.edit.note_window(ui);
        self.confirm_close_window(ui);
        self.password_window(ui);
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
        let color = if n == search.current_index() {
            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 230, 0, 90)
        };
        ui.painter()
            .rect_filled(screen_rect(r, page_rect.min, zoom), 2.0, color);
    }
}
