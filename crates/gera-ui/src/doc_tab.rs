use crate::bench::Bench;
use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::print_job::{self, PrintJobs};
use crate::search::SearchState;
use crate::shell::commands::{self, Command};
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
    /// Resultado do Comparar arquivos: próxima / anterior alteração.
    NextChange,
    PrevChange,
    /// Exibir → Navegação de páginas.
    FirstPage,
    PrevPage,
    NextPage,
    LastPage,
    /// Leva o foco ao campo da página.
    GoToPage,
    /// Zoom para…: leva o foco ao campo de zoom.
    ZoomTo,
    /// Escolhe a ferramenta (Selecionar, Mão, Zoom com marca; teclas únicas).
    Tool(app_state::tools::Tool),
    /// Shift+U: alterna entre Destacar e Sublinhar.
    CycleMarkup,
    /// Histórico de vistas (Alt+← / Alt+→).
    ViewBack,
    ViewForward,
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
    /// Abrir este painel à esquerda (ex.: a barra azul pede o painel Assinaturas).
    ShowPanel(LeftPanel),
    /// Abrir este arquivo numa aba (ex.: o resultado do Comprimir PDF).
    OpenFile(PathBuf),
    /// Documento criado pelo app (sem lugar escolhido): Salvar pede o destino.
    NeedSaveAs,
    /// Clique num link para fora do documento (endereço, outro arquivo ou bloqueado).
    OpenLink(pdf_core::LinkTarget),
    /// O arquivo pede para abrir em tela cheia (`/PageMode /FullScreen`).
    FullScreen,
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
    /// Como abrir: padrão das Preferências e a última vista do arquivo (dados pela casca).
    pub(crate) open_view: OpenView,
    /// Mão temporária enquanto o Espaço está segurado.
    temp_tool: app_state::tools::TempTool,
    /// Zoom com marca: a página e o ponto (tela) onde o arraste começou.
    marquee: Option<(usize, egui::Pos2)>,
    /// Página e ponto do último salto ainda não mostrado (refeito se o zoom mudar antes).
    pending_target: Option<(i32, Option<(f32, f32)>)>,
    /// Rolagem horizontal pedida (Ajustar ao conteúdo visível).
    jump_x: Option<f32>,
    /// Zoom para… (Ctrl+Y): o campo de zoom pede o foco; `zoom_text` é o que está sendo digitado.
    focus_zoom: bool,
    zoom_text: Option<String>,
    /// Vista anterior e seguinte (Alt+← / Alt+→).
    history_views: app_state::view_history::ViewHistory,
    /// Ir para página: o campo da página pede o foco no próximo quadro.
    focus_page: bool,
    /// Links de todas as páginas.
    links: Vec<pdf_core::LinkInfo>,
    /// Página a mostrar assim que o documento abrir (link de outro PDF).
    pending_page: Option<i32>,
    /// Tela cheia: modo, capa, ajuste e zoom de antes, para voltar a eles ao sair.
    presentation: Option<(ViewMode, bool, Fit, f32)>,
    pub(crate) search: SearchState,
    pub(crate) edit: EditState,
    /// Anotações visíveis do documento em memória (o worker manda a lista a cada mudança).
    pub(crate) annots: Vec<AnnotInfo>,
    /// Campos de formulário de todas as páginas.
    pub(crate) fields: Vec<pdf_core::FieldInfo>,
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
    /// Linha Preencher e assinar aberta.
    pub(crate) fill_tools: bool,
    /// Linha Preparar formulário aberta (os campos viram objetos editáveis).
    pub(crate) prepare_tools: bool,
    pub(crate) prepare: crate::prepare::PrepareState,
    /// Linha Editar PDF aberta (as imagens viram objetos editáveis).
    pub(crate) edit_pdf_tools: bool,
    pub(crate) edit_pdf: crate::edit_pdf::EditPdfState,
    /// Marca d'água / Cabeçalho e rodapé.
    pub(crate) pagination: crate::pagination_tool::PaginationTool,
    /// Linha Certificados aberta.
    pub(crate) cert_tools: bool,
    pub(crate) cert: crate::certificates::CertState,
    /// Proteção e permissões do documento (de quem abriu).
    pub(crate) security: pdf_core::SecurityInfo,
    /// Linha Proteger PDF aberta.
    pub(crate) protect_tools: bool,
    /// Linha Comparação aberta (resultado do Comparar arquivos).
    pub(crate) compare_tools: bool,
    /// Ferramenta Redigir (aberta ou não) e o estado dela.
    pub(crate) redact: crate::redact_tool::RedactTool,
    pub(crate) protect: Option<crate::shell::protect_dialog::ProtectDialog>,
    /// Pedido da senha de permissões (Desbloquear).
    pub(crate) unlock: Option<String>,
    /// Janela Comprimir PDF aberta: nível escolhido (0 baixa, 1 média, 2 alta) e se está
    /// comprimindo.
    pub(crate) compress: Option<(usize, bool)>,
    /// Janela Exportar PDF: formato (0 Word, 1 texto, 2 PNG, 3 JPEG), resolução e se está
    /// exportando.
    pub(crate) export: Option<(usize, u32, bool)>,
    /// Janela Reconhecer texto (OCR).
    pub(crate) ocr: Option<OcrWindow>,
    /// Criado pelo app (Criar PDF): ainda sem lugar escolhido, conta como não salvo.
    pub(crate) created: bool,
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
    /// Painel Comentários (busca, filtros, ordenação e resposta em digitação).
    comments: crate::comments_panel::PanelState,
    /// Preencher formulários: realce, campo em edição, lista aberta.
    form: crate::fields::FormState,
    /// Diálogo Dividir documento aberto.
    split: Option<SplitDialog>,
    /// Modo Organizar páginas (a grade substitui o documento).
    pub(crate) organize: Option<crate::organize::OrganizeState>,
    /// Seletor de arquivos do modo Organizar e para que serve a escolha em andamento.
    files: crate::open_dialog::FileDialogs,
    picking: Option<PickFor>,
    /// "Excluir páginas após extrair": esperando a extração terminar.
    delete_after_extract: Option<Vec<i32>>,
}

/// O que fazer com o arquivo escolhido no seletor (modo Organizar).
enum PickFor {
    File(crate::organize::Purpose),
    /// Editar PDF: imagem para substituir a selecionada (ou, sem nome, para acrescentar).
    Image(Option<(i32, String)>),
    /// Pasta das partes do Dividir.
    Folder,
    /// Arquivo (imagem ou PDF) da marca d'água.
    WatermarkFile,
    Extract {
        pages: Vec<i32>,
        separate: bool,
        delete_after: bool,
    },
}

