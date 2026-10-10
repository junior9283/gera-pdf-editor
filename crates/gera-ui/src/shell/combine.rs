//! Janela "Combinar arquivos" (como no Acrobat): lista de PDFs para juntar num só, na ordem.
use crate::open_dialog::{FileDialogs, Picked};
use pdf_core::{PdfDoc, PdfError};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Um arquivo da lista.
pub struct Entry {
    pub path: PathBuf,
    /// Páginas (quando já deu para abrir).
    pub pages: Option<i32>,
    pub bytes: u64,
    pub needs_password: bool,
    pub password: String,
    pub error: Option<String>,
}

impl Entry {
    /// Lê o que dá para mostrar: páginas, tamanho e se pede senha.
    pub fn new(path: PathBuf) -> Self {
        let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        let mut e = Self {
            path,
            pages: None,
            bytes,
            needs_password: false,
            password: String::new(),
            error: None,
        };
        e.probe();
        e
    }

    pub fn probe(&mut self) {
        let pw = (!self.password.is_empty()).then_some(self.password.as_str());
        match PdfDoc::open(&self.path, pw).and_then(|d| d.page_count()) {
            Ok(n) => {
                self.pages = Some(n);
                self.error = None;
            }
            Err(e @ (PdfError::PasswordRequired | PdfError::WrongPassword)) => {
                self.needs_password = true;
                let msg = if matches!(e, PdfError::WrongPassword) {
                    "Senha incorreta"
                } else {
                    "Protegido por senha"
                };
                self.error = Some(msg.into());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn ready(&self) -> bool {
        self.pages.is_some()
    }
}

/// Tamanho legível ("850 KB", "1,2 MB").
pub fn size_text(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0).replace('.', ",")
    }
}

/// Botões de cada linha.
#[derive(Clone, Copy)]
enum RowAction {
    Up,
    Down,
    Remove,
}

#[derive(Default)]
pub struct CombineWindow {
    pub open: bool,
    /// Documentos abertos nas abas: o resultado não pode ir por cima de um deles.
    pub open_docs: Vec<PathBuf>,
    pub files: Vec<Entry>,
    dialogs: FileDialogs,
    job: Option<Receiver<Result<PathBuf, String>>>,
    pub status: String,
}

impl CombineWindow {
    /// Desenha a janela; devolve o arquivo combinado (para abrir numa aba).
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut done = None;
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    self.job = None;
                    self.status.clear();
                    self.files.clear();
                    self.open = false;
                    done = Some(path);
                }
                Ok(Err(e)) => {
                    self.job = None;
                    self.status = format!("Não foi possível combinar: {e}");
                }
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = "Não foi possível combinar.".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        match self.dialogs.poll() {
            Some(Picked::Open(paths)) => self.files.extend(paths.into_iter().map(Entry::new)),
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
        egui::Window::new("Combinar arquivos")
            .open(&mut open)
            .default_size([560.0, 380.0])
            .show(ui, |ui| self.contents(ui));
        self.open = open && self.open;
        done
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let busy = self.job.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Adicionar arquivos…"))
                .clicked()
            {
                self.dialogs.open(ui.ctx());
            }
            let ok = !self.files.is_empty() && self.files.iter().all(Entry::ready) && !busy;
            if ui.add_enabled(ok, egui::Button::new("Combinar")).clicked() {
                let first = &self.files[0].path;
                self.dialogs
                    .save_to(ui.ctx(), first.with_file_name("Combinado.pdf"));
            }
            if busy {
                ui.spinner();
                ui.label("Combinando…");
            }
        });
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).color(ui.visuals().warn_fg_color));
        }
        ui.separator();
        if self.files.is_empty() {
            ui.label("Adicione os PDFs que vão virar um arquivo só, na ordem desejada.");
            return;
        }
        let mut action: Option<(usize, RowAction)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("arquivos").striped(true).show(ui, |ui| {
                ui.strong("Arquivo");
                ui.strong("Páginas");
                ui.strong("Tamanho");
                ui.label("");
                ui.end_row();
                let n = self.files.len();
                for (i, e) in self.files.iter_mut().enumerate() {
                    let name = e
                        .path
                        .file_name()
                        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                    ui.label(name).on_hover_text(e.path.display().to_string());
                    match (&e.pages, &e.error) {
                        (Some(p), _) => ui.label(p.to_string()),
                        (None, Some(err)) => ui.colored_label(ui.visuals().warn_fg_color, err),
                        _ => ui.label("—"),
                    };
                    ui.label(size_text(e.bytes));
                    ui.horizontal(|ui| {
                        if e.needs_password && e.pages.is_none() {
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut e.password)
                                    .password(true)
                                    .hint_text("Senha")
                                    .desired_width(90.0),
                            );
                            if field.lost_focus() || ui.button("OK").clicked() {
                                e.probe();
                            }
                        }
                        if ui
                            .add_enabled(!busy && i > 0, egui::Button::new("▲"))
                            .on_hover_text("Mover para cima")
                            .clicked()
                        {
                            action = Some((i, RowAction::Up));
                        }
                        if ui
                            .add_enabled(!busy && i + 1 < n, egui::Button::new("▼"))
                            .on_hover_text("Mover para baixo")
                            .clicked()
                        {
                            action = Some((i, RowAction::Down));
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Remover"))
                            .clicked()
                        {
                            action = Some((i, RowAction::Remove));
                        }
                    });
                    ui.end_row();
                }
            });
        });
        match action {
            Some((i, RowAction::Remove)) => {
                self.files.remove(i);
            }
            Some((i, RowAction::Up)) => self.files.swap(i, i - 1),
            Some((i, RowAction::Down)) => self.files.swap(i, i + 1),
            None => {}
        }
    }

    /// Combina numa thread (arquivos grandes não travam a janela).
    fn start(&mut self, ctx: &egui::Context, dest: PathBuf) {
        let same = |p: &PathBuf| p.canonicalize().ok() == dest.canonicalize().ok() || *p == dest;
        if self.open_docs.iter().any(same) {
            self.status =
                "Esse arquivo está aberto numa aba: feche-o ou escolha outro nome.".into();
            return;
        }
        let inputs: Vec<(PathBuf, Option<String>)> = self
            .files
            .iter()
            .map(|e| {
                (
                    e.path.clone(),
                    (!e.password.is_empty()).then(|| e.password.clone()),
                )
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("combinar".into())
            .spawn(move || {
                let result = pdf_core::combine(&inputs, &dest)
                    .map(|()| dest)
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.job = Some(rx);
            self.status.clear();
        } else {
            self.status = "Não foi possível iniciar a combinação.".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_the_file_manager() {
        assert_eq!(size_text(1000), "1 KB");
        assert_eq!(size_text(1_300_000), "1,2 MB");
    }

    #[test]
    fn entries_show_pages_or_the_problem() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("a.pdf");
        std::fs::write(&ok, pdf_core::sample::sample_pdf(3, 0)).unwrap();
        assert_eq!(Entry::new(ok).pages, Some(3));
        let bad = dir.path().join("b.pdf");
        std::fs::write(&bad, b"nada").unwrap();
        let e = Entry::new(bad);
        assert!(!e.ready() && e.error.is_some());
    }
}
