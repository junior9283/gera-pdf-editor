use crate::bench::Bench;
use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::search::SearchState;
use crate::sidebar::{self, SidebarAction};
use crate::view::DocView;
use app_state::layout::ViewMode;
use app_state::nav::{nav_target, NavKey};
use app_state::prefs::LeftPanel;
use app_state::transform::PageMap;
use app_state::zoom::{self, Fit};
use pdf_core::{AnnotInfo, DocSummary, OutlineItem, PdfError, Request, Response, Worker};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::TryRecvError;

/// Numeração das abas (identidade na interface), única durante a sessão.
static NEXT_TAB: AtomicU64 = AtomicU64::new(0);

/// Texturas novas por quadro (cada uma custa conversão e envio à GPU na thread da UI).
const MAX_RENDERS_PER_FRAME: usize = 2;

/// Ações sobre o documento, vindas de botões, menus ou atalhos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DocCommand {
    Save,
    Undo,
    Redo,
    /// Leva o foco ao campo de busca.
    Find,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fit(Fit),
    Mode(ViewMode),
    /// Duas páginas: liga/desliga a capa sozinha.
    ToggleCover,
    /// Copia o texto selecionado.
    Copy,
    /// Seleciona o texto do documento inteiro.
    SelectAll,
    Rotate {
        clockwise: bool,
    },
}

/// O que aconteceu com o documento e interessa à casca (abas e recentes).
#[derive(Debug, PartialEq)]
pub enum DocEvent {
    Opened,
    /// Não abriu: a aba deve ser fechada.
    Failed(String),
    /// "Salvar como" concluído no novo caminho.
    Renamed(PathBuf),
}

/// Um documento aberto: worker próprio, vista, busca, edição e painel lateral.
pub struct DocTab {
    pub(crate) worker: Worker,
    /// Identidade estável da aba na interface (rolagem do canvas e das listas laterais).
    id: egui::Id,
    /// Identidade da aba (caminho canônico).
    pub(crate) path: PathBuf,
    /// Tema escuro herdado da casca.
    dark: bool,
    /// Eventos ainda não lidos pela casca.
    events: Vec<DocEvent>,
    /// "Salvar como" em andamento: novo caminho.
    save_dest: Option<PathBuf>,
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
    outline: Vec<OutlineItem>,
    /// Caminho que está sendo aberto (para pedir a senha de um PDF protegido).
    opening: Option<PathBuf>,
    /// Pedido de senha: (caminho, senha digitada).
    password_prompt: Option<(PathBuf, String)>,
    /// Linha de ferramentas de comentário visível ("✏ Comentar").
    pub(crate) comment_tools: bool,
    /// Pedido de foco no campo de busca (Ctrl+F ou menu).
    pub(crate) focus_search: bool,
    /// Copiar pedido por atalho ou menu (feito no próximo quadro, que tem o `egui::Context`).
    copy_requested: bool,
    /// Última virada de página pela roda (tempo do egui), para não virar várias de uma vez.
    turned_at: f64,
}

impl DocTab {
    pub fn new(ctx: &egui::Context, path: PathBuf, dark: bool) -> Self {
        let ctx = ctx.clone();
        Self {
            worker: Worker::spawn(move || ctx.request_repaint()),
            id: egui::Id::new(("documento", NEXT_TAB.fetch_add(1, Ordering::Relaxed))),
            path,
            dark,
            events: Vec::new(),
            save_dest: None,
            view: None,
            status: String::new(),
            last: CanvasFrame::default(),
            jump_to: None,
            search: SearchState::default(),
            edit: EditState::default(),
            saved_annots: Vec::new(),
            engine_stopped: false,
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
            comment_tools: false,
            focus_search: false,
            copy_requested: false,
            turned_at: f64::NEG_INFINITY,
        }
    }

    /// Há anotações ainda não gravadas no PDF?
    pub fn is_dirty(&self) -> bool {
        self.edit.pending.is_dirty()
    }

    /// Fechar esta aba precisa de confirmação (pendências ou salvamento em andamento)?
    pub fn needs_confirm(&self) -> bool {
        self.is_dirty() || self.edit.saving
    }

