use crate::bench::Bench;
use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::search::SearchState;
use crate::sidebar::{self, SidebarAction, SidebarTab};
use crate::view::DocView;
use app_state::nav::{nav_target, NavKey};
use app_state::tools::Tool;
use app_state::zoom;
use pdf_core::{AnnotInfo, DocSummary, OutlineItem, PdfError, Request, Response, Worker};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

/// Texturas novas por quadro (cada uma custa conversão e envio à GPU na thread da UI).
const MAX_RENDERS_PER_FRAME: usize = 2;

/// Um documento aberto: worker próprio, vista, busca, edição e painel lateral.
pub struct DocTab {
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
    /// O motor de PDF (thread do worker) parou; já avisado.
    engine_stopped: bool,
    tab: SidebarTab,
    outline: Vec<OutlineItem>,
    /// Caminho que está sendo aberto (para pedir a senha de um PDF protegido).
    opening: Option<PathBuf>,
    /// Pedido de senha: (caminho, senha digitada).
    password_prompt: Option<(PathBuf, String)>,
}

impl DocTab {
    pub fn new(ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        Self {
            worker: Worker::spawn(move || ctx.request_repaint()),
            view: None,
            status: "Arraste um PDF para cá ou execute: gera-pdf-editor arquivo.pdf".into(),
            last: CanvasFrame::default(),
            jump_to: None,
            search: SearchState::default(),
            edit: EditState::default(),
            saved_annots: Vec::new(),
            engine_stopped: false,
            tab: SidebarTab::default(),
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
        }
    }

    /// Há anotações ainda não gravadas no PDF?
    pub fn is_dirty(&self) -> bool {
        self.edit.pending.is_dirty()
    }

    pub fn open(&mut self, path: PathBuf) {
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

    pub fn poll(&mut self, ctx: &egui::Context, mut bench: Option<&mut Bench>) {
        let mut renders = 0;
        loop {
            let response = match self.worker.rx.try_recv() {
                Ok(response) => response,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.engine_stopped {
                        self.engine_stopped = true;
                        self.edit.saving = false;
                        self.status =
                            "O motor de PDF parou. Salve o que for possível e reabra o aplicativo."
                                .into();
                    }
                    break;
                }
            };
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
                    let mut is_page = false;
                    if let Some(view) = self.view.as_mut() {
                        view.on_rendered(ctx, page, scale, invert, image);
                        is_page = view.is_page_scale(page as usize, scale);
                    }
                    if is_page {
                        // só a página do canvas conta, não as miniaturas
                        if let Some(bench) = bench.as_deref_mut() {
                            bench.on_first_page(ctx);
                        }
                    }
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

    /// Teclado e zoom do documento: navegação, Ctrl+roda, desfazer/refazer e salvar.
    pub fn handle_input(&mut self, ctx: &egui::Context) {
        // Ctrl+Z/Ctrl+Shift+Z desfazem/refazem anotações, a menos que o usuário esteja digitando
        // (campo de busca, nota, senha): nesse caso o atalho é do campo de texto.
        // Refazer vem antes de desfazer, para um atalho não consumir o outro.
        let typing = ctx.egui_wants_keyboard_input();
        if !typing {
            let nav = [
                (NavKey::PageUp, egui::Key::PageUp),
                (NavKey::PageDown, egui::Key::PageDown),
                (NavKey::Home, egui::Key::Home),
                (NavKey::End, egui::Key::End),
            ];
            let pressed = ctx.input_mut(|i| {
                nav.into_iter()
                    .find(|&(_, k)| i.consume_key(egui::Modifiers::NONE, k))
            });
            if let (Some((key, _)), Some(view)) = (pressed, self.view.as_ref()) {
                let (scroll, height) = (self.last.scroll_y, self.last.viewport_h);
                self.jump_to = Some(nav_target(key, scroll, height, view.layout.total_height()));
            }
        }
        let zoom_delta = ctx.input(|i| i.zoom_delta()); // Ctrl+roda e gesto de pinça
        if let (true, Some(view)) = ((zoom_delta - 1.0).abs() > f32::EPSILON, self.view.as_mut()) {
            let ratio = view.set_zoom(view.zoom * zoom_delta);
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
        let (redo, undo, save) = ctx.input_mut(|i| {
            (
                !typing
                    && i.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::Z,
                    ),
                !typing && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
            )
        });
        if redo {
            self.edit.redo();
        } else if undo {
            self.edit.undo();
        }
        if let (true, Some(view)) = (save, self.view.as_ref()) {
            self.edit.save(&self.worker, view.path.clone());
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
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
        self.password_window(ui);
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
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
            let can_edit = !self.edit.saving;
            if ui
                .add_enabled(
                    can_edit && self.edit.pending.is_dirty(),
                    egui::Button::new("↶"),
                )
                .clicked()
            {
                self.edit.undo();
            }
            if ui.add_enabled(can_edit, egui::Button::new("↷")).clicked() {
                self.edit.redo();
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
            let count = view.layout.page_count().max(1);
            let mut page = self.last.current_page + 1;
            let go = ui.add(egui::DragValue::new(&mut page).range(1..=count).speed(0.2));
            ui.label(format!("/ {count}"));
            if go.changed() {
                if let Some(y) = view.scroll_target(page as i32 - 1, 0.0) {
                    self.jump_to = Some(y);
                }
            }
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
