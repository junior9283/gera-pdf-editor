//! Impressão: as impressoras do sistema (CUPS no Linux, `cups.rs`; GDI no Windows,
//! `print_win.rs`), a pasta temporária privada e os envios numa thread.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Impressoras do sistema e a padrão (lista vazia se não houver como saber).
pub fn printers() -> (Vec<String>, Option<String>) {
    #[cfg(unix)]
    return crate::cups::printers();
    #[cfg(windows)]
    return crate::print_win::printers();
}

/// Lê as impressoras numa thread (o CUPS ou o spooler podem demorar).
pub fn printers_in_background(ctx: &egui::Context) -> Receiver<(Vec<String>, Option<String>)> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("impressoras".into())
        .spawn(move || {
            let _ = tx.send(printers());
            ctx.request_repaint();
        });
    rx
}

/// Pasta `base/name` só do usuário (0700), recusando link simbólico ou pasta de outro dono.
#[cfg(unix)]
fn private_dir(base: &Path, name: &str) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let dir = base.join(name);
    let me = std::fs::metadata("/proc/self")?.uid();
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() || m.uid() != me => {
            return Err(std::io::Error::other(format!(
                "{} não é uma pasta segura para a impressão",
                dir.display()
            )));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        }
        Err(e) => return Err(e),
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// Pasta `base/name` do usuário (a pasta temporária do Windows já é só dele), recusando link
/// simbólico ou junção.
#[cfg(windows)]
fn private_dir(base: &Path, name: &str) -> std::io::Result<PathBuf> {
    let dir = base.join(name);
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => Err(std::io::Error::other(format!(
            "{} não é uma pasta segura para a impressão",
            dir.display()
        ))),
        Ok(_) => Ok(dir),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&dir)?;
            Ok(dir)
        }
        Err(e) => Err(e),
    }
}

/// Pasta das cópias de impressão (saem sem senha): `$XDG_RUNTIME_DIR/gera-pdf-editor`, ou uma
/// pasta com o uid no nome dentro do temporário do sistema, que é compartilhado entre usuários.
#[cfg(unix)]
pub(crate) fn spool_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        Some(runtime) => private_dir(Path::new(&runtime), "gera-pdf-editor"),
        None => {
            let uid = std::fs::metadata("/proc/self")?.uid();
            private_dir(&std::env::temp_dir(), &format!("gera-pdf-editor-{uid}"))
        }
    }
}

/// Pasta das cópias de impressão no Windows: dentro do temporário do usuário (`%TEMP%`).
#[cfg(windows)]
pub(crate) fn spool_dir() -> std::io::Result<PathBuf> {
    private_dir(&std::env::temp_dir(), "gera-pdf-editor")
}

/// Prefixos das cópias na pasta privada: impressão, imagem colada, anexo aberto e páginas copiadas. Todas levam o
/// pid logo depois do prefixo, e a varredura apaga as de execuções que já terminaram.
pub(crate) const PRINT_PREFIX: &str = "imprimir-";
pub(crate) const PASTE_PREFIX: &str = "gera-colar-";
pub(crate) const ATTACHMENT_PREFIX: &str = "gera-anexo-";
/// Páginas a caminho de outra aba (Copiar para, arrastar até a aba).
pub(crate) const COPY_PREFIX: &str = "gera-copia-";
const SPOOL_PREFIXES: [&str; 4] = [PRINT_PREFIX, PASTE_PREFIX, ATTACHMENT_PREFIX, COPY_PREFIX];

/// Nome único de uma cópia na pasta privada: "<prefixo><pid>-<n><fim>".
pub(crate) fn spool_file_name(prefix: &str, end: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{}-{n}{end}", std::process::id())
}

/// Arquivo temporário novo para imprimir.
pub fn temp_file() -> std::io::Result<PathBuf> {
    Ok(spool_dir()?.join(spool_file_name(PRINT_PREFIX, ".pdf")))
}

