use crate::bench::Bench;
use crate::doc_tab::{DocEvent, DocTab};
use crate::open_dialog::{FileDialogs, Picked};
use crate::shell::close;
use crate::shell::commands::{self, Command};
use crate::shell::home::{self, HomeAction};
use crate::shell::menus::{self, MenuState};
use crate::shell::tabs::{self as tab_bar, TabAction, TabLabel};
use crate::shell::{help, panels, toolbar, tools_hub};
use app_state::close::{plan_close, CloseFlow, Step};
use app_state::prefs::{Prefs, MAX_PANEL, MIN_PANEL};
use app_state::recents::{now_secs, RecentFiles};
use app_state::store::{self, BackgroundSaver};
use app_state::tabs::{canonical, tab_title, TabList, FIXED, HOME, TOOLS};
use app_state::tools_catalog;
use std::path::{Path, PathBuf};

const RECENTS_FILE: &str = "recentes.json";
const PREFS_FILE: &str = "preferencias.json";

pub struct GeraApp {
    tabs: TabList,
    /// Um por aba de documento, na mesma ordem de `tabs.docs()`.
    docs: Vec<DocTab>,
    dialogs: FileDialogs,
    recents: RecentFiles,
    /// Recentes que ainda existem no disco (atualizado ao entrar no Início, não a cada quadro).
    recent_exists: Vec<bool>,
    prefs: Prefs,
    saver: BackgroundSaver,
    /// Mensagem da casca (falhas do seletor ou de abertura), mostrada no Início.
    status: String,
    closing: Option<CloseFlow>,
    /// Saída já confirmada: não interceptar o fechamento da janela de novo.
    allow_close: bool,
    /// Preferências mudaram e ainda não foram gravadas.
    prefs_dirty: bool,
    /// Recentes mudaram e ainda não foram gravados.
    recents_dirty: bool,
    fullscreen: bool,
    /// Modo leitura: só o menu, as abas e as páginas.
    reading: bool,
    /// Último movimento do mouse (a barra da tela cheia some 2 s depois).
    last_motion: f64,
    /// Janela Preferências aberta (as mudanças só valem com OK).
    preferences: Option<crate::shell::preferences::PrefsDraft>,
    /// O arquivo pediu tela cheia: a pergunta está aberta.
    full_screen_request: Option<crate::shell::reading::FullScreenRequest>,
    /// Linha de ferramentas (Comentar ou Preencher e assinar) a abrir no próximo documento
    /// aberto pelo seletor.
    row_after_open: Option<&'static str>,
    /// As imagens do próximo seletor viram um PDF (Criar PDF).
    create_after_pick: bool,
    /// Criar PDF em andamento.
    creating: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    /// Último documento que esteve na frente (para o hub voltar a ele).
    last_doc: Option<PathBuf>,
    show_shortcuts: bool,
    show_about: bool,
    /// Janela Combinar arquivos.
    combine: crate::shell::combine::CombineWindow,
    compare: crate::shell::compare::CompareWindow,
    /// Aviso de segurança de um link (endereço ou outro arquivo) esperando resposta.
    link_warning: Option<crate::links::LinkWarning>,
    bench: Option<Bench>,
}

