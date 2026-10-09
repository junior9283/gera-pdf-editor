use crate::bench::Bench;
use crate::doc_tab::DocTab;
use crate::open_dialog::{FileDialogs, Picked};
use crate::shell::close::{self, CloseFlow, Step};
use crate::shell::commands::{self, Command};
use crate::shell::menus::{self, MenuState};
use crate::shell::tabs::{self as tab_bar, TabAction, TabLabel};
use crate::shell::{help, toolbar};
use app_state::prefs::Prefs;
use app_state::recents::{now_secs, RecentFiles};
use app_state::store;
use app_state::tabs::{canonical, tab_title, TabList, FIXED, HOME, TOOLS};
use std::path::PathBuf;

const RECENTS_FILE: &str = "recentes.json";
const PREFS_FILE: &str = "preferencias.json";

pub struct GeraApp {
    tabs: TabList,
    /// Um por aba de documento, na mesma ordem de `tabs.docs()`.
    docs: Vec<DocTab>,
    dialogs: FileDialogs,
    recents: RecentFiles,
    prefs: Prefs,
    /// Mensagem da casca (falhas do seletor ou de abertura), mostrada no Início.
    status: String,
    closing: Option<CloseFlow>,
    /// Saída já confirmada: não interceptar o fechamento da janela de novo.
    allow_close: bool,
    fullscreen: bool,
    show_shortcuts: bool,
    show_about: bool,
    bench: Option<Bench>,
}