/// Apaga cópias de impressão deixadas por execuções que já terminaram (app fechado no meio).
fn sweep_stale(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| SPOOL_PREFIXES.iter().find_map(|p| n.strip_prefix(p)))
            .and_then(|n| n.split('-').next())
            .and_then(|p| p.parse::<u32>().ok());
        if pid.is_some_and(|pid| !alive(pid)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// O processo `pid` ainda está rodando?
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: o handle aberto é fechado; o código de saída vai para uma variável local
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

/// Ao abrir o app: limpa sobras de impressões de execuções anteriores.
pub fn sweep() {
    if let Ok(dir) = spool_dir() {
        sweep_stale(&dir);
    }
}

/// Um pedido de impressão: as opções do diálogo e o PDF temporário já exportado.
pub struct PrintRequest {
    pub options: app_state::print::PrintOptions,
    /// Página atual (1-based) e total de páginas.
    pub current: usize,
    pub total: usize,
    /// Alguma página é maior que o papel ("Reduzir páginas grandes"; no Windows, a montagem das
    /// folhas compara cada página com o papel).
    #[cfg_attr(windows, allow(dead_code))]
    pub oversized: bool,
    pub file: PathBuf,
}

/// Envios em andamento (cada um numa thread, para não travar a janela). Ao fechar o app, espera
/// os envios terminarem: a impressão não se perde e o temporário é apagado.
#[derive(Default)]
pub struct PrintJobs {
    running: Vec<(String, Receiver<Result<String, String>>)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl PrintJobs {
    /// Envia o pedido à impressora numa thread; opções inválidas voltam como erro na hora (e o
    /// temporário é apagado).
    pub fn start(&mut self, ctx: &egui::Context, req: PrintRequest) -> Result<(), String> {
        let ctx = ctx.clone();
        let printer = req.options.printer.clone();
        #[cfg(unix)]
        let job = crate::cups::job(&req).inspect_err(|_| {
            let _ = std::fs::remove_file(&req.file);
        })?;
        #[cfg(windows)]
        let job = move || crate::print_win::print(&req);
        self.spawn(printer, job, move || ctx.request_repaint());
        Ok(())
    }

    fn spawn(
        &mut self,
        printer: String,
        job: impl FnOnce() -> Result<String, String> + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("impressao".into())
            .spawn(move || {
                let _ = tx.send(job());
                wake();
            });
        if let Ok(handle) = spawned {
            self.running.push((printer, rx));
            self.threads.push(handle);
        }
    }

    /// Mensagens dos envios que terminaram ("Enviado para X." ou o erro).
    pub fn poll(&mut self) -> Vec<String> {
        self.threads.retain(|t| !t.is_finished());
        let mut done = Vec::new();
        self.running.retain(|(printer, rx)| match rx.try_recv() {
            Ok(Ok(_)) => {
                done.push(format!("Enviado para {printer}."));
                false
            }
            Ok(Err(e)) => {
                done.push(format!("Não foi possível imprimir: {e}"));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                done.push("Não foi possível imprimir: o envio foi interrompido.".into());
                false
            }
        });
        done
    }
}

impl Drop for PrintJobs {
    fn drop(&mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_spool_folder_is_private_and_refuses_symlinks() {
        let base = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        let dir = private_dir(base.path(), "gera-teste").unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let elsewhere = tempfile::tempdir().unwrap();
        let link = base.path().join("armadilha");
        std::os::unix::fs::symlink(elsewhere.path(), &link).unwrap();
        assert!(
            private_dir(base.path(), "armadilha").is_err(),
            "link simbólico é recusado"
        );
        let mode = std::fs::metadata(elsewhere.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_ne!(mode, 0o700, "a pasta apontada pelo link não é tocada");
    }

    #[test]
    fn stale_copies_from_finished_runs_are_swept() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir
            .path()
            .join(format!("imprimir-{}-0.pdf", std::process::id()));
        let dead = dir.path().join("imprimir-999999999-3.pdf");
        let other = dir.path().join("anotacoes.txt");
        for f in [&mine, &dead, &other] {
            std::fs::write(f, b"x").unwrap();
        }
        sweep_stale(dir.path());
        assert!(mine.exists(), "este processo ainda está vivo");
        assert!(!dead.exists(), "sobra de execução que já terminou");
        assert!(other.exists(), "só mexe nas cópias de impressão");
    }

    #[test]
    fn dropping_the_jobs_waits_for_lp_to_finish() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        let mut jobs = PrintJobs::default();
        let f = file.clone();
        jobs.spawn(
            "HP".into(),
            move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = std::fs::remove_file(&f);
                Ok(String::new())
            },
            || {},
        );
        drop(jobs); // o app fechando logo depois de clicar Imprimir
        assert!(
            !file.exists(),
            "o envio terminou e apagou o temporário antes de sair"
        );
    }
}
