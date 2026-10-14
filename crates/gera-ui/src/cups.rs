//! Impressão pelo CUPS (Linux): impressoras disponíveis e o `lp`.
use crate::print_job::PrintRequest;
use std::path::Path;
use std::process::Command;

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

/// O envio do pedido pelo `lp`, pronto para rodar numa thread (as opções já conferidas).
pub fn job(req: &PrintRequest) -> Result<impl FnOnce() -> Result<String, String> + Send, String> {
    let args = app_state::print::lp_args(
        &req.options,
        req.current,
        req.total,
        req.oversized,
        &req.file,
    )?;
    let lp = program("GERA_LP", "lp");
    let file = req.file.clone();
    Ok(move || run_lp(&lp, &args, &file))
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