impl GeraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<PathBuf>,
        bench: Option<Bench>,
    ) -> Self {
        let prefs = store::config_file(PREFS_FILE).map_or_else(Prefs::default, |p| Prefs::load(&p));
        cc.egui_ctx.set_visuals(if prefs.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let mut app = Self {
            tabs: TabList::default(),
            docs: Vec::new(),
            dialogs: FileDialogs::default(),
            recents: store::config_file(RECENTS_FILE)
                .map(|p| RecentFiles::load(&p))
                .unwrap_or_default(),
            prefs,
            status: String::new(),
            closing: None,
            allow_close: false,
            fullscreen: false,
            show_shortcuts: false,
            show_about: false,
            bench,
        };
        for path in paths {
            app.open_path(&cc.egui_ctx, path);
        }
        app
    }

    pub(crate) fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = canonical(&path);
        let before = self.tabs.active();
        let (tab, created) = self.tabs.open(path.clone());
        if created {
            let mut doc = DocTab::new(ctx, path.clone(), self.prefs.dark);
            doc.open(path);
            self.docs.push(doc);
        }
        self.tabs.activate(before); // `activate` abaixo suspende a aba que estava na frente
        self.activate(tab);
    }

    fn activate(&mut self, tab: usize) {
        let old = self.tabs.active();
        self.tabs.activate(tab);
        if old != self.tabs.active() {
            if let Some(doc) = self.doc_at(old) {
                doc.suspend();
            }
        }
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
            self.docs.remove(i); // o Worker cai junto e a thread termina
        }
    }

    /// Fecha as abas de `paths`, perguntando antes pelas que têm alterações pendentes.
    fn request_close(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>, quit: bool) {
        let mut dirty = Vec::new();
        for path in paths {
            let Some(tab) = self.tabs.find(&path) else {
                continue;
            };
            if self
                .doc_at(tab)
                .is_some_and(|d| d.is_dirty() || d.edit.saving)
            {
                dirty.push(path);
            } else {
                self.close_tab(tab);
            }
        }
        if !dirty.is_empty() {
            self.closing = Some(CloseFlow::new(dirty, quit));
        } else if quit {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn remember(&mut self, path: PathBuf) {
        self.recents.add(path, now_secs());
        if let Some(file) = store::config_file(RECENTS_FILE) {
            if let Err(e) = self.recents.save(&file) {
                self.status = format!("Não foi possível gravar os recentes: {e}");
            }
        }
    }

    /// Respostas dos workers e do seletor de arquivos.
    fn poll(&mut self, ctx: &egui::Context) {
        let active = self.tabs.active();
        for tab in (FIXED..self.tabs.len()).rev() {
            let Some(i) = self.tabs.doc_index(tab) else {
                continue;
            };
            let bench = if tab == active {
                self.bench.as_mut()
            } else {
                None
            };
            let doc = &mut self.docs[i];
            doc.poll(ctx, bench);
            let (opened, failure, renamed) =
                (doc.take_opened(), doc.take_failure(), doc.take_renamed());
            let path = doc.path.clone();
            if opened {
                self.remember(path);
            }
            if let Some(dest) = renamed {
                let dest = canonical(&dest);
                self.tabs.rename(tab, dest.clone());
                self.docs[i].path = dest.clone();
                self.remember(dest);
            }
            if let Some(msg) = failure {
                self.status = msg;
                self.close_tab(tab);
            }
        }
        if let Some(picked) = self.dialogs.poll() {
            match picked {
                Picked::Open(files) => {
                    for f in files {
                        self.open_path(ctx, f);
                    }
                }
                Picked::SaveAs { doc, dest } => self.save_as(doc, dest),
                Picked::Cancelled => {}
                Picked::Failed => {
                    self.status = "Não foi possível abrir o seletor de arquivos.".into()
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
                if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
                    let path = self.tabs.docs()[i].clone();
                    self.dialogs.save_as(ctx, path);
                }
            }
            Command::CloseTab => {
                if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
                    let path = self.tabs.docs()[i].clone();
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
                    doc.run(c);
                }
            }
            Command::ToggleDark => self.set_dark(ctx, !self.prefs.dark),
            Command::FullScreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Command::NextTab | Command::PrevTab => {
                let old = self.tabs.active();
                if cmd == Command::NextTab {
                    self.tabs.next();
                } else {
                    self.tabs.prev();
                }
                let new = self.tabs.active();
                self.tabs.activate(old);
                self.activate(new);
            }
            Command::ActivateTab(t) => self.activate(t),
            Command::Shortcuts => self.show_shortcuts = true,
            Command::About => self.show_about = true,
        }
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.prefs.dark = dark;
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        for doc in &mut self.docs {
            doc.set_dark(dark);
        }
        if let Some(file) = store::config_file(PREFS_FILE) {
            if let Err(e) = store::save_json(&file, &self.prefs) {
                self.status = format!("Não foi possível gravar as preferências: {e}");
            }
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

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("Gera PDF Editor");
            ui.add_space(16.0);
            let open = egui::Button::new("📂  Abrir arquivo…").min_size(egui::vec2(220.0, 40.0));
            if ui.add(open).clicked() {
                self.dialogs.open(ui.ctx());
            }
            if !self.status.is_empty() {
                ui.add_space(12.0);
                ui.label(&self.status);
            }
        });
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
            && self.docs.iter().any(|d| d.is_dirty() || d.edit.saving)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.closing.is_none() {
                let all = self.tabs.docs().to_vec();
                self.request_close(ctx, all, true);
            }
        }
        if self.closing.is_none() {
            for cmd in commands::from_keyboard(ctx) {
                self.run(ctx, cmd);
            }
            if self.fullscreen
                && !ctx.egui_wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.run(ctx, Command::FullScreen);
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
                    if let Some(i) = self.tabs.doc_index(t) {
                        let path = self.tabs.docs()[i].clone();
                        self.request_close(ui.ctx(), vec![path], false);
                    }
                }
                Some(TabAction::New) => self.dialogs.open(ui.ctx()),
                None => {}
            }
        }
        match self.tabs.active() {
            HOME => {
                egui::CentralPanel::default().show(ui, |ui| self.home(ui));
            }
            TOOLS => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.heading("Ferramentas");
                });
            }
            _ if !self.fullscreen => {
                let mut cmd = None;
                if let Some(doc) = self.active_doc() {
                    cmd = egui::Panel::top("ferramentas-doc")
                        .show(ui, |ui| toolbar::show(ui, doc))
                        .inner;
                    if doc.comment_tools {
                        cmd = egui::Panel::top("comentar")
                            .show(ui, |ui| toolbar::comment_row(ui, doc))
                            .inner
                            .or(cmd);
                    }
                    doc.show(ui, true);
                }
                if let Some(cmd) = cmd {
                    self.run(ui.ctx(), cmd);
                }
            }
            _ => {
                if let Some(doc) = self.active_doc() {
                    doc.show(ui, false); // tela cheia: só as páginas
                }
            }
        }
        self.drive_close(ui);
        help::shortcuts_window(ui, &mut self.show_shortcuts);
        help::about_window(ui, &mut self.show_about);
    }
}
