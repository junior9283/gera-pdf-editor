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
    /// Pasta escolhida (Dividir: onde gravar as partes).
    Folder(PathBuf),
    /// Arquivo novo a gravar (Combinar).
    SaveTo(PathBuf),
    Cancelled,
    /// O seletor não pôde ser aberto.
    Failed,
}

/// Imagens que o MuPDF lê (Criar PDF, Editar PDF).
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif"];

/// "Salvar como" de PDF partindo de `suggested` (pasta e nome); sem extensão, ganha ".pdf".
fn save_dialog(suggested: &std::path::Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Salvar como")
        .add_filter("PDF", &["pdf"]);
    if let Some(dir) = suggested.parent() {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    dialog.save_file().map(|p| {
        if p.extension().is_some() {
            p
        } else {
            p.with_extension("pdf")
        }
    })
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
        self.spawn(ctx, move || match save_dialog(&doc) {
            Some(dest) => Picked::SaveAs { doc, dest },
            None => Picked::Cancelled,
        });
    }

    /// Imagens para o Criar PDF (várias).
    pub fn pick_images(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            match rfd::FileDialog::new()
                .set_title("Criar PDF a partir de imagens")
                .add_filter("Imagens", IMAGE_EXTS)
                .pick_files()
            {
                Some(p) if !p.is_empty() => Picked::Open(p),
                _ => Picked::Cancelled,
            }
        });
    }

    /// Arquivo de ID digital (PKCS#12).
    pub fn pick_id_file(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            match rfd::FileDialog::new()
                .set_title("Importar ID digital")
                .add_filter("ID digital", &["pfx", "p12"])
                .pick_file()
            {
                Some(p) => Picked::Open(vec![p]),
                None => Picked::Cancelled,
            }
        });
    }

    /// Uma imagem (assinatura: PNG ou JPEG; Editar PDF: os formatos que o Criar PDF aceita).
    pub fn pick_image(
        &mut self,
        ctx: &egui::Context,
        title: &'static str,
        exts: &'static [&'static str],
    ) {
        self.spawn(ctx, move || {
            match rfd::FileDialog::new()
                .set_title(title)
                .add_filter("Imagem", exts)
                .pick_file()
            {
                Some(p) => Picked::Open(vec![p]),
                None => Picked::Cancelled,
            }
        });
    }

    pub fn pick_folder(&mut self, ctx: &egui::Context, start: Option<PathBuf>) {
        self.spawn(ctx, move || {
            let mut dialog = rfd::FileDialog::new().set_title("Escolher pasta");
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            dialog
                .pick_folder()
                .map_or(Picked::Cancelled, Picked::Folder)
        });
    }

    /// Onde gravar um PDF novo, sugerindo `suggested`.
    pub fn save_to(&mut self, ctx: &egui::Context, suggested: PathBuf) {
        self.spawn(ctx, move || {
            save_dialog(&suggested).map_or(Picked::Cancelled, Picked::SaveTo)
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