    pub fn run(&mut self, cmd: DocCommand) {
        let page = self.last.current_page;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        match cmd {
            DocCommand::Save => self.save(),
            DocCommand::Undo => {
                self.edit.undo();
            }
            DocCommand::Redo => {
                self.edit.redo();
            }
            DocCommand::Find => self.focus_search = true,
            DocCommand::Copy => self.copy_requested = true,
            DocCommand::SelectAll => self.edit.select_all(&self.worker),
            DocCommand::ZoomIn | DocCommand::ZoomOut | DocCommand::ZoomReset => {
                let z = match cmd {
                    DocCommand::ZoomIn => zoom::zoom_in(view.zoom),
                    DocCommand::ZoomOut => zoom::zoom_out(view.zoom),
                    _ => zoom::DEFAULT_ZOOM,
                };
                self.zoom_to(z);
            }
            DocCommand::Fit(fit) => view.fit = fit,
            DocCommand::Mode(mode) => {
                view.set_mode(mode, page);
                self.jump_to = view.go_to(page as i32, None);
            }
            DocCommand::ToggleCover => {
                view.cover = !view.cover;
                let mode = view.mode;
                self.run(DocCommand::Mode(mode));
            }
            DocCommand::Rotate { clockwise } => {
                let r = if clockwise {
                    view.rotation.cw()
                } else {
                    view.rotation.ccw()
                };
                view.set_rotation(r);
                self.jump_to = view.go_to(page as i32, None);
            }
        }
    }

    /// Zoom escolhido pelo usuário, mantendo o mesmo ponto do documento na tela.
    fn zoom_to(&mut self, z: f32) {
        if let Some(view) = self.view.as_mut() {
            let ratio = view.set_manual_zoom(z);
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
    }

    pub fn save(&mut self) {
        if let Some(view) = self.view.as_ref() {
            self.edit.save(&self.worker, view.path.clone());
        }
    }

    /// A aba saiu da frente: libera texturas e renders pendentes (só tamanhos, zoom, posição e
    /// anotações ficam). Ao voltar, as páginas visíveis são pedidas de novo.
    pub fn suspend(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.clear_cache();
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        if let Some(view) = self.view.as_mut() {
            view.dark = dark;
            view.clear_cache();
        }
    }

    pub fn save_as(&mut self, dest: PathBuf) {
        if self.view.is_some() && !self.edit.saving {
            self.save_dest = Some(dest.clone());
            self.edit.save_as(&self.worker, dest);
        }
    }

    pub fn take_events(&mut self) -> Vec<DocEvent> {
        std::mem::take(&mut self.events)
    }

    #[cfg(test)]
    fn scroll_id(&self) -> egui::Id {
        self.id
    }

    pub fn title(&self) -> String {
        app_state::tabs::tab_title(&self.path, 28)
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
        let dark = self.dark;
        let mut view = DocView::new(summary.path, summary.sizes);
        view.dark = dark;
        self.view = Some(view);
        self.search.text.clear(); // descarta a busca (e os destaques) do documento anterior
        self.search.start(&self.worker);
        self.last = CanvasFrame::default();
        self.jump_to = Some(0.0);
        self.events.push(DocEvent::Opened);
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
                Response::OpenFailed(e) => {
                    self.status = format!("Erro: {e}");
                    if self.view.is_none() {
                        let msg = format!("Não foi possível abrir {}: {e}", self.path.display());
                        self.events.push(DocEvent::Failed(msg));
                    }
                }
                Response::Rendered {
                    page,
                    scale,
                    invert,
                    rotation,
                    image,
                } => {
                    let mut is_page = false;
                    if let Some(view) = self.view.as_mut() {
                        view.on_rendered(ctx, page, scale, invert, rotation, image);
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
                        first.and_then(|(p, r)| self.view.as_mut()?.go_to(p, Some((r.x0, r.y0))))
                    {
                        self.jump_to = Some(y);
                    }
                }
                Response::SearchDone { id } => self.search.on_done(id),
                Response::Words { .. } => {} // a interface não pede mais palavras soltas
                Response::Selection {
                    tag,
                    page,
                    selection,
                } => self.edit.on_selection(tag, page, selection),
                Response::DocumentText { tag, text } => self.edit.on_document_text(tag, text),
                Response::Saved { annots } => {
                    self.edit.reset();
                    self.saved_annots = annots;
                    if let Some(view) = self.view.as_mut() {
                        view.clear_cache(); // as páginas precisam ser renderizadas de novo, agora com as anotações
                    }
                    self.status = "Salvo.".into();
                    if let Some(dest) = self.save_dest.take() {
                        if let Some(view) = self.view.as_mut() {
                            view.path = dest.clone();
                        }
                        self.events.push(DocEvent::Renamed(dest));
                    }
                }
                Response::SaveFailed(msg) => {
                    self.edit.saving = false;
                    self.save_dest = None;
                    self.status = format!("Não foi possível salvar: {msg}");
                }
                // ligados à interface nas próximas tarefas (seleção, propriedades, impressão)
                Response::Properties(_)
                | Response::PropertiesFailed(_)
                | Response::Exported { .. }
                | Response::ExportFailed(_) => {}
            }
        }
    }

