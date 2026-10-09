use crate::bench::Bench;
use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::print_job::{self, PrintJobs};
use crate::search::SearchState;
use crate::shell::print_dialog::{PrintChoice, PrintDialog};
use crate::sidebar::{self, SidebarAction};
use crate::view::DocView;
use app_state::layout::ViewMode;
use app_state::nav::{nav_target, NavKey};
use app_state::prefs::LeftPanel;
use app_state::print::{any_oversized, lp_args, PrintOptions};
use app_state::transform::PageMap;
use app_state::zoom::{self, Fit};
use pdf_core::{
    AnnotInfo, DocProperties, DocSummary, HistoryState, OutlineItem, PdfError, Request, Response,
    Worker,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

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
    /// Apaga a anotação selecionada.
    Delete,
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
    /// O usuário informou o nome no diálogo Identidade.
    Author(String),
}

/// Impressão esperando a cópia temporária: opções, página atual no clique e o arquivo pedido.
struct PendingPrint {
    options: PrintOptions,
    page: usize,
    file: PathBuf,
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
    /// Anotações visíveis do documento em memória (o worker manda a lista a cada mudança).
    pub(crate) annots: Vec<AnnotInfo>,
    /// Estado do histórico de edições no worker.
    history: HistoryState,
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
    /// Propriedades mostradas na janela (aberta enquanto houver).
    properties: Option<DocProperties>,
    /// Diálogo "Imprimir" aberto.
    print_dialog: Option<PrintDialog>,
    /// Impressão esperando a cópia temporária ficar pronta: opções e a página atual no clique.
    print_pending: Option<PendingPrint>,
    /// Lista de impressoras sendo lida do CUPS em segundo plano.
    printers_rx: Option<Receiver<(Vec<String>, Option<String>)>>,
    print_jobs: PrintJobs,
    /// Última virada de página pela roda (tempo do egui), para não virar várias de uma vez.
    turned_at: f64,
    /// Nome sendo digitado no diálogo Identidade.
    identity_name: String,
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
            annots: Vec::new(),
            history: HistoryState::default(),
            engine_stopped: false,
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
            comment_tools: false,
            focus_search: false,
            properties: None,
            print_dialog: None,
            print_pending: None,
            printers_rx: None,
            print_jobs: PrintJobs::default(),
            turned_at: f64::NEG_INFINITY,
            identity_name: String::new(),
        }
    }

    /// Há edições ainda não gravadas no arquivo?
    pub fn is_dirty(&self) -> bool {
        self.history.dirty
    }

    /// Fechar esta aba precisa de confirmação (pendências ou salvamento em andamento)?
    pub fn needs_confirm(&self) -> bool {
        self.is_dirty() || self.edit.saving
    }

    pub fn run(&mut self, ctx: &egui::Context, cmd: DocCommand) {
        let page = self.last.current_page;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        match cmd {
            DocCommand::Save => self.save(),
            DocCommand::Undo if self.history.can_undo => self.worker.send(Request::Undo),
            DocCommand::Redo if self.history.can_redo => self.worker.send(Request::Redo),
            DocCommand::Undo | DocCommand::Redo => {}
            DocCommand::Find => self.focus_search = true,
            DocCommand::Copy => self.edit.copy(ctx),
            DocCommand::SelectAll => self.edit.select_all(&self.worker),
            DocCommand::Delete => self.edit.delete_selected(),
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
                self.run(ctx, DocCommand::Mode(mode));
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

    /// Salvar: só quando há o que gravar.
    pub fn save(&mut self) {
        if let Some(view) = self.view.as_ref().filter(|_| self.history.dirty) {
            self.edit.save(&self.worker, view.path.clone());
        }
    }

    /// Manda ao worker as edições geradas neste quadro.
    fn send_edits(&mut self) {
        for op in self.edit.take_ops() {
            self.worker.send(Request::Annot(op));
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
        if self.view.is_some() && self.edit.save(&self.worker, dest.clone()) {
            self.save_dest = Some(dest);
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
        self.annots = summary.annots;
        self.history = HistoryState::default();
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
        if let Some(msg) = self.print_jobs.poll().pop() {
            self.status = msg;
        }
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
                Response::Selection {
                    tag,
                    page,
                    selection,
                } => self.edit.on_selection(tag, page, selection),
                Response::DocumentText { tag, text } => self.edit.on_document_text(tag, text),
                Response::Saved { annots, history } => {
                    self.edit.saving = false;
                    self.annots = annots;
                    self.history = history;
                    self.status = "Salvo.".into();
                    if let Some(dest) = self.save_dest.take() {
                        if let Some(view) = self.view.as_mut() {
                            view.path = dest.clone();
                        }
                        self.events.push(DocEvent::Renamed(dest));
                    }
                }
                Response::SaveFailed { error, history } => {
                    self.edit.saving = false;
                    self.history = history;
                    self.save_dest = None;
                    self.status = format!("Não foi possível salvar: {error}");
                }
                Response::Properties(p) => {
                    self.properties = Some(p);
                }
                Response::PropertiesFailed(msg) => {
                    self.status = format!("Não foi possível ler as propriedades: {msg}");
                }
                Response::Exported { dest } => self.send_to_printer(ctx, dest),
                Response::ExportFailed(msg) => {
                    self.print_pending = None;
                    self.status = format!("Não foi possível preparar a impressão: {msg}");
                }
                Response::AnnotsChanged {
                    pages,
                    annots,
                    history,
                } => {
                    replace_pages(&mut self.annots, &pages, annots);
                    self.edit.keep_valid_selection(&self.annots);
                    self.history = history;
                    if let Some(view) = self.view.as_mut() {
                        for page in pages {
                            view.invalidate_page(page);
                        }
                    }
                }
                Response::AnnotFailed(msg) => {
                    self.status = format!("Não foi possível alterar a anotação: {msg}");
                }
            }
        }
    }

    /// Teclado e zoom do documento: navegação (fora de campos de texto) e Ctrl+roda. Os demais
    /// atalhos vêm da tabela de comandos da casca.
    pub fn handle_input(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        // Esc volta para Selecionar (como no Acrobat); numa caixa de texto, só termina a digitação
        if !typing
            && self.edit.text_draft.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.edit.tool = app_state::tools::Tool::Select;
            self.edit.shape_drag = None;
        }
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
        let text = status_line(shell, &self.status, self.history.dirty);
        egui::Panel::bottom("status").show(ui, |ui| ui.label(text));
    }

    /// Arquivo → Imprimir: abre o diálogo na hora; as impressoras chegam do CUPS em segundo plano.
    pub fn open_print_dialog(&mut self, ctx: &egui::Context) {
        if self.view.is_some() && self.print_pending.is_none() {
            self.print_dialog = Some(PrintDialog::new(self.last.current_page));
            self.printers_rx = Some(print_job::printers_in_background(ctx));
        }
    }

    /// Opções escolhidas: o worker grava a cópia temporária; o envio continua em `send_to_printer`.
    fn start_print(&mut self, options: PrintOptions) {
        match print_job::temp_file() {
            Ok(dest) => {
                self.worker.send(Request::ExportForPrint {
                    dest: dest.clone(),
                    with_annots: options.with_annots,
                });
                self.status = format!("Preparando a impressão para {}…", options.printer);
                self.print_pending = Some(PendingPrint {
                    options,
                    page: self.last.current_page,
                    file: dest,
                });
            }
            Err(e) => self.status = format!("Não foi possível preparar a impressão: {e}"),
        }
    }

    /// A cópia temporária ficou pronta: monta os argumentos e entrega ao CUPS.
    fn send_to_printer(&mut self, ctx: &egui::Context, file: PathBuf) {
        let (Some(PendingPrint { options, page, .. }), Some(view)) =
            (self.print_pending.take(), self.view.as_ref())
        else {
            let _ = std::fs::remove_file(&file);
            return;
        };
        let oversized = any_oversized(view.sizes.iter().map(|s| (s.width, s.height)));
        match lp_args(&options, page + 1, view.sizes.len(), oversized, &file) {
            Ok(args) => {
                self.status = format!("Enviando para {}…", options.printer);
                self.print_jobs.start(ctx, options.printer, args, file);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&file);
                self.status = format!("Não foi possível imprimir: {e}");
            }
        }
    }

    /// Arquivo → Propriedades: pede ao worker; a janela abre quando a resposta chega.
    pub fn request_properties(&mut self) {
        if self.view.is_some() {
            self.worker.send(Request::Properties);
        }
    }

    /// Páginas, janelas (propriedades, imprimir, nota, senha) e fim do quadro, depois dos painéis.
    pub fn show_canvas(&mut self, ui: &mut egui::Ui) {
        if let Some(p) = &self.properties {
            let mut open = true;
            crate::shell::properties::window(ui, p, &mut open);
            if !open {
                self.properties = None;
            }
        }
        if let (Some(rx), Some(dialog)) = (&self.printers_rx, self.print_dialog.as_mut()) {
            if let Ok((list, default)) = rx.try_recv() {
                dialog.set_printers(list, default);
                self.printers_rx = None;
            }
        }
        let choice = match (self.print_dialog.as_mut(), self.view.as_mut()) {
            (Some(dialog), Some(view)) => dialog.show(ui, view, &self.worker),
            _ => None,
        };
        match choice {
            Some(PrintChoice::Print(options)) => {
                self.print_dialog = None;
                self.start_print(options);
            }
            Some(PrintChoice::Cancel) => self.print_dialog = None,
            None => {}
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        let down = ui.input(|i| i.pointer.any_down());
        self.edit.drop_stale_drag(down);
        self.edit.note_window(ui);
        self.edit.props_window(ui, &self.annots);
        self.identity_window(ui);
        self.send_edits();
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
                sidebar::show(ui, panel, view, &self.worker, &self.outline, &self.annots)
            })
            .inner;
        let target = match action {
            SidebarAction::None => return,
            SidebarAction::JumpToPage(page) => (page as i32, None),
            SidebarAction::SelectAnnot { page, id, at } => {
                self.edit.selected = Some((page, id));
                (page, Some(at))
            }
        };
        if let Some(y) = view.go_to(target.0, target.1) {
            self.jump_to = Some(y);
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
    /// Nome do usuário: vai para os carimbos dinâmicos e, no worker, como autor das anotações.
    pub fn set_author(&mut self, author: Option<String>) {
        self.edit.author = author.clone();
        self.worker.send(Request::SetAuthor(author));
    }

    /// Diálogo Identidade, aberto por um carimbo dinâmico sem nome; o OK põe o carimbo.
    fn identity_window(&mut self, ui: &mut egui::Ui) {
        let Some((page, at)) = self.edit.identity_for else {
            return;
        };
        match crate::shell::identity::window(ui, &mut self.identity_name) {
            Some(true) => {
                let name = self.identity_name.trim().to_owned();
                self.edit.identity_for = None;
                self.set_author(Some(name.clone()));
                self.edit
                    .place_stamp(page, at, Some(&name), crate::create::local_now());
                self.events.push(DocEvent::Author(name));
            }
            Some(false) => self.edit.identity_for = None,
            None => {}
        }
    }

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
        let (worker, edit, search, annots) =
            (&self.worker, &mut self.edit, &self.search, &self.annots);
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
                        edit.on_page(ui, worker, annots, page as i32, rect, map);
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

/// Troca na lista as anotações das `pages` pelas novas, mantendo a ordem por página.
fn replace_pages(all: &mut Vec<AnnotInfo>, pages: &[i32], new: Vec<AnnotInfo>) {
    all.retain(|a| !pages.contains(&a.page));
    all.extend(new);
    all.sort_by_key(|a| a.page); // estável: a ordem dentro da página é a de desenho
}

/// Texto da barra de status: a mensagem da casca (se houver) vem antes da do documento.
fn status_line(shell: &str, doc: &str, unsaved: bool) -> String {
    let mut line = match (shell.is_empty(), doc.is_empty()) {
        (true, _) => doc.to_string(),
        (false, true) => shell.to_string(),
        (false, false) => format!("{shell} · {doc}"),
    };
    if unsaved {
        line.push_str(" — alterações não salvas");
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

impl Drop for DocTab {
    /// Aba fechada com a impressão em preparo: a cópia sem senha não pode ficar para trás
    /// (se o worker ainda a gravar depois disto, a faxina da próxima abertura a remove).
    fn drop(&mut self) {
        if let Some(p) = self.print_pending.take() {
            let _ = std::fs::remove_file(p.file);
        }
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
            status_line("", "doc.pdf — 3 páginas", false),
            "doc.pdf — 3 páginas"
        );
        assert_eq!(
            status_line("", "doc.pdf", true),
            "doc.pdf — alterações não salvas"
        );
        let line = status_line(
            "Esse arquivo já está aberto em outra aba.",
            "doc.pdf",
            false,
        );
        assert!(line.starts_with("Esse arquivo já está aberto"), "{line}");
    }

    #[test]
    fn changed_pages_replace_only_their_annotations() {
        let a = |page, id| AnnotInfo {
            id,
            kind: pdf_core::AnnotKind::Note,
            page,
            rect: pdf_core::PdfRect::new(0.0, 0.0, 1.0, 1.0),
            contents: String::new(),
            color: None,
            opacity: 1.0,
            width: None,
            ..Default::default()
        };
        let mut all = vec![a(0, 1), a(1, 2), a(1, 3), a(2, 4)];
        replace_pages(&mut all, &[1], vec![a(1, 3), a(1, 5)]);
        let ids: Vec<_> = all.iter().map(|x| x.id).collect();
        assert_eq!(ids, [1, 3, 5, 4]);
    }
}