/// Diálogo Dividir documento.
struct SplitDialog {
    by: SplitBy,
    pages: u32,
    mb: f32,
    dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitBy {
    Pages,
    Size,
    Bookmarks,
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
            jump_x: None,
            pending_target: None,
            temp_tool: Default::default(),
            open_view: OpenView::default(),
            marquee: None,
            focus_zoom: false,
            zoom_text: None,
            history_views: Default::default(),
            focus_page: false,
            links: Vec::new(),
            pending_page: None,
            presentation: None,
            search: SearchState::default(),
            edit: EditState::default(),
            annots: Vec::new(),
            fields: Vec::new(),
            history: HistoryState::default(),
            engine_stopped: false,
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
            comment_tools: false,
            fill_tools: false,
            prepare_tools: false,
            prepare: Default::default(),
            edit_pdf_tools: false,
            edit_pdf: Default::default(),
            pagination: Default::default(),
            cert_tools: false,
            cert: Default::default(),
            security: Default::default(),
            protect_tools: false,
            compare_tools: false,
            redact: Default::default(),
            protect: None,
            unlock: None,
            compress: None,
            export: None,
            created: false,
            ocr: None,
            focus_search: false,
            properties: None,
            print_dialog: None,
            print_pending: None,
            printers_rx: None,
            print_jobs: PrintJobs::default(),
            turned_at: f64::NEG_INFINITY,
            identity_name: String::new(),
            comments: Default::default(),
            form: Default::default(),
            organize: None,
            split: None,
            files: Default::default(),
            picking: None,
            delete_after_extract: None,
        }
    }

    /// Há edições ainda não gravadas no arquivo?
    pub fn is_dirty(&self) -> bool {
        self.history.dirty || self.created
    }

    /// Fechar esta aba precisa de confirmação (pendências ou salvamento em andamento)?
    pub fn needs_confirm(&self) -> bool {
        self.is_dirty()
            || self.edit.saving
            || self.form.has_draft()
            || self.ocr.as_ref().is_some_and(|w| w.cancel.is_some())
    }

    /// Confirma o texto que está sendo digitado num campo (antes de salvar ou imprimir).
    /// Devolve se havia o que gravar.
    /// Confirma o que está sendo digitado (campo de formulário, texto do Editar PDF) antes de gravar,
    /// imprimir ou desfazer: o worker atende em ordem. Devolve se havia o que confirmar.
    fn commit_field(&mut self) -> bool {
        let field = match self.form.commit() {
            Some(crate::fields::FieldAction::Set { page, id, value }) => {
                self.worker.send(Request::SetField { page, id, value });
                true
            }
            _ => false,
        };
        let text = self.edit_pdf.commit().map(|action| {
            send_edit_action(&self.worker, &mut self.status, action);
        });
        field || text.is_some()
    }

    pub fn run(&mut self, ctx: &egui::Context, cmd: DocCommand) {
        if let DocCommand::NextChange | DocCommand::PrevChange = cmd {
            if self.compare_tools {
                self.step_change(cmd == DocCommand::NextChange);
            }
            return;
        }
        if matches!(cmd, DocCommand::Undo | DocCommand::Redo) && self.commit_field() {
            // o texto em edição entra no histórico; desfazer o tira (como no Acrobat)
            if cmd == DocCommand::Undo {
                self.worker.send(Request::Undo);
            }
            return;
        }
        if self.navigate(cmd) {
            return;
        }
        let page = self.last.current_page;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        // no modo Organizar, Excluir e Selecionar tudo agem sobre as páginas
        if let Some(org) = self.organize.as_mut() {
            match cmd {
                DocCommand::Delete => {
                    if let Some(crate::organize::OrgAction::Op(op)) = org.delete(view.sizes.len()) {
                        self.worker.send(Request::Pages(op));
                    }
                    return;
                }
                DocCommand::SelectAll => {
                    org.sel
                        .pick(app_state::organize::Picker::All, &view.page_dims());
                    return;
                }
                _ => {}
            }
        }
        match cmd {
            // PDF criado pelo app: o primeiro Salvar escolhe onde guardar (como o Acrobat)
            DocCommand::Save if self.created => self.events.push(DocEvent::NeedSaveAs),
            DocCommand::Save => self.save(),
            DocCommand::Undo if self.history.can_undo => self.worker.send(Request::Undo),
            DocCommand::Redo if self.history.can_redo => self.worker.send(Request::Redo),
            DocCommand::Undo | DocCommand::Redo => {}
            DocCommand::Find => self.focus_search = true,
            DocCommand::Copy if !self.security.permissions.copy => self.deny("copiar"),
            DocCommand::Copy => self.edit.copy(ctx),
            DocCommand::SelectAll => self.edit.select_all(&self.worker),
            DocCommand::Delete if self.prepare_tools => {
                if let Some((page, id)) = self.prepare.selected.take() {
                    self.worker
                        .send(Request::Prepare(pdf_core::PrepareOp::Delete { page, id }));
                }
            }
            DocCommand::Delete if self.edit_pdf_tools => {
                if let Some(action) = self.edit_pdf.delete_selected() {
                    send_edit_action(&self.worker, &mut self.status, action);
                }
            }
            DocCommand::Delete => self.edit.delete_selected(),
            DocCommand::ZoomIn | DocCommand::ZoomOut | DocCommand::ZoomReset => {
                let z = match cmd {
                    DocCommand::ZoomIn => zoom::zoom_in(view.zoom),
                    DocCommand::ZoomOut => zoom::zoom_out(view.zoom),
                    _ => zoom::DEFAULT_ZOOM,
                };
                self.zoom_to(z);
            }
            DocCommand::Fit(Fit::Visible) => {
                view.fit = Fit::Visible;
                view.visible_box = None;
                self.worker.send(Request::ContentBox { page: page as i32 });
            }
            DocCommand::Fit(fit) => view.fit = fit,
            DocCommand::ZoomTo => self.focus_zoom = true,
            DocCommand::Tool(tool) => self.pick_tool(tool),
            DocCommand::CycleMarkup => {
                use app_state::tools::Tool;
                let next = if self.edit.tool == Tool::Highlight {
                    Tool::Underline
                } else {
                    Tool::Highlight
                };
                self.pick_tool(next);
            }
            DocCommand::NextChange
            | DocCommand::PrevChange
            | DocCommand::FirstPage
            | DocCommand::PrevPage
            | DocCommand::NextPage
            | DocCommand::LastPage
            | DocCommand::GoToPage
            | DocCommand::ViewBack
            | DocCommand::ViewForward => {} // já tratados no começo
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

    /// Navegação de páginas e histórico de vistas; `false` se `cmd` não é de navegação.
    fn navigate(&mut self, cmd: DocCommand) -> bool {
        let page = self.last.current_page;
        let Some(view) = self.view.as_ref() else {
            return matches!(cmd, DocCommand::GoToPage);
        };
        let last = view.layout.page_count().saturating_sub(1);
        match cmd {
            DocCommand::PrevPage | DocCommand::NextPage => {
                let step = if cmd == DocCommand::NextPage { 1 } else { -1 };
                if let Some(target) = view.page_step(page, step) {
                    self.go(target as i32, None, false); // virar página não é salto
                }
            }
            DocCommand::FirstPage => self.jump(0, None),
            DocCommand::LastPage => self.jump(last as i32, None),
            DocCommand::GoToPage => self.focus_page = true,
            DocCommand::ViewBack | DocCommand::ViewForward => {
                let current = view.view_entry(self.last.scroll_y, page);
                let to = if cmd == DocCommand::ViewBack {
                    self.history_views.back(current)
                } else {
                    self.history_views.forward(current)
                };
                if let Some(y) = to.and_then(|e| self.view.as_mut()?.restore(&e)) {
                    self.jump_to = Some(y);
                }
            }
            _ => return false,
        }
        true
    }

    /// Escolhe a ferramenta; as de comentário abrem a linha Comentar (como no Acrobat).
    fn pick_tool(&mut self, tool: app_state::tools::Tool) {
        if !tool.is_screen_tool() && !self.comment_tools {
            self.show_row(app_state::tools_catalog::COMMENT);
            if !self.comment_tools {
                return; // sem permissão de comentar (o aviso já saiu)
            }
        }
        self.edit.tool = tool;
    }

    /// Vista de abertura: o padrão das Preferências, depois a visualização inicial do arquivo, e
    /// por cima a última vista lembrada do arquivo.
    pub(crate) fn apply_open_view(&mut self, initial: pdf_core::InitialView) {
        use app_state::prefs::DefaultZoom;
        let open = self.open_view;
        if let Some(view) = self.view.as_mut() {
            if let Some(mode) = open.mode {
                view.set_mode(mode, 0);
            }
            match open.zoom {
                DefaultZoom::Auto => {}
                DefaultZoom::FitPage => view.fit = Fit::Page,
                DefaultZoom::FitWidth => view.fit = Fit::Width,
                DefaultZoom::FitHeight => view.fit = Fit::Height,
                DefaultZoom::Level(z) => {
                    view.set_manual_zoom(z);
                }
            }
        }
        self.apply_initial_view(initial);
        if let Some(last) = open.last {
            if let Some(y) = self.view.as_mut().and_then(|v| v.restore(&last)) {
                self.pending_target = None;
                self.jump_to = Some(y);
            }
        }
    }

    /// A vista atual (para lembrar ao fechar).
    pub(crate) fn current_view(&self) -> Option<app_state::view_history::ViewEntry> {
        let view = self.view.as_ref()?;
        Some(view.view_entry(self.last.scroll_y, self.last.current_page))
    }

    /// Visualização inicial pedida pelo arquivo: modo, zoom, página, painel e tela cheia.
    pub(crate) fn apply_initial_view(&mut self, initial: pdf_core::InitialView) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let page = initial
            .page
            .and_then(|p| usize::try_from(p).ok())
            .filter(|&p| p < view.layout.page_count());
        if let Some((mode, cover)) = initial.layout.map(app_state::layout::initial_layout) {
            view.cover = cover;
            view.set_mode(mode, page.unwrap_or(0));
        }
        if let Some((fit, factor)) = initial.zoom.map(zoom::initial_zoom) {
            view.fit = fit;
            if let Some(z) = factor {
                view.set_zoom(z);
            }
        }
        if let Some(p) = page {
            self.go(p as i32, None, false);
        }
        match initial.panel {
            Some(pdf_core::InitialPanel::Bookmarks) => {
                self.events.push(DocEvent::ShowPanel(LeftPanel::Outline))
            }
            Some(pdf_core::InitialPanel::Thumbnails) => {
                self.events.push(DocEvent::ShowPanel(LeftPanel::Thumbnails))
            }
            None => {}
        }
        if initial.full_screen {
            self.events.push(DocEvent::FullScreen);
        }
    }

    /// Tela cheia: uma página por vez, inteira na tela (o modo de antes volta ao sair).
    pub(crate) fn enter_presentation(&mut self) {
        let page = self.last.current_page;
        let Some(view) = self.view.as_mut() else {
            return;
        };
        self.presentation
            .get_or_insert((view.mode, view.cover, view.fit, view.zoom));
        view.set_mode(ViewMode::Single, page);
        view.fit = Fit::Page;
        self.jump_to = Some(0.0);
    }

    pub(crate) fn exit_presentation(&mut self) {
        let page = self.last.current_page;
        let (Some((mode, cover, fit, zoom)), Some(view)) =
            (self.presentation.take(), self.view.as_mut())
        else {
            return;
        };
        view.cover = cover;
        view.set_mode(mode, page);
        view.fit = fit;
        view.set_zoom(zoom);
        self.jump_to = view.go_to(page as i32, None);
    }

    /// Vai à página `page` agora, ou assim que o documento abrir.
    pub(crate) fn show_page(&mut self, page: i32) {
        if self.view.is_some() {
            self.jump(page, None);
        } else {
            self.pending_page = Some(page);
        }
    }

    /// Salto (link, marcador, miniatura, busca, comentário, Ir para página…): guarda a vista atual
    /// para a Vista anterior e vai à página `page`, no ponto `at` dela.
    pub(crate) fn jump(&mut self, page: i32, at: Option<(f32, f32)>) {
        self.go(page, at, true);
    }

    /// Única porta para mostrar uma página: `record` diz se a vista de onde se sai entra no
    /// histórico (Vista anterior).
    fn go(&mut self, page: i32, at: Option<(f32, f32)>, record: bool) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let current = view.view_entry(self.last.scroll_y, self.last.current_page);
        if let Some(y) = view.go_to(page, at) {
            if record {
                self.history_views.push(current);
            }
            self.jump_to = Some(y);
            self.pending_target = Some((page, at));
        }
    }

    /// O zoom mudou pela razão `ratio` (ajuste à janela): um salto ainda não mostrado é refeito na
    /// mesma página e ponto (o vão entre as páginas não escala); senão a rolagem acompanha o zoom.
    fn after_zoom_change(&mut self, ratio: f32) {
        let target = self.pending_target;
        let redone = target.and_then(|(page, at)| self.view.as_mut()?.go_to(page, at));
        self.jump_to = redone.or(Some(self.last.scroll_y * ratio));
    }

    /// Zoom escolhido pelo usuário, mantendo o mesmo ponto do documento na tela.
    fn zoom_to(&mut self, z: f32) {
        if let Some(view) = self.view.as_mut() {
            let ratio = view.set_manual_zoom(z);
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
    }

    /// Salvar: com marcas de redação não aplicadas, pergunta antes (como o Acrobat).
    pub fn save(&mut self) {
        if self
            .annots
            .iter()
            .any(|a| a.kind == pdf_core::AnnotKind::Redact)
        {
            self.redact.pending_save = true;
            return;
        }
        self.save_now();
    }

    /// Salvar de fato: só quando há o que gravar.
    pub(crate) fn save_now(&mut self) {
        // o worker atende em ordem: o valor do campo chega antes da gravação
        let typed = self.commit_field();
        if let Some(view) = self.view.as_ref().filter(|_| self.history.dirty || typed) {
            self.edit.save(&self.worker, view.path.clone());
        }
    }

    /// Manda ao worker as edições geradas neste quadro.
    fn send_edits(&mut self) {
        let ops = self.edit.take_ops();
        // documento protegido contra comentários: nenhuma alteração de anotação sai daqui
        // (menu de contexto, Delete, arrastos, painel Comentários)
        if !ops.is_empty() && !self.security.permissions.annotate {
            self.deny("alterar comentários");
            return;
        }
        for op in ops {
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
        self.commit_field();
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

    /// O documento todo chegou (aberto ou mudado): anotações, campos e o estado das ferramentas.
    /// Devolve o caminho e os tamanhos das páginas, para a vista.
    fn apply_summary(
        &mut self,
        summary: DocSummary,
    ) -> (std::path::PathBuf, Vec<pdf_core::PageSize>) {
        self.security = summary.security;
        self.annots = summary.annots;
        self.fields = summary.fields;
        self.prepare.on_fields(&self.fields);
        self.edit_pdf.clear();
        if let Some(r) = self.pagination.refresh() {
            self.worker.send(r);
        }
        self.form.notes = summary.form_notes;
        self.outline = summary.outline;
        self.links = summary.links;
        (summary.path, summary.sizes)
    }

    fn on_opened(&mut self, summary: DocSummary) {
        let comparison = summary.is_comparison;
        let initial = summary.initial_view;
        let (path, sizes) = self.apply_summary(summary);
        self.history = HistoryState::default();
        self.password_prompt = None;
        self.edit.reset();
        self.status = format!("{} — {} páginas", path.display(), sizes.len());
        let dark = self.dark;
        let mut view = DocView::new(path, sizes);
        view.dark = dark;
        self.view = Some(view);
        self.search.text.clear(); // descarta a busca (e os destaques) do documento anterior
        self.search.start(&self.worker);
        self.last = CanvasFrame::default();
        self.jump_to = Some(0.0);
        self.events.push(DocEvent::Opened);
        self.apply_open_view(initial);
        if let Some(page) = self.pending_page.take() {
            self.jump(page, None);
        }
        if comparison {
            self.show_row(app_state::tools_catalog::COMPARE);
        }
    }

    /// As alterações do resultado da comparação, na ordem de leitura.
    pub(crate) fn changes(&self) -> Vec<&AnnotInfo> {
        let marks: Vec<&AnnotInfo> = self
            .annots
            .iter()
            .filter(|a| a.author == pdf_core::COMPARE_AUTHOR && a.reply_to.is_none())
            .collect();
        let spots: Vec<_> = marks
            .iter()
            .map(|a| (a.page, a.rect.y0, a.rect.x0))
            .collect();
        app_state::compare_nav::order(&spots)
            .into_iter()
            .map(|i| marks[i])
            .collect()
    }

    /// Posição da alteração selecionada entre as `changes`.
    pub(crate) fn change_at(&self, changes: &[&AnnotInfo]) -> Option<usize> {
        let sel = self.edit.selected?;
        changes.iter().position(|a| (a.page, a.id) == sel)
    }

    /// Seleciona a próxima (ou a anterior) alteração e leva até ela.
    pub(crate) fn step_change(&mut self, forward: bool) {
        let changes = self.changes();
        let current = self.change_at(&changes);
        let Some(k) = app_state::compare_nav::step(changes.len(), current, forward) else {
            return;
        };
        let (page, id, at) = (
            changes[k].page,
            changes[k].id,
            (changes[k].rect.x0, changes[k].rect.y0),
        );
        self.edit.selected = Some((page, id));
        self.jump(page, Some(at));
    }

    /// O documento mudou de páginas: recarrega tudo, ficando perto da página em que estava.
    fn on_pages_changed(&mut self, summary: DocSummary, history: HistoryState) {
        let (_, sizes) = self.apply_summary(summary);
        self.history = history;
        self.edit.keep_valid_selection(&self.annots);
        let page = self.last.current_page.min(sizes.len().saturating_sub(1));
        // com páginas a mais ou a menos, a seleção antiga marcaria outras páginas
        let same_count = self.view.as_ref().map(|v| v.sizes.len()) == Some(sizes.len());
        if let Some(org) = self.organize.as_mut().filter(|_| !same_count) {
            org.sel = Default::default();
        }
        if let Some(view) = self.view.as_mut() {
            view.set_pages(sizes);
            self.jump_to = view.go_to(page as i32, None);
        }
        self.search.start(&self.worker); // os resultados antigos apontam para páginas velhas
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
                Response::Opened(summary) => {
                    self.on_opened(summary);
                    self.worker.send(Request::Signatures);
                }
                Response::Signed { summary, history } => {
                    self.on_pages_changed(summary, history);
                    self.status = "Documento assinado.".into();
                    if let Some(view) = self.view.as_mut() {
                        view.clear_cache();
                    }
                    self.worker.send(Request::Signatures);
                }
                Response::SignFailed(e) => self.status = format!("Não foi possível assinar: {e}"),
                Response::SecurityChanged {
                    security,
                    pending,
                    history,
                } => {
                    let was = std::mem::replace(&mut self.security, security);
                    self.history = history;
                    if pending {
                        self.status =
                            "A proteção será aplicada quando o documento for salvo.".into();
                    } else if security.unlocked && was.encrypted && !was.unlocked {
                        self.status =
                            "Documento desbloqueado: todas as permissões liberadas.".into();
                    }
                }
                Response::SecurityFailed(e) => self.status = format!("Proteger PDF: {e}"),
                Response::Compressed { dest, report } => {
                    self.compress = None;
                    let saved = report.before.saturating_sub(report.after) as f64;
                    let pct = 100.0 * saved / report.before.max(1) as f64;
                    self.status = format!(
                        "Comprimido: de {} para {} (−{pct:.0}%), em {}.",
                        crate::shell::combine::size_text(report.before),
                        crate::shell::combine::size_text(report.after),
                        dest.file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    );
                    self.events.push(DocEvent::OpenFile(dest));
                }
                Response::OcrProgress { done, total } => {
                    if let Some(w) = self.ocr.as_mut() {
                        w.progress = Some((done, total));
                    }
                }
                Response::OcrDone { report, history } => {
                    self.ocr = None;
                    self.history = history;
                    if let Some(view) = self.view.as_mut() {
                        view.clear_cache();
                    }
                    self.status = format!(
                        "Texto reconhecido em {} página(s) ({} palavras){}.",
                        report.recognized,
                        report.words,
                        if report.skipped > 0 {
                            format!("; {} já tinham texto", report.skipped)
                        } else {
                            String::new()
                        }
                    );
                }
                Response::OcrFailed(e) => {
                    self.ocr = None;
                    self.status = format!("Não foi possível reconhecer o texto: {e}");
                }
                Response::ExportedTo(files) => {
                    self.export = None;
                    self.status = match files.as_slice() {
                        [one] => format!("Exportado: {}", one.display()),
                        [first, ..] => format!(
                            "{} imagens exportadas em {}",
                            files.len(),
                            first
                                .parent()
                                .map_or_else(String::new, |p| p.display().to_string())
                        ),
                        [] => "Nada exportado.".into(),
                    };
                }
                Response::ExportToFailed(e) => {
                    self.export = None;
                    self.status = format!("Não foi possível exportar: {e}");
                }
                Response::CompressFailed(e) => {
                    self.compress = None;
                    self.status = format!("Não foi possível comprimir: {e}");
                }
                Response::Signatures(list) => self.cert.signatures = list,
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
                Response::Tile {
                    page,
                    scale,
                    invert,
                    rotation,
                    tile,
                    image,
                } => {
                    if let Some(view) = self.view.as_mut() {
                        view.on_tile(ctx, page, scale, invert, rotation, tile, image);
                    }
                }
                Response::ContentBox { page, rect } => {
                    if let Some(view) = self.view.as_mut().filter(|v| v.fit == Fit::Visible) {
                        // página em branco: a página inteira
                        let full = view.sizes.get(page as usize).map(|s| pdf_core::PdfRect {
                            x0: 0.0,
                            y0: 0.0,
                            x1: s.width,
                            y1: s.height,
                        });
                        view.visible_box = rect.or(full).map(|r| (page as usize, r));
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
                    if let Some((p, r)) = self.search.current().filter(|_| !had_hits) {
                        self.jump(p, Some((r.x0, r.y0)));
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
                    // depois de salvar, "alterado depois de assinado" muda
                    if !self.cert.signatures.is_empty() {
                        self.worker.send(Request::Signatures);
                    }
                    // a proteção pendente passou a valer
                    self.worker.send(Request::Security);
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
                    self.edit_pdf.invalidate(&pages);
                    self.history = history;
                    if let Some(view) = self.view.as_mut() {
                        for page in pages {
                            view.invalidate_page(page);
                        }
                    }
                }
                Response::PagesChanged { summary, history } => {
                    self.on_pages_changed(summary, history)
                }
                Response::RedactionsApplied { result } => match result {
                    Ok((summary, history)) => {
                        self.on_pages_changed(summary, history);
                        self.on_redaction_outcome(true);
                    }
                    Err(msg) => {
                        self.status = format!("Não foi possível aplicar a redação: {msg}");
                        self.on_redaction_outcome(false);
                    }
                },
                Response::PagesFailed(msg) => {
                    self.delete_after_extract = None;
                    self.status = format!("Não foi possível organizar as páginas: {msg}");
                }
                Response::Split(files) => {
                    self.status = match (files.len(), files.first().and_then(|f| f.parent())) {
                        (1, Some(dir)) => format!("1 arquivo criado em {}", dir.display()),
                        (n, Some(dir)) => format!("{n} arquivos criados em {}", dir.display()),
                        _ => "Nenhum arquivo criado.".into(),
                    };
                }
                Response::FieldsChanged { fields, history } => {
                    let mut pages: Vec<i32> =
                        self.fields.iter().chain(&fields).map(|f| f.page).collect();
                    pages.dedup();
                    self.fields = fields;
                    self.prepare.on_fields(&self.fields);
                    self.history = history;
                    if let Some(view) = self.view.as_mut() {
                        pages.into_iter().for_each(|p| view.invalidate_page(p));
                    }
                }
                Response::FieldProps { page, id, props } => {
                    if self.prepare.props_for.take() == Some((page, id)) {
                        let kind = self.fields.iter().find(|f| f.page == page && f.id == id);
                        match (props, kind) {
                            (Ok(props), Some(f)) => {
                                self.prepare.props =
                                    Some(crate::shell::field_props::FieldPropsWindow::new(
                                        page, id, &f.kind, props,
                                    ));
                            }
                            (Err(e), _) => {
                                self.status = format!("Não foi possível ler o campo: {e}")
                            }
                            _ => {}
                        }
                    }
                }
                Response::SplitFailed(msg) => {
                    self.status = format!("Não foi possível dividir: {msg}");
                }
                Response::Extracted(files) => {
                    if let Some(pages) = self.delete_after_extract.take() {
                        self.worker
                            .send(Request::Pages(pdf_core::PageOp::Delete { pages }));
                    }
                    self.status = match files.as_slice() {
                        [one] => format!("Páginas extraídas para {}", one.display()),
                        many => format!("{} arquivos extraídos", many.len()),
                    };
                }
                Response::EditFailed(msg) => {
                    self.status = format!("Não foi possível fazer a alteração: {msg}");
                }
                Response::PageImages { page, images } => self.edit_pdf.on_images(page, images),
                Response::RedactHits(hits) => {
                    if let Some(find) = self.redact.find.as_mut() {
                        find.on_hits(hits);
                    }
                }
                Response::PaginationState {
                    kind,
                    present,
                    config,
                } => self.pagination.on_state(kind, present, config),
                Response::PageTextBlocks { page, blocks } => {
                    self.edit_pdf.on_text_blocks(page, blocks)
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
            if self.organize.is_some() {
                self.organize = None; // Esc fecha o Organizar páginas
            } else if self.prepare_tools && self.prepare.escape() {
                // Preparar formulário: solta a ferramenta, depois a seleção
            } else if self.edit_pdf_tools && self.edit_pdf.escape() {
                // Editar PDF: solta a imagem a colocar, depois a seleção
            } else if self.edit.fill.dialog.take().is_some() {
                // Esc fecha a janela de assinatura (como Cancelar)
                self.edit.fill.into_field = None;
            } else if !self.form.escape() {
                self.edit.escape();
            }
        }
        if !typing {
            self.nudge_selected(ctx);
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
            match pressed.map(|(key, _)| key) {
                // Home/End são saltos: entram no histórico, como no Acrobat
                Some(NavKey::Home) => self.run(ctx, DocCommand::FirstPage),
                Some(NavKey::End) => self.run(ctx, DocCommand::LastPage),
                Some(key) => {
                    if let Some(view) = self.view.as_mut() {
                        if view.mode.is_paged() {
                            if view.turn(if key == NavKey::PageUp { -1 } else { 1 }) {
                                self.jump_to = Some(0.0);
                            }
                        } else {
                            let (scroll, height) = (self.last.scroll_y, self.last.viewport_h);
                            self.jump_to =
                                Some(nav_target(key, scroll, height, view.layout.total_height()));
                        }
                    }
                }
                None => {}
            }
            // Espaço segurado: Mão temporária (como no Acrobat), em qualquer modo
            let space = ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                i.key_down(egui::Key::Space)
            });
            if space {
                self.edit.tool = self.temp_tool.press(self.edit.tool);
            } else if let Some(tool) = self.temp_tool.release() {
                self.edit.tool = tool;
            }
            // ←/→ mudam de página em qualquer modo. Com a janela de assinatura ou de propriedades
            // do campo aberta, as setas são dela.
            let arrows = self.edit.fill.dialog.is_none() && self.prepare.props.is_none();
            let step = ctx.input_mut(|i| {
                [
                    (egui::Key::ArrowRight, arrows, DocCommand::NextPage),
                    (egui::Key::ArrowLeft, arrows, DocCommand::PrevPage),
                ]
                .into_iter()
                .find(|&(k, on, _)| on && i.consume_key(egui::Modifiers::NONE, k))
                .map(|(_, _, c)| c)
            });
            if let Some(cmd) = step {
                self.run(ctx, cmd);
            }
        }
        let zoom_delta = ctx.input(|i| i.zoom_delta()); // Ctrl+roda e gesto de pinça
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            if let Some(z) = self.view.as_ref().map(|v| v.zoom * zoom_delta) {
                self.zoom_to(z);
            }
        }
    }

    /// Setas movem a anotação selecionada (1 pt; Shift, 10 pt), como no Acrobat.
    fn nudge_selected(&mut self, ctx: &egui::Context) {
        // com a janela de assinatura aberta, as setas são dela; no Preparar, movem o campo; no
        // Editar PDF, só a imagem
        let image = self.edit_pdf.selected.is_some();
        let field = self.prepare.selected.filter(|_| self.prepare_tools);
        let target = field
            .or(self.edit.selected)
            .filter(|_| self.edit.fill.dialog.is_none() && self.prepare.props.is_none());
        if !(if self.edit_pdf_tools {
            image
        } else {
            target.is_some()
        }) {
            return;
        }
        let Some((dir, shift)) = arrow_pressed(ctx) else {
            return;
        };
        let rotation = self
            .view
            .as_ref()
            .map_or_else(Default::default, |v| v.rotation);
        let (dx, dy) = app_state::fill_sign::nudge(dir, shift, rotation);
        if self.edit_pdf_tools {
            self.edit_image(pdf_core::ImageEdit::Move { dx, dy });
            return;
        }
        let Some((page, id)) = target else {
            return;
        };
        let rect = field.and_then(|_| {
            self.fields
                .iter()
                .find(|f| f.page == page && f.id == id)
                .map(|f| f.rect.translated(dx, dy))
        });
        match rect {
            Some(rect) => self
                .worker
                .send(Request::Prepare(pdf_core::PrepareOp::Move {
                    page,
                    id,
                    rect,
                })),
            None if !self.security.permissions.annotate => self.deny("mover comentários"),
            None => self
                .worker
                .send(Request::Annot(pdf_core::AnnotOp::Move { page, id, dx, dy })),
        }
    }

    /// Comprimir PDF: abre a janela (como a ferramenta do Acrobat).
    pub fn open_compress(&mut self) {
        // reduzir imagens altera o documento: precisa da permissão de alterar (como no Acrobat)
        if !self.security.permissions.modify {
            self.deny("comprimir");
            return;
        }
        // já aberta (talvez comprimindo): continua como está
        if self.view.is_some() && self.compress.is_none() {
            self.compress = Some((1, false));
        }
    }

    /// Janela Comprimir PDF: tamanho atual, nível e Comprimir.
    fn compress_window(&mut self, ctx: &egui::Context) {
        let Some((level, busy)) = self.compress.as_mut() else {
            return;
        };
        const LEVELS: [(&str, &str); 3] = [
            (
                "Baixa compactação",
                "Só a estrutura do arquivo, sem perder qualidade.",
            ),
            (
                "Compactação média",
                "Fotos reduzidas a 150 ppp (boa para tela e impressão comum).",
            ),
            (
                "Alta compactação",
                "Fotos reduzidas a 100 ppp (menor arquivo, para e-mail).",
            ),
        ];
        let size = std::fs::metadata(&self.path).map_or(0, |m| m.len());
        let mut go = false;
        let mut open = true;
        egui::Window::new("Comprimir PDF")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Tamanho atual: {}", crate::shell::combine::size_text(size)));
                ui.separator();
                for (i, (name, hint)) in LEVELS.iter().enumerate() {
                    ui.radio_value(level, i, *name).on_hover_text(*hint);
                }
                ui.label(egui::RichText::new(LEVELS[*level].1).weak());
                ui.label("O resultado é gravado como \"(comprimido)\" ao lado do original e aberto numa aba.");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!*busy, egui::Button::new("Comprimir")).clicked() {
                        go = true;
                    }
                    if *busy {
                        ui.spinner();
                        ui.label("Comprimindo…");
                    }
                });
            });
        // durante a compressão a janela fica até o resultado chegar (nada de pedido em dobro)
        if !open && !*busy {
            self.compress = None;
            return;
        }
        if go {
            let lvl = match *level {
                0 => pdf_core::CompressLevel::Low,
                1 => pdf_core::CompressLevel::Medium,
                _ => pdf_core::CompressLevel::High,
            };
            *busy = true;
            self.worker.send(Request::Compress { level: lvl });
        }
    }

    /// Digitalizar e OCR: abre a janela Reconhecer texto (precisa da permissão de alterar).
    pub fn open_ocr(&mut self) {
        if !self.security.permissions.modify {
            self.deny("reconhecer texto");
            return;
        }
        if self.view.is_some() && self.ocr.is_none() {
            self.ocr = Some(OcrWindow::default());
        }
    }

    /// Janela Reconhecer texto: idioma, páginas, progresso e Cancelar.
    fn ocr_window(&mut self, ctx: &egui::Context) {
        let current = self.last.current_page as i32;
        let pages = self.view.as_ref().map_or(0, |v| v.sizes.len() as i32);
        let Some(w) = self.ocr.as_mut() else {
            return;
        };
        const LANGS: [(&str, pdf_core::OcrLanguage); 3] = [
            ("Português", pdf_core::OcrLanguage::Portuguese),
            ("Inglês", pdf_core::OcrLanguage::English),
            (
                "Português e inglês",
                pdf_core::OcrLanguage::PortugueseEnglish,
            ),
        ];
        let mut start = false;
        let mut cancel = false;
        let mut open = true;
        let lang = LANGS[w.lang].1;
        // procurar os dados custa alguns stat: só quando o idioma muda
        if w.data_for != Some(w.lang) {
            w.has_data = pdf_core::tessdata_dir(lang).is_some();
            w.data_for = Some(w.lang);
        }
        let has_data = w.has_data;
        egui::Window::new("Reconhecer texto")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let busy = w.cancel.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Idioma");
                        egui::ComboBox::from_id_salt("ocr-idioma")
                            .selected_text(LANGS[w.lang].0)
                            .show_index(ui, &mut w.lang, LANGS.len(), |i| LANGS[i].0);
                    });
                    ui.radio_value(&mut w.all_pages, true, "Todas as páginas");
                    ui.radio_value(&mut w.all_pages, false, format!("Só a página {}", current + 1));
                });
                ui.label(
                    egui::RichText::new(
                        "As páginas digitalizadas ganham texto pesquisável e selecionável, sem mudar a aparência. Páginas que já têm texto são puladas.",
                    )
                    .weak(),
                );
                if !has_data {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Faltam os dados de idioma do OCR (pacotes tesseract-langpack-por e tesseract-langpack-eng).",
                    );
                }
                if let Some((done, total)) = w.progress {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                            .text(format!("Página {} de {}", (done + 1).min(total), total)),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!busy && has_data, egui::Button::new("Reconhecer texto"))
                        .clicked()
                    {
                        start = true;
                    }
                    if busy && ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            if let Some(c) = &w.cancel {
                c.cancel();
            }
        }
        if !open && w.cancel.is_none() {
            self.ocr = None;
            return;
        }
        if start {
            let token = pdf_core::CancelToken::new();
            w.cancel = Some(token.clone());
            w.progress = Some((0, 1));
            let list = if w.all_pages {
                (0..pages).collect()
            } else {
                vec![current]
            };
            self.worker.send(Request::RecognizeText {
                pages: list,
                lang,
                cancel: token,
            });
        }
    }

    /// Exportar PDF: abre a janela (precisa da permissão de copiar o conteúdo).
    pub fn open_export(&mut self) {
        if !self.security.permissions.copy {
            self.deny("exportar o conteúdo");
            return;
        }
        if self.view.is_some() && self.export.is_none() {
            self.export = Some((0, 150, false));
        }
    }

    /// Janela Exportar PDF: formato, resolução das imagens e Exportar.
    fn export_window(&mut self, ctx: &egui::Context) {
        let Some((format, dpi, busy)) = self.export.as_mut() else {
            return;
        };
        const FORMATS: [&str; 4] = [
            "Microsoft Word (.docx)",
            "Texto (.txt)",
            "Imagens PNG (uma por página)",
            "Imagens JPEG (uma por página)",
        ];
        let mut go = false;
        let mut open = true;
        egui::Window::new("Exportar PDF")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for (i, name) in FORMATS.iter().enumerate() {
                    ui.radio_value(format, i, *name);
                }
                if *format >= 2 {
                    ui.horizontal(|ui| {
                        ui.label("Resolução");
                        for d in [72, 150, 300] {
                            ui.radio_value(dpi, d, format!("{d} ppp"));
                        }
                    });
                }
                ui.label("O resultado é gravado ao lado do original, sem sobrescrever nada.");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!*busy, egui::Button::new("Exportar"))
                        .clicked()
                    {
                        go = true;
                    }
                    if *busy {
                        ui.spinner();
                        ui.label("Exportando…");
                    }
                });
            });
        if !open && !*busy {
            self.export = None;
            return;
        }
        if go {
            let f = match *format {
                0 => pdf_core::ExportFormat::Word,
                1 => pdf_core::ExportFormat::Text,
                2 => pdf_core::ExportFormat::Png { dpi: *dpi },
                _ => pdf_core::ExportFormat::Jpeg { dpi: *dpi },
            };
            *busy = true;
            self.worker.send(Request::Export { format: f });
        }
    }

    /// Janelas do Proteger PDF: Proteger usando senha e Desbloquear.
    fn protect_windows(&mut self, ctx: &egui::Context) {
        if let Some(d) = self.protect.as_mut() {
            if let Some(outcome) = d.show(ctx) {
                self.protect = None;
                if let Some(p) = outcome {
                    self.worker.send(Request::SetProtection(Some(p)));
                }
            }
        }
        let Some(pw) = self.unlock.as_mut() else {
            return;
        };
        let mut done = None;
        let mut open = true;
        egui::Window::new("Desbloquear")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Senha de permissões:");
                let r = ui.add(egui::TextEdit::singleline(pw).password(true));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                // o campo já abre pronto para digitar
                if pw.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || enter {
                        done = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        done = Some(false);
                    }
                });
            });
        match (done, open) {
            (Some(true), _) => {
                let password = self.unlock.take().unwrap_or_default();
                self.worker.send(Request::Unlock { password });
            }
            (Some(false), _) | (None, false) => self.unlock = None,
            _ => {}
        }
    }

    /// Remover segurança: grava sem criptografia no próximo Salvar.
    pub fn remove_security(&mut self) {
        self.worker.send(Request::SetProtection(None));
    }

    /// Valida de novo as assinaturas (o worker relê o arquivo).
    pub fn validate_signatures(&mut self) {
        self.worker.send(Request::Signatures);
        self.status = "Assinaturas validadas.".into();
    }

    /// Janela "Assinar com um ID digital": Assinar manda ao worker (que grava o arquivo).
    fn sign_window(&mut self, ctx: &egui::Context) {
        let Some(d) = self.cert.dialog.as_mut() else {
            return;
        };
        match d.show(ctx) {
            Some(crate::shell::sign_dialog::SignOutcome::Sign {
                place,
                id,
                reason,
                location,
            }) => {
                self.cert.dialog = None;
                self.status = "Assinando…".into();
                self.worker.send(Request::SignDigitally {
                    place,
                    id,
                    reason,
                    location,
                });
            }
            Some(crate::shell::sign_dialog::SignOutcome::Cancel) => self.cert.dialog = None,
            None => {}
        }
    }

    /// Janela Propriedades do campo (Preparar formulário): OK manda as propriedades ao worker.
    fn field_props_window(&mut self, ctx: &egui::Context) {
        let Some(w) = self.prepare.props.as_mut() else {
            return;
        };
        let (page, id) = (w.page, w.id);
        if let Some(outcome) = w.show(ctx) {
            self.prepare.props = None;
            if let Some(props) = outcome {
                self.worker
                    .send(Request::Prepare(pdf_core::PrepareOp::SetProps {
                        page,
                        id,
                        props,
                    }));
            }
        }
    }

    /// Abre a linha de ferramentas `id` (Comentar ou Preencher e assinar; uma por vez).
    pub fn show_row(&mut self, id: &str) {
        use app_state::tools_catalog as tc;
        let p = self.security.permissions;
        let denied = match id {
            tc::COMMENT => (!p.annotate).then_some("comentar"),
            tc::FILL_SIGN => (!p.annotate).then_some("preencher e assinar"),
            tc::PREPARE_FORM => (!p.modify).then_some("alterar formulários"),
            tc::EDIT_PDF => (!p.modify).then_some("editar o PDF"),
            tc::WATERMARK | tc::HEADER_FOOTER => (!p.modify).then_some("alterar as páginas"),
            tc::CERTIFICATES => (!p.forms).then_some("assinar"),
            tc::REDACT => (!p.modify).then_some("redigir"),
            _ => None,
        };
        if let Some(what) = denied {
            self.deny(what);
            return;
        }
        self.protect_tools = id == tc::PROTECT;
        self.compare_tools = id == tc::COMPARE;
        self.comment_tools = id == app_state::tools_catalog::COMMENT;
        self.fill_tools = id == app_state::tools_catalog::FILL_SIGN;
        self.prepare_tools = id == app_state::tools_catalog::PREPARE_FORM;
        self.cert_tools = id == app_state::tools_catalog::CERTIFICATES;
        self.cert.placing = false;
        self.prepare.set_tool(None);
        self.prepare.selected = None;
        self.edit_pdf_tools = id == tc::EDIT_PDF;
        let pagination = match id {
            tc::WATERMARK => Some(pdf_core::Pagination::Watermark),
            tc::HEADER_FOOTER => Some(pdf_core::Pagination::HeaderFooter),
            _ => None,
        };
        if let Some(r) = self.pagination.set_tool(pagination) {
            self.worker.send(r);
        }
        if let Some(action) = self.edit_pdf.reset() {
            send_edit_action(&self.worker, &mut self.status, action);
        }
        self.edit.set_fill_tool(None);
        self.redact.set_open(id == tc::REDACT);
        // a ferramenta com que a linha abre (o Redigir já marcando texto)
        self.edit.tool = if self.redact.open {
            app_state::tools::Tool::RedactText
        } else {
            app_state::tools::Tool::Select
        };
    }

    /// Editar PDF: aplica `edit` à imagem selecionada.
    pub(crate) fn edit_image(&mut self, edit: pdf_core::ImageEdit) {
        if let Some(action) = self.edit_pdf.edit_selected(edit) {
            send_edit_action(&self.worker, &mut self.status, action);
        }
    }

    /// Marca d'água: escolhe o arquivo (imagem ou PDF) de origem.
    pub(crate) fn pick_watermark_file(&mut self, ctx: &egui::Context) {
        if self.files.busy() {
            return;
        }
        self.picking = Some(PickFor::WatermarkFile);
        self.files.pick_image(
            ctx,
            "Arquivo da marca d'água",
            crate::shell::watermark_dialog::FILE_EXTS,
        );
    }

    /// Marca d'água / Cabeçalho e rodapé: Remover.
    pub(crate) fn remove_pagination(&mut self) {
        if let Some(kind) = self.pagination.tool {
            self.worker.send(Request::RemovePagination(kind));
        }
    }

    /// Editar PDF: escolhe a imagem para substituir a selecionada (`replace`) ou para acrescentar.
    pub(crate) fn pick_image(&mut self, ctx: &egui::Context, replace: bool) {
        if self.files.busy() {
            return;
        }
        let (target, title) = if replace {
            let Some(sel) = self.edit_pdf.selected.clone() else {
                return;
            };
            (Some(sel), "Substituir imagem")
        } else {
            (None, "Adicionar imagem")
        };
        self.picking = Some(PickFor::Image(target));
        self.files
            .pick_image(ctx, title, crate::open_dialog::IMAGE_EXTS);
    }

    /// Permissão negada pela proteção do documento: explica na barra de status.
    pub(crate) fn deny(&mut self, what: &str) {
        self.status = format!(
            "O documento não permite {what}: está protegido. Em Proteger PDF, Desbloquear com a senha de permissões."
        );
    }

    /// Início do quadro: tem que vir antes de qualquer pedido de render (painel ou canvas).
    pub fn begin(&mut self) {
        self.edit.can_copy = self.security.permissions.copy;
        self.edit.can_annotate = self.security.permissions.annotate;
        self.edit.fill.active = self.fill_tools;
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
        if !self.security.permissions.print {
            self.deny("imprimir");
            return;
        }
        if self.view.is_some() && self.print_pending.is_none() {
            self.print_dialog = Some(PrintDialog::new(self.last.current_page));
            self.printers_rx = Some(print_job::printers_in_background(ctx));
        }
    }

    /// Opções escolhidas: o worker grava a cópia temporária; o envio continua em `send_to_printer`.
    fn start_print(&mut self, options: PrintOptions) {
        self.commit_field();
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
        if !self.cert.signatures.is_empty() && self.organize.is_none() {
            let open = egui::Panel::top("assinaturas")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| crate::certificates::bar(ui, &self.cert.signatures))
                .inner;
            if open {
                self.events.push(DocEvent::ShowPanel(LeftPanel::Signatures));
            }
        }
        if !self.fields.is_empty() && self.organize.is_none() && !self.prepare_tools {
            let reset = egui::Panel::top("formulario")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| crate::fields::bar(ui, &mut self.form))
                .inner;
            if reset && !self.security.permissions.forms {
                self.deny("limpar o formulário");
            } else if reset {
                self.worker.send(Request::ResetForm);
            }
        }
        if self.organize.is_some() {
            egui::CentralPanel::default().show(ui, |ui| self.organize_view(ui));
        } else {
            egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        }
        let down = ui.input(|i| i.pointer.any_down());
        self.edit.drop_stale_drag(down);
        self.edit.note_window(ui);
        self.edit.props_window(ui, &self.annots);
        if let Some(msg) = self.edit.signature_window(ui.ctx()) {
            self.status = msg;
        }
        self.field_props_window(ui.ctx());
        self.protect_windows(ui.ctx());
        self.pagination_windows(ui.ctx());
        self.redact_windows(ui.ctx());
        self.compress_window(ui.ctx());
        self.export_window(ui.ctx());
        self.ocr_window(ui.ctx());
        self.sign_window(ui.ctx());
        self.split_window(ui);
        self.poll_files();
        self.identity_window(ui);
        self.send_edits();
        self.password_window(ui);
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
        }
    }

    /// Conteúdo do painel esquerdo; clicar leva à página.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, panel: LeftPanel) {
        if panel == LeftPanel::Signatures {
            match crate::certificates::panel(ui, &self.cert.signatures, &self.fields) {
                Some(crate::certificates::PanelAction::Validate) => {
                    self.worker.send(Request::Signatures)
                }
                Some(crate::certificates::PanelAction::GoTo(page, r)) => {
                    self.jump(page, Some((r.x0, r.y0)));
                }
                None => {}
            }
            return;
        }
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
        self.jump(target.0, target.1);
    }

    /// ◀ campo da página / total ▶
    pub fn page_controls(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        let count = view.layout.page_count().max(1);
        let mut page = self.last.current_page + 1;
        let prev = ui
            .button("◀")
            .on_hover_ui(|ui| {
                ui.label(commands::hover(&Command::Doc(DocCommand::PrevPage)));
            })
            .clicked();
        let go = ui
            .add(egui::DragValue::new(&mut page).range(1..=count).speed(0.2))
            .on_hover_ui(|ui| {
                ui.label(commands::hover(&Command::Doc(DocCommand::GoToPage)));
            });
        if std::mem::take(&mut self.focus_page) {
            go.request_focus(); // Ir para página: digita o número e Enter
        }
        ui.label(format!("/ {count}"));
        let next = ui
            .button("▶")
            .on_hover_ui(|ui| {
                ui.label(commands::hover(&Command::Doc(DocCommand::NextPage)));
            })
            .clicked();
        if prev || next {
            self.navigate(if next {
                DocCommand::NextPage
            } else {
                DocCommand::PrevPage
            });
        } else if go.changed() {
            self.jump(page as i32 - 1, None);
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
            if let Some((page, rect)) = self.search.step(next) {
                self.jump(page, Some((rect.x0, rect.y0)));
            }
        }
    }

    /// Campo de zoom editável ("150" e Enter) e a lista ⏷ com os níveis e os ajustes, como no
    /// Acrobat. Devolve o ajuste escolhido na lista.
    pub fn zoom_controls(&mut self, ui: &mut egui::Ui) -> Option<DocCommand> {
        let zoom = self.view.as_ref()?.zoom;
        let mut text = self
            .zoom_text
            .take()
            .unwrap_or_else(|| zoom::percent_text(zoom));
        let field = ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(60.0))
            .on_hover_ui(|ui| {
                ui.label(commands::hover(&Command::Doc(DocCommand::ZoomTo)));
            });
        if std::mem::take(&mut self.focus_zoom) {
            field.request_focus();
        }
        if field.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(z) = zoom::parse_zoom(&text) {
                    self.zoom_to(z); // valor inválido: o campo volta ao zoom atual
                }
            }
        } else if field.has_focus() {
            self.zoom_text = Some(text);
        }
        let mut picked = None;
        ui.menu_button("⏷", |ui| {
            for level in zoom::LEVELS {
                if ui.button(zoom::percent_text(level)).clicked() {
                    self.zoom_to(level);
                }
            }
            ui.separator();
            for fit in [Fit::Page, Fit::Width, Fit::Height, Fit::Visible] {
                let cmd = DocCommand::Fit(fit);
                if ui.button(Command::Doc(cmd).label()).clicked() {
                    picked = Some(cmd);
                }
            }
        });
        picked
    }

    /// Abre o diálogo Dividir documento (partes na pasta do documento, por padrão).
    pub fn open_split(&mut self) {
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_default();
        self.split = Some(SplitDialog {
            by: SplitBy::Pages,
            pages: 1,
            mb: 10.0,
            dir,
        });
    }

    fn split_window(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.split.as_mut() else {
            return;
        };
        let (mut go, mut cancel, mut pick) = (false, false, false);
        egui::Window::new("Dividir documento")
            .collapsible(false)
            .resizable(false)
            .show(ui, |ui| {
                ui.label("Dividir por");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.by, SplitBy::Pages, "Número de páginas:");
                    ui.add(egui::DragValue::new(&mut d.pages).range(1..=9999));
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.by, SplitBy::Size, "Tamanho máximo do arquivo:");
                    ui.add(
                        egui::DragValue::new(&mut d.mb)
                            .range(0.1..=10_000.0)
                            .speed(0.5)
                            .suffix(" MB"),
                    );
                });
                ui.radio_value(
                    &mut d.by,
                    SplitBy::Bookmarks,
                    "Marcadores de primeiro nível",
                );
                ui.separator();
                ui.horizontal(|ui| {
                    let folder = d.dir.file_name().map_or_else(
                        || d.dir.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    ui.label(format!("Pasta: {folder}"))
                        .on_hover_text(d.dir.display().to_string());
                    pick = ui.button("Escolher pasta…").clicked();
                });
                let stem = self
                    .path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let example = match d.by {
                    SplitBy::Bookmarks => format!("{stem}_<marcador>.pdf"),
                    _ => format!("{stem}_Parte1.pdf, {stem}_Parte2.pdf…"),
                };
                ui.label(
                    egui::RichText::new(format!("Arquivos: {example}"))
                        .small()
                        .weak(),
                );
                ui.horizontal(|ui| {
                    go = ui.button("Dividir").clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        if pick {
            let start = Some(d.dir.clone());
            if !self.files.busy() {
                self.picking = Some(PickFor::Folder);
                self.files.pick_folder(ui.ctx(), start);
            }
        }
        if go {
            let rule = match d.by {
                SplitBy::Pages => pdf_core::SplitRule::Pages(d.pages),
                SplitBy::Size => pdf_core::SplitRule::MaxBytes((d.mb * 1_048_576.0) as u64),
                SplitBy::Bookmarks => pdf_core::SplitRule::TopBookmarks,
            };
            self.worker.send(Request::Split {
                rule,
                dir: d.dir.clone(),
            });
            self.status = "Dividindo o documento…".into();
        }
        if go || cancel {
            self.split = None;
        }
    }

    /// Abre o Organizar páginas, já com um diálogo se pedido (Inserir / Recortar do hub).
    pub fn open_organize(&mut self, ctx: &egui::Context, tool: &str) {
        let p = self.security.permissions;
        if !p.assemble || (tool == app_state::tools_catalog::SPLIT && !p.copy) {
            self.deny("organizar as páginas");
            return;
        }
        let org = self.organize.get_or_insert_with(Default::default);
        match tool {
            app_state::tools_catalog::INSERT => {
                self.picking = Some(PickFor::File(crate::organize::Purpose::Insert));
                self.files.open(ctx);
            }
            app_state::tools_catalog::SPLIT => {
                self.organize = None;
                self.open_split();
            }
            app_state::tools_catalog::CROP => {
                org.dialog = Some(crate::organize::Dialog::Crop { mm: [0.0; 4] })
            }
            _ => {}
        }
    }

    /// A grade do Organizar páginas e as ações dela.
    fn organize_view(&mut self, ui: &mut egui::Ui) {
        use crate::organize::{show, OrgAction};
        let (Some(view), Some(org)) = (self.view.as_mut(), self.organize.as_mut()) else {
            return;
        };
        org.sel.clamp(view.sizes.len() as i32);
        let history = (self.history.can_undo, self.history.can_redo);
        let actions = show(ui, org, view, &self.worker, history);
        for action in actions {
            match action {
                OrgAction::Op(op) => self.worker.send(Request::Pages(op)),
                OrgAction::Undo => self.worker.send(Request::Undo),
                OrgAction::Redo => self.worker.send(Request::Redo),
                OrgAction::Close => self.organize = None,
                OrgAction::Split => self.open_split(),
                OrgAction::PickFile(purpose) if !self.files.busy() => {
                    self.picking = Some(PickFor::File(purpose));
                    self.files.open(ui.ctx());
                }
                OrgAction::PickFile(_) => {}
                OrgAction::Extract { .. } if !self.security.permissions.copy => {
                    self.deny("extrair páginas")
                }
                OrgAction::Extract {
                    pages,
                    separate,
                    delete_after,
                } => {
                    let stem = self
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let suggested = self.path.with_file_name(format!("{stem} (páginas).pdf"));
                    self.picking = Some(PickFor::Extract {
                        pages,
                        separate,
                        delete_after,
                    });
                    self.files.save_as(ui.ctx(), suggested);
                }
            }
        }
    }

    /// O arquivo ou a pasta escolhidos no seletor do Organizar/Dividir.
    fn poll_files(&mut self) {
        use crate::organize::{Dialog, Purpose};
        let Some(picked) = self.files.poll() else {
            return;
        };
        use crate::open_dialog::Picked;
        match (self.picking.take(), picked) {
            (Some(PickFor::File(purpose)), Picked::Open(files)) => {
                let Some(path) = files.into_iter().next() else {
                    return;
                };
                let dialog = match purpose {
                    Purpose::Insert => Dialog::InsertFile {
                        path,
                        after: true,
                        all: true,
                        from: 1,
                        to: 1,
                        password: String::new(),
                    },
                    Purpose::Replace => Dialog::Replace {
                        path,
                        from: 1,
                        password: String::new(),
                    },
                };
                if let Some(org) = self.organize.as_mut() {
                    org.dialog = Some(dialog);
                }
            }
            (
                Some(PickFor::Extract {
                    pages,
                    separate,
                    delete_after,
                }),
                Picked::SaveAs { dest, .. },
            ) => {
                self.worker.send(Request::Extract {
                    pages: pages.clone(),
                    dest,
                    separate,
                });
                // só exclui depois que a extração deu certo (resposta Extracted)
                self.delete_after_extract = delete_after.then_some(pages);
            }
            (Some(PickFor::Image(target)), Picked::Open(files)) => {
                let Some(path) = files.into_iter().next() else {
                    return;
                };
                match target {
                    Some((page, name)) => self.worker.send(Request::EditImage {
                        page,
                        name,
                        edit: pdf_core::ImageEdit::Replace { path },
                    }),
                    None => {
                        self.edit_pdf.placing = Some(crate::edit_pdf::Placing::Image(path));
                        self.status =
                            "Clique na página para colocar a imagem (Esc cancela).".into();
                    }
                }
            }
            (Some(PickFor::WatermarkFile), Picked::Open(files)) => {
                if let Some((crate::pagination_tool::PaginationWindow::Watermark(d), _)) =
                    self.pagination.dialog.as_mut()
                {
                    d.path = files.into_iter().next().or(d.path.take());
                }
            }
            (Some(PickFor::Folder), Picked::Folder(dir)) => {
                if let Some(split) = self.split.as_mut() {
                    split.dir = dir;
                }
            }
            (_, Picked::Failed) => {
                self.status = "Não foi possível abrir o seletor de arquivos.".into()
            }
            _ => {}
        }
    }

    /// Painel Comentários (na direita, com "Comentar" ligado).
    pub fn comments_panel(&mut self, ui: &mut egui::Ui) {
        use crate::comments_panel::{show, PanelAction};
        for action in show(ui, &mut self.comments, &self.annots, self.edit.selected) {
            match action {
                PanelAction::Select { page, id, at } => {
                    self.edit.selected = Some((page, id));
                    self.jump(page, Some(at));
                }
                PanelAction::Op(op) => self.edit.push_op(op),
                PanelAction::Properties { page, id } => {
                    self.edit.open_props(&self.annots, (page, id))
                }
            }
        }
        self.send_edits();
    }

    /// Nome do usuário: vai para os carimbos dinâmicos e, no worker, como autor das anotações.
    /// Repetir o mesmo nome não manda nada (a Identidade já o aplica antes de avisar a casca).
    pub fn set_author(&mut self, author: Option<String>) {
        if self.edit.author != author {
            self.edit.author = author.clone();
            self.worker.send(Request::SetAuthor(author));
        }
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
        let (avail_w, avail_h) = (ui.available_width() - 24.0, ui.available_height() - 24.0);
        if let Some(ratio) = self
            .view
            .as_mut()
            .and_then(|v| v.apply_fit(avail_w, avail_h))
        {
            self.after_zoom_change(ratio);
            if let Some(view) = self.view.as_ref().filter(|v| v.fit == Fit::Visible) {
                self.jump_x = view.visible_left(avail_w);
            }
        }
        let Some(view) = self.view.as_mut() else {
            ui.centered_and_justified(|ui| {
                ui.label(&self.status);
            });
            return;
        };
        let (worker, edit, search, annots) =
            (&self.worker, &mut self.edit, &self.search, &self.annots);
        let (form, fields) = (&mut self.form, &self.fields);
        let (prepare, preparing) = (&mut self.prepare, self.prepare_tools);
        let (edit_pdf, editing_pdf) = (&mut self.edit_pdf, self.edit_pdf_tools);
        let mut edit_actions = Vec::new();
        let signing = self.cert_tools && self.cert.placing;
        let cert = &mut self.cert;
        let mut sign_at = None;
        let mut field_actions = Vec::new();
        let mut prepare_ops = Vec::new();
        let can_fill = self.security.permissions.forms;
        let mut open_props = None;
        let links = &self.links;
        let mut link_clicked = None;
        let mut screen_action = None;
        let marquee = &mut self.marquee;
        let presenting = self.presentation.is_some();
        let mut turn = None;
        let jump_to = self.jump_to.take();
        self.pending_target = None; // a rolagem pedida vai para a tela agora
        let jump_x = self.jump_x.take();
        // cada aba com seu id: a área de rolagem guarda a posição de cada documento
        self.last = ui
            .push_id(self.id, |ui| {
                canvas::show_pages(
                    ui,
                    view,
                    worker,
                    jump_to,
                    jump_x,
                    &mut |ui, page, rect, map: &PageMap| {
                        paint_search_hits(ui, search, page as i32, map);
                        if signing {
                            // Certificados > Assinar digitalmente: arrastar ou clicar no campo
                            sign_at = sign_at.or(crate::certificates::on_page(
                                ui,
                                cert,
                                fields,
                                page as i32,
                                rect,
                                map,
                            ));
                            return;
                        }
                        if editing_pdf {
                            // Editar PDF: só o texto e as imagens (sem comentários nem campos)
                            edit_actions.extend(crate::edit_pdf::on_page(
                                ui,
                                edit_pdf,
                                worker,
                                page as i32,
                                rect,
                                map,
                            ));
                            return;
                        }
                        if preparing {
                            // Preparar formulário: só os campos (sem comentários nem preencher)
                            let (ops, props) = crate::prepare::on_page(
                                ui,
                                prepare,
                                fields,
                                page as i32,
                                rect,
                                map,
                            );
                            prepare_ops.extend(ops);
                            open_props = open_props.or(props);
                            return;
                        }
                        let response = edit.on_page(ui, worker, annots, page as i32, rect, map);
                        use app_state::tools::Tool;
                        if let Some(a) =
                            screen_gesture(ui, &response, edit.tool, marquee, page, map)
                        {
                            screen_action = Some(a);
                        }
                        if matches!(edit.tool, Tool::Select | Tool::Hand) {
                            let hit = link_gesture(ui, &response, links, annots, page as i32, map);
                            // tela cheia: clicar fora de link avança, o botão direito volta
                            if presenting && hit.is_none() {
                                if response.clicked() {
                                    turn = Some(DocCommand::NextPage);
                                } else if response.secondary_clicked() {
                                    turn = Some(DocCommand::PrevPage);
                                }
                            }
                            link_clicked = link_clicked.take().or(hit);
                        }
                        // depois dos gestos da página: os campos ficam por cima (sem a permissão
                        // de preencher, ficam só visíveis)
                        if !can_fill {
                            return;
                        }
                        field_actions.extend(crate::fields::on_page(
                            ui,
                            form,
                            fields,
                            page as i32,
                            map,
                        ));
                    },
                )
            })
            .inner;
        if let Some((page, r)) = self.form.scroll_to.take() {
            // o Tab foi a um campo de outra página
            self.jump_to = view.go_to(page, Some((r.x0, r.y0)));
        }
        if let Some(place) = sign_at {
            let author = self.edit.author.clone();
            self.cert.sign(place, author.as_deref());
        }
        for op in prepare_ops {
            self.worker.send(Request::Prepare(op));
        }
        for action in edit_actions {
            send_edit_action(&self.worker, &mut self.status, action);
        }
        if let Some((page, id)) = open_props {
            self.prepare.props_for = Some((page, id));
            self.worker.send(Request::FieldProps { page, id });
        }
        for action in field_actions {
            match action {
                crate::fields::FieldAction::Set { page, id, value } => {
                    self.worker.send(Request::SetField { page, id, value })
                }
                // como no Acrobat: com Preencher e assinar ativo, o campo recebe a assinatura
                // manuscrita; senão, o campo vazio abre a assinatura com ID digital
                crate::fields::FieldAction::Sign { page, rect, .. }
                    if self.edit.tool == app_state::tools::Tool::Fill =>
                {
                    self.edit.sign_field(page, rect)
                }
                // campo já assinado: o Acrobat mostra a assinatura (painel), não pede outra
                crate::fields::FieldAction::Sign { page, id, .. }
                    if self
                        .cert
                        .signatures
                        .iter()
                        .any(|s| s.page == page && s.id == id) =>
                {
                    self.events.push(DocEvent::ShowPanel(LeftPanel::Signatures));
                }
                crate::fields::FieldAction::Sign { page, id, .. } => {
                    let author = self.edit.author.clone();
                    self.cert
                        .sign(pdf_core::SignPlace::Field { page, id }, author.as_deref());
                }
            }
        }
        let now = ui.input(|i| i.time);
        if self.last.turn != 0 && now - self.turned_at > 0.25 && view.turn(self.last.turn) {
            self.turned_at = now;
            self.jump_to = Some(0.0);
        }
        match link_clicked {
            Some(pdf_core::LinkTarget::Page { page, at }) => self.jump(page, at),
            Some(target) => self.events.push(DocEvent::OpenLink(target)),
            None => {}
        }
        if let Some(cmd) = turn {
            self.run(ui.ctx(), cmd);
        }
        if let Some(action) = screen_action {
            self.screen_action(action);
        }
    }

    /// Mão (rola pelo arraste) e Zoom com marca (amplia até o retângulo, ou um nível no clique).
    fn screen_action(&mut self, action: ScreenAction) {
        let avail = (self.last.viewport_w, self.last.viewport_h);
        let target = match action {
            ScreenAction::Pan(d) => Some((
                (self.last.scroll_x - d.x).max(0.0),
                (self.last.scroll_y - d.y).max(0.0),
            )),
            ScreenAction::ZoomRect { page, a, b } => self
                .view
                .as_mut()
                .and_then(|v| v.zoom_to_rect(page, pdf_core::PdfRect::from_corners(a, b), avail)),
            ScreenAction::ZoomAt { page, pt, out } => self.view.as_mut().and_then(|v| {
                let z = if out {
                    zoom::zoom_out(v.zoom)
                } else {
                    zoom::zoom_in(v.zoom)
                };
                v.set_manual_zoom(z);
                v.center_on(page, pt, avail)
            }),
        };
        if let Some((x, y)) = target {
            self.jump_x = Some(x);
            self.jump_to = Some(y);
        }
    }
}

/// Vista de abertura vinda das Preferências.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenView {
    pub mode: Option<ViewMode>,
    pub zoom: app_state::prefs::DefaultZoom,
    pub last: Option<app_state::view_history::ViewEntry>,
}