impl GeraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<PathBuf>,
        bench: Option<Bench>,
    ) -> Self {
        crate::print_job::sweep();
        let prefs = store::config_file(PREFS_FILE).map_or_else(Prefs::default, |p| Prefs::load(&p));
        cc.egui_ctx.set_visuals(visuals(prefs.dark));
        crate::fill_sign::install_script_font(&cc.egui_ctx);
        let mut app = Self {
            tabs: TabList::default(),
            docs: Vec::new(),
            dialogs: FileDialogs::default(),
            recents: store::config_file(RECENTS_FILE)
                .map(|p| RecentFiles::load(&p))
                .unwrap_or_default(),
            recent_exists: Vec::new(),
            prefs,
            saver: BackgroundSaver::default(),
            status: String::new(),
            closing: None,
            allow_close: false,
            prefs_dirty: false,
            recents_dirty: false,
            fullscreen: false,
            reading: false,
            last_motion: 0.0,
            full_screen_request: None,
            preferences: None,
            row_after_open: None,
            create_after_pick: false,
            creating: None,
            last_doc: None,
            show_shortcuts: false,
            show_about: false,
            combine: Default::default(),
            compare: Default::default(),
            link_warning: None,
            bench,
        };
        app.refresh_recent_exists();
        for path in paths {
            app.open_path(&cc.egui_ctx, path);
        }
        app
    }

    /// Criar PDF: as imagens viram um PDF novo (pasta de documentos criados) numa thread (fotos
    /// grandes levam segundos); o resultado abre numa aba como "não salvo".
    fn create_from_images(&mut self, ctx: &egui::Context, images: Vec<PathBuf>) {
        if self.creating.is_some() {
            self.status = "Já há um PDF sendo criado.".into();
            return;
        }
        let stem = images.first().and_then(|p| p.file_stem()).map_or_else(
            || "Imagens".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        let Some(dest) = store::created_pdf(&stem) else {
            self.status = "Pasta de documentos criados indisponível.".into();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("criar-pdf".into())
            .spawn(move || {
                let r = pdf_core::create_pdf_from_images(&images, &dest)
                    .map(|()| dest)
                    .map_err(|e| e.to_string());
                let _ = tx.send(r);
                ctx2.request_repaint();
            });
        if spawned.is_ok() {
            self.creating = Some(rx);
            self.status = "Criando o PDF…".into();
        }
    }

    /// O PDF criado ficou pronto: abre e marca como não salvo.
    fn poll_creating(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.creating else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("falha ao criar".into()),
        };
        self.creating = None;
        match result {
            Ok(dest) => {
                self.open_path(ctx, dest.clone());
                let tab = self.tabs.find(&canonical(&dest));
                if let Some(doc) = tab.and_then(|t| self.doc_at(t)) {
                    doc.created = true;
                }
                self.status =
                    "PDF criado a partir das imagens: Salvar escolhe onde guardá-lo.".into();
            }
            Err(e) => self.status = format!("Não foi possível criar o PDF: {e}"),
        }
    }

    pub(crate) fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        // abrir uma imagem cria um PDF com ela (como o Acrobat)
        if store::is_image(&path) {
            self.create_from_images(ctx, vec![path]);
            return;
        }
        let path = canonical(&path);
        let mut created = false;
        self.switch_tab(|tabs| created = tabs.open(path.clone()).1);
        if created {
            let mut doc = DocTab::new(ctx, path.clone(), self.prefs.dark);
            doc.set_author(self.prefs.author.clone());
            let key = path.to_string_lossy();
            doc.open_view = crate::doc_tab::OpenView {
                mode: self.prefs.default_mode,
                zoom: self.prefs.default_zoom,
                last: self
                    .prefs
                    .restore_views
                    .then(|| self.prefs.last_view(&key))
                    .flatten(),
            };
            doc.open(path);
            self.docs.push(doc);
        }
    }

    /// Único ponto de troca da aba da frente: suspende a que saiu (libera texturas) e lembra o
    /// último documento visto (para o hub voltar a ele).
    fn switch_tab(&mut self, change: impl FnOnce(&mut TabList)) {
        let old = self.tabs.active();
        change(&mut self.tabs);
        let new = self.tabs.active();
        if old == new {
            return;
        }
        self.status.clear(); // a mensagem era sobre a aba anterior
        if let Some(doc) = self.doc_at(old) {
            doc.suspend();
        }
        if let Some(p) = self.tabs.doc_path(new) {
            self.last_doc = Some(p.to_path_buf());
        } else if new == HOME {
            self.refresh_recent_exists();
        }
    }

    fn activate(&mut self, tab: usize) {
        self.switch_tab(|tabs| tabs.activate(tab));
    }

    fn active_path(&self) -> Option<PathBuf> {
        self.tabs
            .doc_path(self.tabs.active())
            .map(Path::to_path_buf)
    }

    fn refresh_recent_exists(&mut self) {
        self.recent_exists = self
            .recents
            .items()
            .iter()
            .map(|r| r.path.exists())
            .collect();
    }

    fn doc_at(&mut self, tab: usize) -> Option<&mut DocTab> {
        let i = self.tabs.doc_index(tab)?;
        self.docs.get_mut(i)
    }

    fn active_doc(&mut self) -> Option<&mut DocTab> {
        self.doc_at(self.tabs.active())
    }

    fn close_tab(&mut self, tab: usize) {
        if let Some(i) = self.tabs.close(tab) {
            let doc = self.docs.remove(i); // o Worker cai junto e a thread termina
            if let (true, Some(view)) = (self.prefs.restore_views, doc.current_view()) {
                // Restaurar a última vista ao reabrir (Preferências → Documentos)
                self.prefs.remember_view(&doc.path.to_string_lossy(), view);
                self.prefs_dirty = true;
            }
        }
    }

    /// Fecha as abas de `paths`, perguntando antes pelas que têm alterações pendentes.
    fn request_close(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>, quit: bool) {
        let targets = paths
            .into_iter()
            .filter_map(|p| {
                let confirm = self
                    .tabs
                    .find(&p)
                    .and_then(|t| self.doc_at(t))?
                    .needs_confirm();
                Some((p, confirm))
            })
            .collect();
        let (close_now, flow) = plan_close(targets, quit);
        for path in close_now {
            if let Some(t) = self.tabs.find(&path) {
                self.close_tab(t);
            }
        }
        self.closing = flow;
        if quit && self.closing.is_none() {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn remember(&mut self, path: PathBuf) {
        self.recents.add(path, now_secs());
        self.recents.limit(self.prefs.max_recents);
        self.recents_changed();
    }

    fn forget(&mut self, path: &Path) {
        self.recents.remove(path);
        self.recents_changed();
    }

    fn recents_changed(&mut self) {
        self.recents_dirty = true;
        self.refresh_recent_exists();
    }

    /// Respostas dos workers e do seletor de arquivos.
    fn poll(&mut self, ctx: &egui::Context) {
        let active = self.tabs.active();
        // arquivos a abrir em abas novas (depois do laço: abrir mexe na lista de abas)
        let mut to_open = Vec::new();
        let mut full_screen = false;
        for tab in (FIXED..self.tabs.len()).rev() {
            let Some(i) = self.tabs.doc_index(tab) else {
                continue;
            };
            let bench = if tab == active {
                self.bench.as_mut()
            } else {
                None
            };
            self.docs[i].poll(ctx, bench);
            for event in self.docs[i].take_events() {
                match event {
                    DocEvent::Opened => self.remember(self.docs[i].path.clone()),
                    DocEvent::Renamed(dest) => {
                        self.docs[i].created = false;
                        let dest = canonical(&dest);
                        self.tabs.rename(tab, dest.clone());
                        self.docs[i].path = dest.clone();
                        self.remember(dest);
                    }
                    DocEvent::Author(name) => self.set_author(name),
                    DocEvent::ShowPanel(p) => {
                        if self.prefs.left_panel != Some(p) {
                            self.prefs.toggle_left(p);
                            self.prefs_dirty = true;
                        }
                    }
                    DocEvent::Failed(msg) => {
                        self.status = msg;
                        self.close_tab(tab);
                        break;
                    }
                    DocEvent::OpenFile(path) => to_open.push(path),
                    DocEvent::NeedSaveAs => self.dialogs.save_as(ctx, self.docs[i].path.clone()),
                    DocEvent::FullScreen => full_screen |= tab == active && !self.fullscreen,
                    DocEvent::OpenLink(target) => {
                        use crate::links::Outcome;
                        match crate::links::outcome(&target, &self.prefs.link_sites) {
                            Some(Outcome::OpenUrl(url)) => {
                                ctx.open_url(egui::OpenUrl::new_tab(url))
                            }
                            Some(Outcome::Ask(warning)) => self.link_warning = Some(warning),
                            Some(Outcome::Refuse(msg)) => self.docs[i].status = msg,
                            None => {}
                        }
                    }
                }
            }
        }
        for path in to_open {
            self.open_path(ctx, path);
        }
        if full_screen {
            // o arquivo pediu (/PageMode /FullScreen): pergunta antes, como o Acrobat
            self.full_screen_request = Some(Default::default());
        }
        self.poll_creating(ctx);
        if let Some(picked) = self.dialogs.poll() {
            match picked {
                Picked::Open(files) if std::mem::take(&mut self.create_after_pick) => {
                    self.create_from_images(ctx, files);
                }
                Picked::Open(files) => {
                    for f in files {
                        self.open_path(ctx, f);
                    }
                    if let Some(row) = self.row_after_open.take() {
                        if let Some(doc) = self.active_doc() {
                            doc.show_row(row);
                        }
                    }
                }
                Picked::SaveAs { doc, dest } => self.save_as(doc, dest),
                Picked::Cancelled | Picked::Folder(_) | Picked::SaveTo(_) => {
                    self.create_after_pick = false;
                    self.row_after_open = None
                }
                Picked::Failed => {
                    self.create_after_pick = false;
                    self.row_after_open = None;
                    self.status = "Não foi possível abrir o seletor de arquivos.".into();
                }
            }
        }
    }

    fn save_as(&mut self, doc: PathBuf, dest: PathBuf) {
        let dest_id = canonical(&dest);
        let current = self.tabs.find(&doc);
        if self.tabs.find(&dest_id).is_some_and(|t| Some(t) != current) {
            self.status = "Esse arquivo já está aberto em outra aba.".into();
            return;
        }
        if let Some(d) = current.and_then(|t| self.doc_at(t)) {
            d.save_as(dest);
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.iter().map(|f| f.path().to_path_buf()) {
            self.open_path(ctx, file);
        }
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::Open => self.dialogs.open(ctx),
            Command::OpenRecent(path) => {
                if path.exists() {
                    self.open_path(ctx, path);
                } else {
                    self.status = format!("{} não existe mais.", path.display());
                }
            }
            Command::SaveAs => {
                if let Some(path) = self.active_path() {
                    self.dialogs.save_as(ctx, path);
                }
            }
            Command::CloseTab => {
                if let Some(path) = self.active_path() {
                    self.request_close(ctx, vec![path], false);
                }
            }
            Command::CloseAll => {
                let all = self.tabs.docs().to_vec();
                self.request_close(ctx, all, false);
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Doc(c) => {
                if let Some(doc) = self.active_doc() {
                    doc.run(ctx, c);
                }
            }
            Command::ToggleDark => self.set_dark(ctx, !self.prefs.dark),
            Command::TogglePanel(p) => {
                self.prefs.toggle_left(p);
                self.prefs_dirty = true;
            }
            Command::ToggleTools => {
                self.prefs.tools_panel = !self.prefs.tools_panel;
                self.prefs_dirty = true;
            }
            Command::Tool(id) => self.run_tool(ctx, id),
            Command::Preferences => {
                self.preferences = Some(crate::shell::preferences::PrefsDraft::new(&self.prefs))
            }
            Command::FullScreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                if self.fullscreen {
                    if let Some(doc) = self.active_doc() {
                        doc.enter_presentation();
                    }
                } else {
                    // a aba que entrou, mesmo que outra esteja na frente agora (as outras ignoram)
                    self.docs.iter_mut().for_each(DocTab::exit_presentation);
                }
            }
            Command::ReadMode => self.reading = !self.reading,
            Command::NextTab => self.switch_tab(TabList::next),
            Command::PrevTab => self.switch_tab(TabList::prev),
            Command::ActivateTab(t) => self.activate(t),
            Command::Shortcuts => self.show_shortcuts = true,
            Command::About => self.show_about = true,
            Command::Compare => self.run_tool(ctx, tools_catalog::COMPARE),
            Command::Print => {
                if let Some(doc) = self.active_doc() {
                    doc.open_print_dialog(ctx);
                }
            }
            Command::Properties => {
                if let Some(doc) = self.active_doc() {
                    doc.request_properties();
                }
            }
        }
    }

    /// Editar → Preferências → Identidade: muda o nome dos carimbos e autor das anotações.
    /// Guarda o nome nas preferências e o passa a todas as abas.
    fn set_author(&mut self, name: String) {
        self.prefs.author = Some(name.clone());
        self.prefs_dirty = true;
        for doc in &mut self.docs {
            doc.set_author(Some(name.clone()));
        }
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.prefs.dark = dark;
        ctx.set_visuals(visuals(dark));
        for doc in &mut self.docs {
            doc.set_dark(dark);
        }
        self.prefs_dirty = true;
    }

    /// Grava preferências e recentes em segundo plano, quando o usuário solta o mouse (não a cada
    /// pixel de um arrasto de painel).
    fn persist_when_idle(&mut self, ctx: &egui::Context) {
        if let Some(e) = self.saver.take_error() {
            self.status = e;
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            self.persist();
        }
    }

    /// Enfileira a gravação do que mudou.
    fn persist(&mut self) {
        if std::mem::take(&mut self.prefs_dirty) {
            if let Some(file) = store::config_file(PREFS_FILE) {
                self.saver.save(file, &self.prefs);
            }
        }
        if std::mem::take(&mut self.recents_dirty) {
            if let Some(file) = store::config_file(RECENTS_FILE) {
                self.saver.save(file, &self.recents);
            }
        }
    }

    /// Barras, trilho, painéis laterais e páginas do documento ativo (fora da tela cheia).
    fn document_view(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.tabs.doc_index(self.tabs.active()) else {
            return;
        };
        let doc = &mut self.docs[i];
        doc.begin();
        if self.reading {
            // modo leitura: só as páginas e a barra flutuante
            doc.show_canvas(ui);
            if let Some(cmd) = crate::shell::reading::bar(ui.ctx(), doc, false) {
                self.run(ui.ctx(), cmd);
            }
            return;
        }
        let mut cmd = egui::Panel::top("ferramentas-doc")
            .show(ui, |ui| toolbar::show(ui, doc))
            .inner;
        if doc.comment_tools {
            cmd = egui::Panel::top("comentar")
                .show(ui, |ui| toolbar::comment_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.redact.open {
            cmd = egui::Panel::top("redigir")
                .show(ui, |ui| toolbar::redact_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.compare_tools {
            cmd = egui::Panel::top("comparacao")
                .show(ui, |ui| toolbar::compare_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.protect_tools {
            cmd = egui::Panel::top("proteger")
                .show(ui, |ui| toolbar::protect_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.cert_tools {
            cmd = egui::Panel::top("certificados")
                .show(ui, |ui| toolbar::cert_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.pagination.tool.is_some() {
            cmd = egui::Panel::top("marca-cabecalho")
                .show(ui, |ui| toolbar::pagination_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.edit_pdf_tools {
            cmd = egui::Panel::top("editar-pdf")
                .show(ui, |ui| toolbar::edit_pdf_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.prepare_tools {
            cmd = egui::Panel::top("preparar-formulario")
                .show(ui, |ui| toolbar::prepare_row(ui, doc))
                .inner
                .or(cmd);
        }
        if doc.fill_tools {
            cmd = egui::Panel::top("preencher-assinar")
                .show(ui, |ui| toolbar::fill_row(ui, doc))
                .inner
                .or(cmd);
        }
        doc.show_status(ui, &self.status);
        // só os campos dos painéis (copiar as preferências inteiras a cada quadro custaria caro)
        let (mut left_panel, mut left_width, mut tools_width) = (
            self.prefs.left_panel,
            self.prefs.left_width,
            self.prefs.tools_width,
        );
        let clicked = egui::Panel::left("trilho")
            .exact_size(40.0)
            .resizable(false)
            .show(ui, |ui| panels::rail(ui, left_panel))
            .inner;
        if let Some(p) = clicked {
            left_panel = app_state::prefs::toggled(left_panel, p);
        }
        if let Some(p) = left_panel {
            let r = egui::Panel::left("painel-esquerdo")
                .resizable(true)
                .default_size(left_width)
                .size_range(MIN_PANEL..=MAX_PANEL)
                .show(ui, |ui| doc.side_panel(ui, p));
            left_width = r.response.rect.width();
        }
        let comments = doc.comment_tools;
        if comments {
            // com Comentar ligado, a direita mostra o painel Comentários (como no Acrobat)
            let r = egui::Panel::right("painel-comentarios")
                .resizable(true)
                .default_size(tools_width.max(280.0))
                .size_range(MIN_PANEL..=MAX_PANEL)
                .show(ui, |ui| doc.comments_panel(ui));
            tools_width = r.response.rect.width();
        }
        // o Organizar páginas usa a largura toda (como no Acrobat)
        let tool_id = (self.prefs.tools_panel && !comments && doc.organize.is_none()).then(|| {
            let r = egui::Panel::right("painel-ferramentas")
                .resizable(true)
                .default_size(tools_width)
                .size_range(MIN_PANEL..=MAX_PANEL)
                .show(ui, tools_hub::side_list);
            tools_width = r.response.rect.width();
            r.inner
        });
        doc.show_canvas(ui);
        let width_changed = |a: f32, b: f32| (a - b).abs() > 0.5;
        if left_panel != self.prefs.left_panel
            || width_changed(left_width, self.prefs.left_width)
            || width_changed(tools_width, self.prefs.tools_width)
        {
            self.prefs.left_panel = left_panel;
            self.prefs.left_width = left_width;
            self.prefs.tools_width = tools_width;
            self.prefs_dirty = true;
        }
        if let Some(id) = tool_id.flatten() {
            self.run_tool(ui.ctx(), id);
        }
        if let Some(cmd) = cmd {
            self.run(ui.ctx(), cmd);
        }
    }

    fn menu_state(&self) -> MenuState<'_> {
        let view = self
            .tabs
            .doc_index(self.tabs.active())
            .and_then(|i| self.docs[i].view.as_ref());
        MenuState {
            doc_mode: view.map(|v| v.mode),
            doc_cover: view.is_some_and(|v| v.cover),
            dark: self.prefs.dark,
            left: self.prefs.left_panel,
            tools_panel: self.prefs.tools_panel,
            recents: &self.recents,
            tabs: [
                (HOME, "Início".to_string()),
                (TOOLS, "Ferramentas".to_string()),
            ]
            .into_iter()
            .chain(
                self.docs
                    .iter()
                    .enumerate()
                    .map(|(n, d)| (FIXED + n, d.title())),
            )
            .collect(),
            active: self.tabs.active(),
        }
    }

    /// Avança o fechamento em andamento: pergunta, espera o salvamento ou fecha.
    fn drive_close(&mut self, ui: &mut egui::Ui) {
        let Some(flow) = self.closing.as_ref() else {
            return;
        };
        if flow.is_done() {
            let quit = flow.quit();
            self.closing = None;
            if quit {
                self.allow_close = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let Some(path) = flow.current().cloned() else {
            return;
        };
        let saving = flow.is_saving();
        let tab = self.tabs.find(&path);
        let step = if saving {
            let ok = match tab.and_then(|t| self.doc_at(t)) {
                Some(doc) if doc.edit.saving => return, // esperando o worker
                Some(doc) => !doc.is_dirty(),
                None => true,
            };
            self.closing.as_mut().map(|f| f.saved(ok))
        } else {
            if let Some(t) = tab.filter(|&t| t != self.tabs.active()) {
                self.activate(t); // mostra o documento sobre o qual se pergunta
            }
            match close::ask(ui, &tab_title(&path, 60)) {
                Some(answer) => self.closing.as_mut().map(|f| f.answer(answer)),
                None => return,
            }
        };
        match step {
            Some(Step::Save(p)) => {
                if let Some(doc) = self.tabs.find(&p).and_then(|t| self.doc_at(t)) {
                    doc.save();
                }
            }
            Some(Step::Close(p)) => {
                if let Some(t) = self.tabs.find(&p) {
                    self.close_tab(t);
                }
            }
            Some(Step::Abort) | None => self.closing = None,
        }
    }

    fn tab_labels(&self) -> Vec<TabLabel> {
        self.docs
            .iter()
            .map(|d| TabLabel {
                title: d.title(),
                tooltip: d.path.display().to_string(),
                dirty: d.is_dirty(),
            })
            .collect()
    }

    /// Cartão ou item de ferramenta clicado.
    fn run_tool(&mut self, ctx: &egui::Context, id: &str) {
        if id == tools_catalog::COMBINE {
            self.combine.open = true;
            return;
        }
        if id == tools_catalog::COMPARE {
            // o documento da aba ativa entra como antigo (como no Acrobat)
            let active = self.active_path();
            self.compare.start_with(active);
            return;
        }
        if id == tools_catalog::CREATE {
            self.create_after_pick = true;
            self.dialogs.pick_images(ctx);
            return;
        }
        if id == tools_catalog::OCR {
            match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
                Some(tab) => {
                    self.activate(tab);
                    if let Some(doc) = self.active_doc() {
                        doc.open_ocr();
                    }
                }
                None => self.status = "Abra um PDF digitalizado para reconhecer o texto.".into(),
            }
            return;
        }
        if id == tools_catalog::EXPORT {
            match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
                Some(tab) => {
                    self.activate(tab);
                    if let Some(doc) = self.active_doc() {
                        doc.open_export();
                    }
                }
                None => self.status = "Abra um PDF para exportar.".into(),
            }
            return;
        }
        if id == tools_catalog::COMPRESS {
            match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
                Some(tab) => {
                    self.activate(tab);
                    if let Some(doc) = self.active_doc() {
                        doc.open_compress();
                    }
                }
                None => self.status = "Abra um PDF para comprimir.".into(),
            }
            return;
        }
        if matches!(
            id,
            tools_catalog::ORGANIZE
                | tools_catalog::INSERT
                | tools_catalog::CROP
                | tools_catalog::SPLIT
        ) {
            match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
                Some(tab) => {
                    self.activate(tab);
                    if let Some(doc) = self.active_doc() {
                        doc.open_organize(ctx, id);
                    }
                }
                None => self.status = "Abra um PDF para organizar as páginas.".into(),
            }
            return;
        }
        let row = match id {
            tools_catalog::COMMENT => tools_catalog::COMMENT,
            tools_catalog::FILL_SIGN => tools_catalog::FILL_SIGN,
            tools_catalog::PREPARE_FORM => tools_catalog::PREPARE_FORM,
            tools_catalog::EDIT_PDF => tools_catalog::EDIT_PDF,
            tools_catalog::CERTIFICATES => tools_catalog::CERTIFICATES,
            tools_catalog::PROTECT => tools_catalog::PROTECT,
            tools_catalog::WATERMARK => tools_catalog::WATERMARK,
            tools_catalog::REDACT => tools_catalog::REDACT,
            tools_catalog::HEADER_FOOTER => tools_catalog::HEADER_FOOTER,
            _ => return,
        };
        match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
            Some(tab) => {
                self.activate(tab);
                if let Some(doc) = self.active_doc() {
                    doc.show_row(row);
                }
            }
            None => {
                self.row_after_open = Some(row);
                self.dialogs.open(ctx);
            }
        }
    }
}

impl Drop for GeraApp {
    /// O app fechando: grava o que ainda estava pendente (o `saver` espera terminar).
    fn drop(&mut self) {
        self.persist();
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
        if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
            let doc = &mut self.docs[i];
            if let (Some(bench), Some(view)) = (self.bench.as_mut(), doc.view.as_mut()) {
                if let Some(y) = bench.scroll_step(ctx, view) {
                    doc.jump_to = Some(y);
                }
            }
        }
        // Fechar a janela com alterações pendentes (em qualquer aba) pergunta antes.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.docs.iter().any(DocTab::needs_confirm)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.closing.is_none() {
                let all = self.tabs.docs().to_vec();
                self.request_close(ctx, all, true);
            }
        }
        self.persist_when_idle(ctx);
        if self.closing.is_none() {
            for cmd in commands::from_keyboard(ctx, self.prefs.single_key) {
                self.run(ctx, cmd);
            }
            // Esc sai da tela cheia, depois do modo leitura
            if (self.fullscreen || self.reading)
                && !ctx.egui_wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                let exit = if self.fullscreen {
                    Command::FullScreen
                } else {
                    Command::ReadMode
                };
                self.run(ctx, exit);
            }
            if let Some(doc) = self.active_doc() {
                doc.handle_input(ctx);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.fullscreen {
            let state = self.menu_state();
            let picked = egui::Panel::top("menus")
                .show(ui, |ui| menus::menu_bar(ui, &state))
                .inner;
            if let Some(cmd) = picked {
                self.run(ui.ctx(), cmd);
            }
            let labels = self.tab_labels();
            let action = egui::Panel::top("abas")
                .show(ui, |ui| tab_bar::show(ui, self.tabs.active(), &labels))
                .inner;
            match action {
                Some(TabAction::Activate(t)) => self.activate(t),
                Some(TabAction::Close(t)) => {
                    if let Some(path) = self.tabs.doc_path(t).map(Path::to_path_buf) {
                        self.request_close(ui.ctx(), vec![path], false);
                    }
                }
                Some(TabAction::New) => self.dialogs.open(ui.ctx()),
                None => {}
            }
        }
        match self.tabs.active() {
            HOME => {
                let action = egui::CentralPanel::default()
                    .show(ui, |ui| {
                        home::show(ui, &self.recents, &self.recent_exists, &self.status)
                    })
                    .inner;
                match action {
                    Some(HomeAction::OpenDialog) => self.dialogs.open(ui.ctx()),
                    Some(HomeAction::Open(p)) => self.open_path(ui.ctx(), p),
                    Some(HomeAction::Forget(p)) => self.forget(&p),
                    None => {}
                }
            }
            TOOLS => {
                let picked = egui::CentralPanel::default().show(ui, tools_hub::hub).inner;
                if let Some(id) = picked {
                    self.run_tool(ui.ctx(), id);
                }
            }
            _ if !self.fullscreen => self.document_view(ui),
            _ => {
                let now = ui.input(|i| i.time);
                if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                    self.last_motion = now;
                }
                let moving = now - self.last_motion < 2.0;
                let mut cmd = None;
                if let Some(doc) = self.active_doc() {
                    doc.begin(); // tela cheia: só as páginas
                    doc.show_canvas(ui);
                    if moving {
                        cmd = crate::shell::reading::bar(ui.ctx(), doc, true);
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_secs(2));
                    }
                }
                if let Some(cmd) = cmd {
                    self.run(ui.ctx(), cmd);
                }
            }
        }
        self.drive_close(ui);
        help::shortcuts_window(ui, &mut self.show_shortcuts);
        help::about_window(ui, &mut self.show_about);
        self.combine.open_docs = self.docs.iter().map(|d| d.path.clone()).collect();
        if let Some(merged) = self.combine.show(ui) {
            self.open_path(ui.ctx(), merged);
        }
        if self.compare.open {
            self.compare.open_docs = self.docs.iter().map(|d| d.path.clone()).collect();
            self.compare.dirty_docs = self
                .docs
                .iter()
                .filter(|d| d.is_dirty())
                .map(|d| d.path.clone())
                .collect();
        }
        if let Some(result) = self.compare.show(ui) {
            // a aba do resultado abre com a linha Comparação (o PDF diz que é uma comparação)
            self.open_path(ui.ctx(), result);
        }
        self.link_warning_window(ui.ctx());
        self.preferences_window(ui.ctx());
        let full = self
            .full_screen_request
            .as_mut()
            .and_then(|r| r.show(ui.ctx()));
        if let Some(yes) = full {
            self.full_screen_request = None;
            if yes && !self.fullscreen {
                self.run(ui.ctx(), Command::FullScreen);
            }
        }
    }
}

impl GeraApp {
    /// Preferências: OK aplica (tema e nome passam às abas), Cancelar descarta.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        let Some(ok) = self.preferences.as_mut().and_then(|d| d.show(ctx)) else {
            return;
        };
        let Some(draft) = self.preferences.take() else {
            return;
        };
        if ok && draft.forget_recents {
            self.recents.clear();
            self.recents_changed();
        }
        let Some(new) = draft.finish(ok) else {
            return;
        };
        let (dark, author) = (new.dark, new.author.clone());
        let theme_changed = dark != self.prefs.dark;
        let author_changed = author != self.prefs.author;
        self.prefs = new;
        self.prefs_dirty = true;
        self.recents.limit(self.prefs.max_recents);
        self.recents_changed();
        if theme_changed {
            self.set_dark(ctx, dark);
        }
        if author_changed {
            for doc in &mut self.docs {
                doc.set_author(author.clone());
            }
        }
    }
}

impl GeraApp {
    /// Aviso de segurança: Permitir abre o endereço (ou o arquivo); a escolha pode ficar lembrada
    /// para o site.
    fn link_warning_window(&mut self, ctx: &egui::Context) {
        use app_state::link_policy::{remember, LinkDecision};
        use pdf_core::LinkTarget;
        let Some(answer) = self.link_warning.as_mut().and_then(|w| w.show(ctx)) else {
            return;
        };
        let Some(warning) = self.link_warning.take() else {
            return;
        };
        let allow = answer == LinkDecision::Allow;
        match warning.target {
            LinkTarget::Uri(url) => {
                if warning.remember {
                    remember(&mut self.prefs.link_sites, &url, answer);
                    self.prefs_dirty = true;
                }
                if allow {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
            LinkTarget::File { path, page } if allow => {
                if !path.is_file() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if let Some(doc) = self.active_doc() {
                        doc.status = format!("Arquivo não encontrado: {name}");
                    }
                    return;
                }
                self.open_path(ctx, path);
                // open_path deixa a aba do arquivo na frente
                if let (Some(page), Some(doc)) = (page, self.active_doc()) {
                    doc.show_page(page);
                }
            }
            _ => {}
        }
    }
}

fn visuals(dark: bool) -> egui::Visuals {
    if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    }
}