    /// Teclado e zoom do documento: navegação (fora de campos de texto) e Ctrl+roda. Os demais
    /// atalhos vêm da tabela de comandos da casca.
    pub fn handle_input(&mut self, ctx: &egui::Context) {
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
            if let (Some((key, _)), Some(view)) = (pressed, self.view.as_mut()) {
                if view.mode.is_paged() {
                    match key {
                        NavKey::PageUp => {
                            view.turn(-1);
                        }
                        NavKey::PageDown => {
                            view.turn(1);
                        }
                        NavKey::Home => view.turn_to(0),
                        NavKey::End => view.turn_to(usize::MAX),
                    }
                    self.jump_to = Some(0.0);
                } else {
                    let (scroll, height) = (self.last.scroll_y, self.last.viewport_h);
                    self.jump_to =
                        Some(nav_target(key, scroll, height, view.layout.total_height()));
                }
            }
            // setas e espaço viram a página nos modos página a página
            if let Some(view) = self.view.as_mut().filter(|v| v.mode.is_paged()) {
                let step = ctx.input_mut(|i| {
                    [
                        (egui::Key::ArrowRight, 1),
                        (egui::Key::Space, 1),
                        (egui::Key::ArrowLeft, -1),
                    ]
                    .into_iter()
                    .find(|&(k, _)| i.consume_key(egui::Modifiers::NONE, k))
                    .map(|(_, s)| s)
                });
                if step.is_some_and(|s| view.turn(s)) {
                    self.jump_to = Some(0.0);
                }
            }
        }
        let zoom_delta = ctx.input(|i| i.zoom_delta()); // Ctrl+roda e gesto de pinça
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            if let Some(z) = self.view.as_ref().map(|v| v.zoom * zoom_delta) {
                self.zoom_to(z);
            }
        }
    }

    /// Início do quadro: tem que vir antes de qualquer pedido de render (painel ou canvas).
    pub fn begin(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.begin_frame();
        }
    }

    /// Barra de status (some na tela cheia).
    /// Barra de status; `shell` é a mensagem da casca (erros do seletor, de abertura...).
    pub fn show_status(&mut self, ui: &mut egui::Ui, shell: &str) {
        let unsaved = self.edit.pending.items().len();
        let text = status_line(shell, &self.status, unsaved);
        egui::Panel::bottom("status").show(ui, |ui| ui.label(text));
    }

    /// Páginas, janelas de nota e senha e fim do quadro (depois dos painéis laterais).
    pub fn show_canvas(&mut self, ui: &mut egui::Ui) {
        if std::mem::take(&mut self.copy_requested) {
            self.edit.copy(ui.ctx());
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.edit.note_window(ui);
        self.password_window(ui);
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
        }
    }

    /// Conteúdo do painel esquerdo; clicar leva à página.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, panel: LeftPanel) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        // cada aba com seu id: as listas guardam a própria rolagem
        let action = ui
            .push_id(self.id, |ui| {
                sidebar::show(
                    ui,
                    panel,
                    view,
                    &self.worker,
                    &self.outline,
                    &self.saved_annots,
                    self.edit.pending.items(),
                )
            })
            .inner;
        if let SidebarAction::JumpToPage(page) = action {
            if let Some(y) = view.go_to(page as i32, None) {
                self.jump_to = Some(y);
            }
        }
    }

    /// ◀ campo da página / total ▶
    pub fn page_controls(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let count = view.layout.page_count().max(1);
        let mut page = self.last.current_page + 1;
        let prev = ui.button("◀").on_hover_text("Página anterior").clicked();
        let go = ui.add(egui::DragValue::new(&mut page).range(1..=count).speed(0.2));
        ui.label(format!("/ {count}"));
        let next = ui.button("▶").on_hover_text("Próxima página").clicked();
        let target = match (prev, next) {
            (true, _) => Some(page.saturating_sub(1).max(1)),
            (_, true) => Some((page + 1).min(count)),
            _ => go.changed().then_some(page),
        };
        if let Some(y) = target.and_then(|p| view.go_to(p as i32 - 1, None)) {
            self.jump_to = Some(y);
        }
    }

    /// Campo de busca, resumo e ▲▼ (o foco vem de `DocCommand::Find`).
    pub fn search_box(&mut self, ui: &mut egui::Ui) {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.search.text)
                .hint_text("Buscar…")
                .desired_width(160.0),
        );
        if std::mem::take(&mut self.focus_search) {
            edit.request_focus();
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.search.start(&self.worker);
            edit.request_focus();
        }
        ui.label(self.search.summary());
        let prev = ui.button("⏶").clicked();
        let next = ui.button("⏷").clicked();
        if prev || next {
            let target = self.search.step(next);
            if let Some(y) = target
                .and_then(|(page, rect)| self.view.as_mut()?.go_to(page, Some((rect.x0, rect.y0))))
            {
                self.jump_to = Some(y);
            }
        }
    }

    pub fn zoom_percent(&self) -> Option<f32> {
        self.view.as_ref().map(|v| v.zoom * 100.0)
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
        if let Some(ratio) =
            view.apply_fit(ui.available_width() - 24.0, ui.available_height() - 24.0)
        {
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
        let (worker, edit, search) = (&self.worker, &mut self.edit, &self.search);
        let jump_to = self.jump_to.take();
        // cada aba com seu id: a área de rolagem guarda a posição de cada documento
        self.last = ui
            .push_id(self.id, |ui| {
                canvas::show_pages(
                    ui,
                    view,
                    worker,
                    jump_to,
                    &mut |ui, page, rect, map: &PageMap| {
                        paint_search_hits(ui, search, page as i32, map);
                        edit.on_page(ui, worker, page as i32, rect, map);
                    },
                )
            })
            .inner;
        let now = ui.input(|i| i.time);
        if self.last.turn != 0 && now - self.turned_at > 0.25 && view.turn(self.last.turn) {
            self.turned_at = now;
            self.jump_to = Some(0.0);
        }
    }
}

