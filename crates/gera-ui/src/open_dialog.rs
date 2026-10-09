//! Seletor de arquivos do sistema (portal XDG) numa thread à parte, para não travar a janela.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Debug)]
pub enum Picked {
    Open(Vec<PathBuf>),
    /// "Salvar como" do documento `doc` para `dest`.
    SaveAs {
        doc: PathBuf,
        dest: PathBuf,
    },
    Cancelled,
    /// O seletor não pôde ser aberto.
    Failed,
}

#[derive(Default)]
pub struct FileDialogs {
    pending: Option<Receiver<Picked>>,
}

impl FileDialogs {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn open(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            match rfd::FileDialog::new()
                .set_title("Abrir PDF")
                .add_filter("PDF", &["pdf", "PDF"])
                .pick_files()
            {
                Some(files) if !files.is_empty() => Picked::Open(files),
                _ => Picked::Cancelled,
            }
        });
    }

    pub fn save_as(&mut self, ctx: &egui::Context, doc: PathBuf) {
        self.spawn(ctx, move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Salvar como")
                .add_filter("PDF", &["pdf"]);
            if let Some(dir) = doc.parent() {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = doc.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            match dialog.save_file() {
                Some(dest) => Picked::SaveAs { doc, dest },
                None => Picked::Cancelled,
            }
        });
    }

    fn spawn(&mut self, ctx: &egui::Context, pick: impl FnOnce() -> Picked + Send + 'static) {
        if self.busy() {
            return; // um seletor por vez
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("seletor-arquivos".into())
            .spawn(move || {
                let _ = tx.send(pick());
                ctx.request_repaint();
            });
        // se a thread nem começou, o canal fecha sem resposta e `poll` devolve `Failed`
        drop(started);
        self.pending = Some(rx);
    }

    /// Resultado do seletor, quando houver. Uma thread que morre sem responder conta como falha.
    pub fn poll(&mut self) -> Option<Picked> {
        let rx = self.pending.as_ref()?;
        let picked = match rx.try_recv() {
            Ok(p) => p,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Picked::Failed,
        };
        self.pending = None;
        Some(picked)
    }
}
