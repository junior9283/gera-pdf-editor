//! Impressão pelo CUPS: impressoras disponíveis, pasta temporária privada e o `lp` numa thread.
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Programa do CUPS, trocável por variável de ambiente (os testes E2E usam um `lp` falso).
fn program(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.into())
}

fn parse_default(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.strip_prefix("system default destination:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_list(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Impressoras do CUPS e a padrão (lista vazia se o CUPS não estiver disponível).
pub fn printers() -> (Vec<String>, Option<String>) {
    let lpstat = program("GERA_LPSTAT", "lpstat");
    let run = |arg: &str| {
        Command::new(&lpstat)
            .arg(arg)
            .env("LC_ALL", "C") // a saída é lida em inglês
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    (parse_list(&run("-e")), parse_default(&run("-d")))
}

/// Lê as impressoras numa thread (o `lpstat` pode demorar com o CUPS lento ou parado).
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
fn private_dir(base: &Path, name: &str) -> std::io::Result<PathBuf> {
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

/// Pasta das cópias de impressão (saem sem senha): `$XDG_RUNTIME_DIR/gera-pdf-editor`, ou uma
/// pasta com o uid no nome dentro do temporário do sistema, que é compartilhado entre usuários.
pub(crate) fn spool_dir() -> std::io::Result<PathBuf> {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        Some(runtime) => private_dir(Path::new(&runtime), "gera-pdf-editor"),
        None => {
            let uid = std::fs::metadata("/proc/self")?.uid();
            private_dir(&std::env::temp_dir(), &format!("gera-pdf-editor-{uid}"))
        }
    }
}

/// Arquivo temporário novo para imprimir.
pub fn temp_file() -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(spool_dir()?.join(format!("imprimir-{}-{n}.pdf", std::process::id())))
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
            // cópias de impressão e imagens coladas (clipboard.rs)
            .and_then(|n| {
                n.strip_prefix("imprimir-")
                    .or_else(|| n.strip_prefix("gera-colar-"))
            })
            .and_then(|n| n.split('-').next())
            .and_then(|p| p.parse::<u32>().ok());
        if let Some(pid) = pid {
            if !Path::new("/proc").join(pid.to_string()).exists() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Ao abrir o app: limpa sobras de impressões de execuções anteriores.
pub fn sweep() {
    if let Ok(dir) = spool_dir() {
        sweep_stale(&dir);
    }
}

/// Roda o `lp` e apaga o temporário em qualquer caso. Devolve a saída ou a mensagem de erro.
fn run_lp(program: &str, args: &[String], file: &Path) -> Result<String, String> {
    let out = Command::new(program).args(args).env("LC_ALL", "C").output();
    let _ = std::fs::remove_file(file);
    let out = out.map_err(|e| format!("não foi possível executar {program}: {e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() {
        Ok(text(&out.stdout))
    } else {
        let err = text(&out.stderr);
        Err(if err.is_empty() {
            format!("{program} terminou com erro")
        } else {
            err
        })
    }
}

/// Envios ao CUPS em andamento (cada um numa thread, para não travar a janela). Ao fechar o
/// app, espera os envios terminarem: a impressão não se perde e o temporário é apagado.
#[derive(Default)]
pub struct PrintJobs {
    running: Vec<(String, Receiver<Result<String, String>>)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl PrintJobs {
    pub fn start(
        &mut self,
        ctx: &egui::Context,
        printer: String,
        args: Vec<String>,
        file: PathBuf,
    ) {
        let ctx = ctx.clone();
        let lp = program("GERA_LP", "lp");
        self.spawn(lp, printer, args, file, move || ctx.request_repaint());
    }

    fn spawn(
        &mut self,
        lp: String,
        printer: String,
        args: Vec<String>,
        file: PathBuf,
        wake: impl Fn() + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("impressao".into())
            .spawn(move || {
                let _ = tx.send(run_lp(&lp, &args, &file));
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
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn default_printer_is_read_from_lpstat_output() {
        assert_eq!(
            parse_default("system default destination: HP_LaserJet\n"),
            Some("HP_LaserJet".into())
        );
        assert_eq!(parse_default("no system default destination\n"), None);
    }

    #[test]
    fn printer_list_skips_blank_lines() {
        assert_eq!(
            parse_list("HP\n\nPDF\n"),
            vec!["HP".to_string(), "PDF".to_string()]
        );
    }

    #[test]
    fn a_print_job_runs_lp_and_removes_the_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        let args = vec!["-d".to_string(), "HP".into(), file.display().to_string()];
        let out = run_lp("echo", &args, &file).unwrap();
        assert!(out.contains("-d HP"), "{out}");
        assert!(!file.exists(), "o temporário sempre some");
        std::fs::write(&file, b"%PDF").unwrap();
        assert!(run_lp("false", &[], &file).is_err());
        assert!(!file.exists(), "some também quando o lp falha");
        assert!(run_lp("/nao/existe/lp", &[], &file).is_err());
    }

    #[test]
    fn the_spool_folder_is_private_and_refuses_symlinks() {
        let base = tempfile::tempdir().unwrap();
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
        jobs.spawn(
            "sleep".into(),
            "HP".into(),
            vec!["0.3".into()],
            file.clone(),
            || {},
        );
        drop(jobs); // o app fechando logo depois de clicar Imprimir
        assert!(
            !file.exists(),
            "o envio terminou e apagou o temporário antes de sair"
        );
    }
}
