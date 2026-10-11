//! Seletor de arquivos do sistema (portal XDG) numa thread à parte, para não travar a janela.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Testes de interface (sem D-Bus, sem portal): no build com a feature `e2e`, com
/// `GERA_E2E_PICK=<caminho>`, todo seletor devolve esse caminho em vez de abrir. No build normal a
/// variável não vale (esquecida no ambiente, faria Salvar gravar por cima sem perguntar).
trait PickOrE2e: Sized {
    fn pick_file_or_e2e(self) -> Option<PathBuf>;
    fn pick_files_or_e2e(self) -> Option<Vec<PathBuf>>;
    fn pick_folder_or_e2e(self) -> Option<PathBuf>;
    fn save_file_or_e2e(self) -> Option<PathBuf>;
}

fn e2e_path() -> Option<PathBuf> {
    if !cfg!(feature = "e2e") {
        return None;
    }
    std::env::var_os("GERA_E2E_PICK")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl PickOrE2e for rfd::FileDialog {
    fn pick_file_or_e2e(self) -> Option<PathBuf> {
        e2e_path().or_else(|| self.pick_file())
    }
    fn pick_files_or_e2e(self) -> Option<Vec<PathBuf>> {
        e2e_path().map(|p| vec![p]).or_else(|| self.pick_files())
    }
    fn pick_folder_or_e2e(self) -> Option<PathBuf> {
        e2e_path().or_else(|| self.pick_folder())
    }
    fn save_file_or_e2e(self) -> Option<PathBuf> {
        e2e_path().or_else(|| self.save_file())
    }
}

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
    let dialog = rfd::FileDialog::new()
        .set_title("Salvar como")
        .add_filter("PDF", &["pdf"]);
    suggest(dialog, suggested).save_file_or_e2e().map(|p| {
        if p.extension().is_some() {
            p
        } else {
            p.with_extension("pdf")
        }
    })
}

/// Pasta e nome iniciais do diálogo de gravação.
fn suggest(mut dialog: rfd::FileDialog, suggested: &std::path::Path) -> rfd::FileDialog {
    if let Some(dir) = suggested.parent() {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    dialog
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
                .pick_files_or_e2e()
            {
                Some(files) if !files.is_empty() => Picked::Open(files),
                _ => Picked::Cancelled,
            }
        });
    }

    /// Salvar como: `doc` identifica a aba; `suggested` é o caminho proposto no diálogo.
    pub fn save_as(&mut self, ctx: &egui::Context, doc: PathBuf, suggested: PathBuf) {
        self.spawn(ctx, move || match save_dialog(&suggested) {
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
                .pick_files_or_e2e()
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
                .pick_file_or_e2e()
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
                .pick_file_or_e2e()
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
                .pick_folder_or_e2e()
                .map_or(Picked::Cancelled, Picked::Folder)
        });
    }

    /// Qualquer arquivo (Anexos → Adicionar arquivo).
    pub fn pick_any_file(&mut self, ctx: &egui::Context, title: &'static str) {
        self.spawn(ctx, move || {
            rfd::FileDialog::new()
                .set_title(title)
                .pick_file_or_e2e()
                .map_or(Picked::Cancelled, |p| Picked::Open(vec![p]))
        });
    }

    /// PDF ou imagem para um carimbo personalizado.
    pub fn pick_stamp_source(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, move || {
            let mut exts = vec!["pdf"];
            exts.extend_from_slice(IMAGE_EXTS);
            rfd::FileDialog::new()
                .set_title("Criar carimbo personalizado")
                .add_filter("PDF ou imagem", &exts)
                .pick_file_or_e2e()
                .map_or(Picked::Cancelled, |p| Picked::Open(vec![p]))
        });
    }

    /// Arquivo de dados de comentários (FDF, XFDF ou PDF) para importar.
    pub fn pick_comment_data(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, move || {
            rfd::FileDialog::new()
                .set_title("Importar comentários")
                .add_filter("Comentários (FDF, XFDF, PDF)", &["fdf", "xfdf", "pdf"])
                .pick_file_or_e2e()
                .map_or(Picked::Cancelled, |p| Picked::Open(vec![p]))
        });
    }

    /// Onde gravar os comentários exportados (FDF ou XFDF), sugerindo `suggested`.
    pub fn save_comment_data(&mut self, ctx: &egui::Context, suggested: PathBuf) {
        self.spawn(ctx, move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Exportar comentários")
                .add_filter("FDF", &["fdf"])
                .add_filter("XFDF", &["xfdf"]);
            suggest(dialog, &suggested)
                .save_file_or_e2e()
                .map_or(Picked::Cancelled, Picked::SaveTo)
        });
    }

    /// Onde gravar um arquivo de qualquer tipo (anexo), sugerindo `suggested`.
    pub fn save_any(&mut self, ctx: &egui::Context, suggested: PathBuf) {
        self.spawn(ctx, move || {
            let dialog = rfd::FileDialog::new().set_title("Salvar anexo");
            suggest(dialog, &suggested)
                .save_file_or_e2e()
                .map_or(Picked::Cancelled, Picked::SaveTo)
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

#[cfg(all(test, not(feature = "e2e")))]
mod hook_tests {
    #[test]
    fn the_pick_hook_is_off_in_normal_builds() {
        std::env::set_var("GERA_E2E_PICK", "/tmp/nao-deve-valer.pdf");
        assert_eq!(
            super::e2e_path(),
            None,
            "build normal ignora a variável dos testes"
        );
    }
}
