//! Janela "Comparar arquivos" (como no Acrobat): o arquivo antigo e o novo, as páginas de cada um e as
//! opções; o resultado é um PDF lado a lado aberto numa aba.
use super::combine::Entry;
use crate::open_dialog::{FileDialogs, Picked};
use pdf_core::{CompareInput, CompareOptions, CompareStage};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// Qual dos dois arquivos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Old,
    New,
}

/// Um lado: o arquivo e o intervalo de páginas ("" = todas).
#[derive(Default)]
pub struct Pick {
    pub file: Option<Entry>,
    pub range: String,
}

impl Pick {
    /// As páginas escolhidas (base 0): `None` = todas. Erro: o motivo, para mostrar.
    pub fn pages(&self) -> Result<Option<Vec<i32>>, String> {
        let text = self.range.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let total = self.file.as_ref().and_then(|f| f.pages).unwrap_or(0) as usize;
        let mut out: Vec<i32> = Vec::new();
        for (a, b) in pdf_core::parse_ranges(text, total)? {
            for p in a..=b {
                let p = p as i32 - 1;
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        Ok(Some(out))
    }

    fn input(&self) -> Option<CompareInput> {
        let f = self.file.as_ref().filter(|f| f.ready())?;
        Some(CompareInput {
            path: f.path.clone(),
            password: (!f.password.is_empty()).then(|| f.password.clone()),
            pages: self.pages().ok()?,
        })
    }
}

enum Msg {
    Stage(CompareStage),
    Done(Result<PathBuf, String>),
}

struct Job {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    stage: Option<CompareStage>,
}

#[derive(Default)]
pub struct CompareWindow {
    pub open: bool,
    /// Documentos abertos nas abas: o resultado não pode ir por cima de um deles.
    pub open_docs: Vec<PathBuf>,
    /// Documentos com alterações não salvas: a comparação usa a versão do disco.
    pub dirty_docs: Vec<PathBuf>,
    pub old: Pick,
    pub new: Pick,
    pub text_only: bool,
    pub formatting: bool,
    picking: Option<Slot>,
    dialogs: FileDialogs,
    job: Option<Job>,
    pub status: String,
}

/// O que está acontecendo, para o rótulo do progresso.
pub fn stage_text(s: &CompareStage) -> String {
    match *s {
        CompareStage::ReadingOld(i, n) => {
            format!("Lendo o arquivo antigo… (página {i} de {n})")
        }
        CompareStage::ReadingNew(i, n) => format!("Lendo o arquivo novo… (página {i} de {n})"),
        CompareStage::Diffing => "Comparando…".into(),
        CompareStage::Writing(i, n) => format!("Montando o resultado… ({i} de {n})"),
    }
}

/// Nome sugerido para o resultado: ao lado do arquivo novo.
pub fn suggested_dest(new: &Path) -> PathBuf {
    let stem = new
        .file_stem()
        .map_or_else(|| "documento".into(), |s| s.to_string_lossy().into_owned());
    new.with_file_name(format!("Comparação de {stem}.pdf"))
}

impl CompareWindow {
    /// Abre a janela; o documento da aba ativa entra como antigo (se ainda não houver um).
    pub fn start_with(&mut self, active: Option<PathBuf>) {
        self.open = true;
        if self.old.file.is_none() {
            if let Some(p) = active {
                self.old.file = Some(Entry::new(p));
            }
        }
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.old, &mut self.new);
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// As opções da comparação, quando os dois lados estão prontos e os intervalos valem.
    pub fn options(&self) -> Option<CompareOptions> {
        Some(CompareOptions {
            old: self.old.input()?,
            new: self.new.input()?,
            text_only: self.text_only,
            formatting: self.formatting,
        })
    }

    /// Algum dos dois arquivos tem alterações não salvas.
    fn uses_saved_version(&self) -> bool {
        [&self.old, &self.new].iter().any(|p| {
            p.file
                .as_ref()
                .is_some_and(|f| self.dirty_docs.contains(&f.path))
        })
    }

    /// Desenha a janela; devolve o resultado gravado (para abrir numa aba).
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let done = self.poll_job();
        match self.dialogs.poll() {
            Some(Picked::Open(paths)) => {
                if let (Some(slot), Some(p)) = (self.picking.take(), paths.into_iter().next()) {
                    let pick = match slot {
                        Slot::Old => &mut self.old,
                        Slot::New => &mut self.new,
                    };
                    pick.file = Some(Entry::new(p));
                    pick.range.clear();
                }
            }
            Some(Picked::SaveTo(dest)) => self.start(ui.ctx(), dest),
            Some(Picked::Failed) => {
                self.status = "Não foi possível abrir o seletor de arquivos.".into()
            }
            _ => {}
        }
        if !self.open {
            return done;
        }
        let mut open = self.open;
        egui::Window::new("Comparar arquivos")
            .open(&mut open)
            .default_size([620.0, 300.0])
            .show(ui, |ui| self.contents(ui));
        self.open = open && self.open;
        if !self.open {
            self.cancel();
        }
        done
    }

    fn poll_job(&mut self) -> Option<PathBuf> {
        let job = self.job.as_mut()?;
        loop {
            match job.rx.try_recv() {
                Ok(Msg::Stage(s)) => job.stage = Some(s),
                Ok(Msg::Done(r)) => {
                    self.job = None;
                    return match r {
                        Ok(path) => {
                            self.status.clear();
                            self.open = false;
                            Some(path)
                        }
                        Err(e) if e.contains("cancelada") => {
                            self.status = "Comparação cancelada.".into();
                            None
                        }
                        Err(e) => {
                            self.status = format!("Não foi possível comparar: {e}");
                            None
                        }
                    };
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = "Não foi possível comparar.".into();
                    return None;
                }
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn side(&mut self, ui: &mut egui::Ui, slot: Slot) {
        let busy = self.busy();
        let pick = match slot {
            Slot::Old => &mut self.old,
            Slot::New => &mut self.new,
        };
        ui.strong(match slot {
            Slot::Old => "Arquivo antigo",
            Slot::New => "Arquivo novo",
        });
        match &mut pick.file {
            Some(e) => {
                let name = e
                    .path
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                ui.label(name).on_hover_text(e.path.display().to_string());
                match (&e.pages, &e.error) {
                    (Some(p), _) => {
                        ui.label(if *p == 1 {
                            "1 página".to_string()
                        } else {
                            format!("{p} páginas")
                        });
                    }
                    (None, Some(err)) => {
                        ui.colored_label(ui.visuals().warn_fg_color, err.as_str());
                    }
                    _ => {}
                }
                if e.needs_password && e.pages.is_none() {
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut e.password)
                                .password(true)
                                .hint_text("Senha")
                                .desired_width(120.0),
                        );
                        if field.lost_focus() || ui.button("OK").clicked() {
                            e.probe();
                        }
                    });
                }
            }
            None => {
                ui.weak("Nenhum arquivo escolhido");
            }
        }
        if ui
            .add_enabled(!busy, egui::Button::new("Escolher arquivo…"))
            .clicked()
        {
            self.picking = Some(slot);
            self.dialogs.open(ui.ctx());
        }
        let pick = match slot {
            Slot::Old => &mut self.old,
            Slot::New => &mut self.new,
        };
        ui.horizontal(|ui| {
            ui.label("Páginas:");
            ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut pick.range)
                    .hint_text("Todas (ou 1-3, 7)")
                    .desired_width(140.0),
            );
        });
        if let Err(e) = pick.pages() {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        ui.columns(2, |cols| {
            self.side(&mut cols[0], Slot::Old);
            self.side(&mut cols[1], Slot::New);
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("⇄ Trocar"))
                .on_hover_text("Trocar o arquivo antigo pelo novo")
                .clicked()
            {
                self.swap();
            }
            ui.add_enabled(
                !busy,
                egui::Checkbox::new(&mut self.text_only, "Comparar somente texto"),
            );
            ui.add_enabled(
                !busy,
                egui::Checkbox::new(&mut self.formatting, "Incluir formatação"),
            );
        });
        if self.uses_saved_version() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Há alterações não salvas: a comparação usa a versão salva no disco.",
            );
        }
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).color(ui.visuals().warn_fg_color));
        }
        ui.horizontal(|ui| match &self.job {
            Some(job) => {
                ui.spinner();
                ui.label(
                    job.stage
                        .as_ref()
                        .map_or_else(|| "Comparando…".into(), stage_text),
                );
                if ui.button("Cancelar").clicked() {
                    self.cancel();
                }
            }
            None => {
                let ready = self.options();
                if ui
                    .add_enabled(ready.is_some(), egui::Button::new("Comparar"))
                    .clicked()
                {
                    if let Some(o) = ready {
                        self.dialogs.save_to(ui.ctx(), suggested_dest(&o.new.path));
                    }
                }
            }
        });
    }

    /// Compara numa thread e grava em `dest` (arquivos grandes não travam a janela).
    pub fn start(&mut self, ctx: &egui::Context, dest: PathBuf) {
        let same = |p: &PathBuf| p.canonicalize().ok() == dest.canonicalize().ok() || *p == dest;
        if self.open_docs.iter().any(same) {
            self.status =
                "Esse arquivo está aberto numa aba: feche-o ou escolha outro nome.".into();
            return;
        }
        let Some(opts) = self.options() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("comparar".into())
            .spawn(move || {
                let r = pdf_core::compare_files(&opts, &dest, &mut |s| {
                    let _ = tx.send(Msg::Stage(s));
                    ctx.request_repaint();
                    !stop.load(Ordering::Relaxed)
                })
                .map(|_| dest)
                .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Done(r));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.job = Some(Job {
                rx,
                cancel,
                stage: None,
            });
            self.status.clear();
        } else {
            self.status = "Não foi possível iniciar a comparação.".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(dir: &Path, name: &str, pages: usize) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, pdf_core::sample::sample_pdf(pages, 0)).unwrap();
        p
    }

    fn window(dir: &Path) -> CompareWindow {
        let mut w = CompareWindow::default();
        w.start_with(Some(sample(dir, "a.pdf", 3)));
        w.new.file = Some(Entry::new(sample(dir, "b.pdf", 2)));
        w
    }

    #[test]
    fn active_document_comes_in_as_old() {
        let dir = tempfile::tempdir().unwrap();
        let w = window(dir.path());
        assert!(w.open);
        assert_eq!(w.old.file.as_ref().unwrap().path, dir.path().join("a.pdf"));
        assert!(w.options().is_some());
    }

    #[test]
    fn swap_swaps() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        w.old.range = "1-2".into();
        w.swap();
        assert_eq!(w.old.file.as_ref().unwrap().path, dir.path().join("b.pdf"));
        assert_eq!(w.new.range, "1-2");
    }

    #[test]
    fn ranges_become_pages() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        w.old.range = "3, 1-2, 2".into();
        assert_eq!(w.old.pages(), Ok(Some(vec![2, 0, 1])));
        assert_eq!(w.options().unwrap().old.pages, Some(vec![2, 0, 1]));
    }

    #[test]
    fn bad_range_blocks_compare() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        w.new.range = "5".into();
        assert!(w.new.pages().is_err());
        assert!(w.options().is_none());
    }

    #[test]
    fn missing_file_blocks_compare() {
        let mut w = CompareWindow::default();
        w.start_with(None);
        assert!(w.options().is_none());
    }

    #[test]
    fn dest_open_in_tab_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        let open = dir.path().join("aberto.pdf");
        std::fs::write(&open, b"x").unwrap();
        w.open_docs = vec![open.clone()];
        w.start(&egui::Context::default(), open);
        assert!(!w.busy());
        assert!(w.status.contains("aberto numa aba"));
    }

    #[test]
    fn unsaved_changes_are_announced() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        assert!(!w.uses_saved_version());
        w.dirty_docs = vec![dir.path().join("a.pdf")];
        assert!(w.uses_saved_version());
    }

    #[test]
    fn result_is_suggested_next_to_the_new_file() {
        assert_eq!(
            suggested_dest(Path::new("/x/contrato v2.pdf")),
            PathBuf::from("/x/Comparação de contrato v2.pdf")
        );
    }

    #[test]
    fn stages_read_like_progress() {
        assert_eq!(
            stage_text(&CompareStage::ReadingNew(2, 5)),
            "Lendo o arquivo novo… (página 2 de 5)"
        );
    }

    #[test]
    fn comparison_runs_and_reports_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = window(dir.path());
        let dest = dir.path().join("r.pdf");
        w.start(&egui::Context::default(), dest.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let got = loop {
            if let Some(p) = w.poll_job() {
                break Some(p);
            }
            if !w.busy() || std::time::Instant::now() > deadline {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(got, Some(dest.clone()), "{}", w.status);
        assert!(dest.exists());
    }
}
