//! Impressão pelo CUPS: impressoras disponíveis, pasta temporária privada e o `lp` numa thread.
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
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

/// Arquivo temporário novo para imprimir, numa pasta só do usuário (a cópia sai sem senha).
pub fn temp_file() -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("gera-pdf-editor");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(dir.join(format!("imprimir-{}-{n}.pdf", std::process::id())))
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

/// Envios ao CUPS em andamento (cada um numa thread, para não travar a janela).
#[derive(Default)]
pub struct PrintJobs {
    running: Vec<(String, Receiver<Result<String, String>>)>,
}

impl PrintJobs {
    pub fn start(
        &mut self,
        ctx: &egui::Context,
        printer: String,
        args: Vec<String>,
        file: PathBuf,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let lp = program("GERA_LP", "lp");
        let spawned = std::thread::Builder::new()
            .name("impressao".into())
            .spawn(move || {
                let _ = tx.send(run_lp(&lp, &args, &file));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.running.push((printer, rx));
        }
    }

    /// Mensagens dos envios que terminaram ("Enviado para X." ou o erro).
    pub fn poll(&mut self) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