/// O que a Mão ou o Zoom com marca pedem neste quadro.
enum ScreenAction {
    /// Arraste da Mão (tela).
    Pan(egui::Vec2),
    /// Retângulo do Zoom com marca, em pontos da página.
    ZoomRect {
        page: usize,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// Clique do Zoom com marca: um nível a mais (ou a menos, com Ctrl) em torno do ponto.
    ZoomAt {
        page: usize,
        pt: (f32, f32),
        out: bool,
    },
}

/// Gestos da Mão e do Zoom com marca na página (cursor, retângulo tracejado e a ação).
fn screen_gesture(
    ui: &egui::Ui,
    response: &egui::Response,
    tool: app_state::tools::Tool,
    marquee: &mut Option<(usize, egui::Pos2)>,
    page: usize,
    map: &PageMap,
) -> Option<ScreenAction> {
    use app_state::tools::Tool;
    match tool {
        Tool::Hand => {
            if response.hovered() || response.dragged() {
                let grab = if response.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                };
                ui.ctx().set_cursor_icon(grab);
            }
            response
                .dragged()
                .then(|| ScreenAction::Pan(response.drag_delta()))
                .filter(|a| matches!(a, ScreenAction::Pan(d) if *d != egui::Vec2::ZERO))
        }
        Tool::Zoom => {
            let ctrl = ui.input(|i| i.modifiers.command);
            if response.hovered() {
                let icon = if ctrl {
                    egui::CursorIcon::ZoomOut
                } else {
                    egui::CursorIcon::ZoomIn
                };
                ui.ctx().set_cursor_icon(icon);
            }
            let pointer = response.interact_pointer_pos().or(response.hover_pos());
            if response.drag_started() {
                *marquee = pointer.map(|p| (page, p));
            }
            let start = marquee.filter(|(p, _)| *p == page).map(|(_, s)| s);
            if let (Some(start), Some(now)) = (start, pointer) {
                if response.dragged() {
                    let r = egui::Rect::from_two_pos(start, now);
                    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 110, 220));
                    ui.painter()
                        .rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                }
                if response.drag_stopped() {
                    *marquee = None;
                    // arraste curto conta como clique
                    if start.distance(now) >= 4.0 {
                        return Some(ScreenAction::ZoomRect {
                            page,
                            a: map.to_page(start.into()),
                            b: map.to_page(now.into()),
                        });
                    }
                    return Some(ScreenAction::ZoomAt {
                        page,
                        pt: map.to_page(now.into()),
                        out: ctrl,
                    });
                }
            }
            match (response.clicked(), pointer) {
                (true, Some(p)) => Some(ScreenAction::ZoomAt {
                    page,
                    pt: map.to_page(p.into()),
                    out: ctrl,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Janela Reconhecer texto: idioma (índice), todas as páginas ou a atual, progresso e o
/// cancelamento do reconhecimento em andamento.
pub(crate) struct OcrWindow {
    lang: usize,
    all_pages: bool,
    progress: Option<(usize, usize)>,
    cancel: Option<pdf_core::CancelToken>,
    /// Há dados de idioma para `data_for` (o índice do idioma conferido).
    has_data: bool,
    data_for: Option<usize>,
}

impl Default for OcrWindow {
    fn default() -> Self {
        Self {
            lang: 2,
            all_pages: true,
            progress: None,
            cancel: None,
            has_data: false,
            data_for: None,
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
        // OCR em andamento: para (libera a CPU e a vez das outras abas)
        if let Some(c) = self.ocr.as_ref().and_then(|w| w.cancel.as_ref()) {
            c.cancel();
        }
    }
}

/// Editar PDF: a edição vai ao worker; os avisos, para a barra de status.
fn send_edit_action(worker: &Worker, status: &mut String, action: crate::edit_pdf::EditAction) {
    match action {
        crate::edit_pdf::EditAction::Send(request) => worker.send(request),
        crate::edit_pdf::EditAction::Notice(msg) => *status = msg,
    }
}

/// Seta apertada (sem modificador ou com Shift): (direção, com Shift).
fn arrow_pressed(ctx: &egui::Context) -> Option<((i8, i8), bool)> {
    let arrows = [
        (egui::Key::ArrowLeft, (-1, 0)),
        (egui::Key::ArrowRight, (1, 0)),
        (egui::Key::ArrowUp, (0, -1)),
        (egui::Key::ArrowDown, (0, 1)),
    ];
    ctx.input_mut(|i| {
        arrows.into_iter().find_map(|(k, dir)| {
            if i.consume_key(egui::Modifiers::SHIFT, k) {
                Some((dir, true))
            } else {
                i.consume_key(egui::Modifiers::NONE, k)
                    .then_some((dir, false))
            }
        })
    })
}

/// Com Selecionar, sobre um link sem comentário por cima: cursor de mão e dica com o destino.
/// Devolve o alvo quando o link é clicado (sem arrastar).
fn link_gesture(
    ui: &egui::Ui,
    response: &egui::Response,
    links: &[pdf_core::LinkInfo],
    annots: &[AnnotInfo],
    page: i32,
    map: &PageMap,
) -> Option<pdf_core::LinkTarget> {
    let pt = map.to_page(response.hover_pos()?.into());
    // o link primeiro: quase sempre não há, e é a busca mais barata
    let link = crate::links::link_at(links, page, pt)?;
    if crate::edit::hit_at(annots, page, pt, map).is_some() {
        return None;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    let clicked = response.clicked();
    response
        .clone()
        .on_hover_text_at_pointer(crate::links::hint(&link.target));
    clicked.then(|| link.target.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_with_pages(n: usize) -> DocTab {
        let ctx = egui::Context::default();
        let mut tab = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        let size = pdf_core::PageSize {
            width: 100.0,
            height: 200.0,
        };
        tab.view = Some(DocView::new(PathBuf::from("/a.pdf"), vec![size; n]));
        tab
    }

    #[test]
    fn a_pending_jump_survives_the_zoom_change() {
        // salto pedido antes do primeiro ajuste (página inicial do arquivo, link para outro PDF):
        // depois que o zoom se ajusta à janela, cai na mesma página (o vão entre páginas não escala)
        let mut tab = tab_with_pages(400);
        tab.jump(300, None);
        let ratio = tab.view.as_mut().unwrap().set_zoom(1.6);
        tab.after_zoom_change(ratio);
        let expected = tab.view.as_mut().unwrap().go_to(300, None);
        assert_eq!(tab.jump_to, expected);
        // sem salto pendente, a rolagem atual acompanha o zoom
        let mut tab = tab_with_pages(4);
        tab.last.scroll_y = 300.0;
        tab.after_zoom_change(0.5);
        assert_eq!(tab.jump_to, Some(150.0));
    }

    #[test]
    fn opening_view_uses_defaults_then_the_file_then_the_last_view() {
        use app_state::prefs::DefaultZoom;
        use app_state::view_history::ViewEntry;
        // padrão das Preferências
        let mut tab = tab_with_pages(6);
        tab.open_view = OpenView {
            mode: Some(ViewMode::TwoUp),
            zoom: DefaultZoom::FitPage,
            last: None,
        };
        tab.apply_open_view(pdf_core::InitialView::default());
        let v = tab.view.as_ref().unwrap();
        assert_eq!((v.mode, v.fit), (ViewMode::TwoUp, Fit::Page));
        // o arquivo prevalece sobre o padrão
        let mut tab = tab_with_pages(6);
        tab.open_view.mode = Some(ViewMode::TwoUp);
        tab.apply_open_view(pdf_core::InitialView {
            layout: Some(pdf_core::InitialLayout::Single),
            ..Default::default()
        });
        assert_eq!(tab.view.as_ref().unwrap().mode, ViewMode::Single);
        // a última vista prevalece sobre tudo e vai à página dela
        let mut tab = tab_with_pages(6);
        tab.open_view.last = Some(ViewEntry {
            page: 4,
            y: 10.0,
            zoom: 2.0,
            fit: Fit::None,
        });
        tab.apply_open_view(pdf_core::InitialView {
            page: Some(1),
            ..Default::default()
        });
        let v = tab.view.as_ref().unwrap();
        assert!((v.zoom - 2.0).abs() < 1e-6);
        let expected = v.layout.page_top(4) - v.window().0 + 10.0 * 2.0;
        assert_eq!(tab.jump_to, Some(expected));
    }

    #[test]
    fn comment_keys_open_the_comment_row_and_shift_u_cycles() {
        use app_state::tools::Tool;
        let ctx = egui::Context::default();
        let mut tab = tab_with_pages(1);
        tab.run(&ctx, DocCommand::Tool(Tool::Note));
        assert!(tab.comment_tools);
        assert_eq!(tab.edit.tool, Tool::Note);
        tab.run(&ctx, DocCommand::CycleMarkup);
        assert_eq!(tab.edit.tool, Tool::Highlight);
        tab.run(&ctx, DocCommand::CycleMarkup);
        assert_eq!(tab.edit.tool, Tool::Underline);
        tab.run(&ctx, DocCommand::Tool(Tool::Hand));
        assert_eq!(tab.edit.tool, Tool::Hand);
    }

    #[test]
    fn presentation_restores_mode_and_fit() {
        let mut tab = tab_with_pages(4);
        let v = tab.view.as_mut().unwrap();
        v.cover = true;
        v.set_mode(ViewMode::TwoUpContinuous, 0);
        v.fit = Fit::Width;
        tab.enter_presentation();
        let v = tab.view.as_ref().unwrap();
        assert_eq!((v.mode, v.fit), (ViewMode::Single, Fit::Page));
        tab.enter_presentation(); // de novo: não perde o que guardou
        tab.exit_presentation();
        let v = tab.view.as_ref().unwrap();
        assert_eq!(
            (v.mode, v.cover, v.fit),
            (ViewMode::TwoUpContinuous, true, Fit::Width)
        );
        tab.exit_presentation(); // sem apresentação: nada muda
        assert_eq!(tab.view.as_ref().unwrap().mode, ViewMode::TwoUpContinuous);
    }

    #[test]
    fn initial_view_sets_layout_zoom_page_panel_and_full_screen() {
        let mut tab = tab_with_pages(4);
        tab.apply_initial_view(pdf_core::InitialView {
            panel: Some(pdf_core::InitialPanel::Bookmarks),
            full_screen: true,
            layout: Some(pdf_core::InitialLayout::TwoPage { cover: true }),
            page: Some(2),
            zoom: Some(pdf_core::InitialZoom::Factor(1.5)),
        });
        let v = tab.view.as_ref().unwrap();
        assert_eq!((v.mode, v.cover, v.fit), (ViewMode::TwoUp, true, Fit::None));
        assert!((v.zoom - 1.5).abs() < 1e-6);
        assert_eq!(v.spread, v.layout.row_of(2), "abre na página pedida");
        let events = tab.take_events();
        assert!(events.contains(&DocEvent::ShowPanel(LeftPanel::Outline)));
        assert!(events.contains(&DocEvent::FullScreen));
        // sem nada pedido, nada muda
        let mut tab = tab_with_pages(2);
        tab.apply_initial_view(pdf_core::InitialView::default());
        let v = tab.view.as_ref().unwrap();
        assert_eq!((v.mode, v.fit), (ViewMode::Continuous, Fit::Width));
        assert!(tab.take_events().is_empty());
    }

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
    fn saving_or_undoing_first_commits_the_text_being_edited() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        tab.edit_pdf.on_text_blocks(
            0,
            vec![pdf_core::PageTextBlock {
                rect: pdf_core::PdfRect::new(10.0, 10.0, 90.0, 25.0),
                text: "Olá".into(),
                size: 12.0,
                color: [0, 0, 0],
                font: "Helvetica".into(),
            }],
        );
        tab.edit_pdf.click(0, (50.0, 18.0));
        tab.edit_pdf.editing.as_mut().unwrap().buffer = "Oi".into();
        assert!(tab.commit_field(), "o texto em edição vai antes de gravar");
        assert!(tab.edit_pdf.editing.is_none());
    }

    #[test]
    fn opening_redact_marks_text_and_closing_it_goes_back_to_select() {
        use app_state::tools::Tool;
        let ctx = egui::Context::default();
        let mut tab = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        tab.show_row(app_state::tools_catalog::REDACT);
        assert!(tab.redact.open);
        assert_eq!(tab.edit.tool, Tool::RedactText);
        tab.show_row("");
        assert_eq!(tab.edit.tool, Tool::Select);
    }

    /// Como o Acrobat: salvar com marcas de redação não aplicadas pergunta antes (o arquivo sairia
    /// com o texto intacto sob as marcas).
    #[test]
    fn saving_with_unapplied_redaction_marks_asks_first() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        tab.annots.push(pdf_core::AnnotInfo {
            kind: pdf_core::AnnotKind::Redact,
            ..Default::default()
        });
        tab.save();
        assert!(tab.redact.pending_save, "pergunta antes de salvar");
    }

    /// Aplicar e salvar falha fechado: grava só depois que a redação foi aplicada; se ela falhar,
    /// nada é salvo (o arquivo sairia com o conteúdo sob as marcas).
    #[test]
    fn apply_and_save_waits_for_the_redaction_and_saves_nothing_if_it_fails() {
        let ctx = egui::Context::default();
        let mut tab = DocTab::new(&ctx, PathBuf::from("/a.pdf"), false);
        tab.answer_save(crate::redact_tool::SaveChoice::ApplyAndSave);
        assert!(tab.redact.save_after_apply, "espera o resultado da redação");
        tab.on_redaction_outcome(false);
        assert!(!tab.redact.save_after_apply);
        assert!(tab.status.contains("não foi salvo"), "{}", tab.status);
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