/// Texto da barra de status: a mensagem da casca (se houver) vem antes da do documento.
fn status_line(shell: &str, doc: &str, unsaved: usize) -> String {
    let mut line = match (shell.is_empty(), doc.is_empty()) {
        (true, _) => doc.to_string(),
        (false, true) => shell.to_string(),
        (false, false) => format!("{shell} · {doc}"),
    };
    if unsaved > 0 {
        line.push_str(&format!(" — {unsaved} alterações não salvas"));
    }
    line
}

/// Realça os resultados da busca na página (o atual em laranja).
fn paint_search_hits(ui: &egui::Ui, search: &SearchState, page: i32, map: &PageMap) {
    for (n, r) in search.hits_on(page) {
        let color = if n == search.current_index() {
            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 230, 0, 90)
        };
        ui.painter().rect_filled(screen_rect(r, map), 2.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tab_has_its_own_scroll_state() {
        let ctx = egui::Context::default();
        let a = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        let b = DocTab::new(&ctx, PathBuf::from("/b.pdf"), false);
        assert_ne!(
            a.scroll_id(),
            b.scroll_id(),
            "rolar uma aba não pode mover a outra"
        );
    }

    #[test]
    fn shell_messages_show_in_the_document_status_bar() {
        assert_eq!(
            status_line("", "doc.pdf — 3 páginas", 0),
            "doc.pdf — 3 páginas"
        );
        assert_eq!(
            status_line("", "doc.pdf", 2),
            "doc.pdf — 2 alterações não salvas"
        );
        let line = status_line("Esse arquivo já está aberto em outra aba.", "doc.pdf", 0);
        assert!(line.starts_with("Esse arquivo já está aberto"), "{line}");
    }
}
