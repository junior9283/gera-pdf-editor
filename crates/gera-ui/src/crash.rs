//! Falhas na abertura e erros fatais. No Windows o app não tem console, então um erro (a placa de
//! vídeo que não inicia, um pânico) fazia a janela "abrir e fechar" sem nenhum aviso: agora vai
//! para um relatório (`erro.log`, na pasta de configurações) e, no Windows, para uma janela.
use std::path::Path;

/// O relatório fica com no máximo isto (o mais antigo sai).
const MAX_LOG: usize = 64 * 1024;

/// Uma entrada do relatório: versão, sistema e o motivo.
fn entry(kind: &str, detail: &str) -> String {
    let when = app_state::recents::now_secs();
    format!(
        "[{when}] {kind} — Gera PDF Editor {} ({} {})\n{detail}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Acrescenta `text` ao relatório `path`, mantendo só o final se passar de `MAX_LOG`.
fn append_log(path: &Path, text: &str) -> std::io::Result<()> {
    let mut log = std::fs::read_to_string(path).unwrap_or_default();
    log.push_str(text);
    // corta no começo, nunca no meio de um caractere
    let mut start = log.len().saturating_sub(MAX_LOG);
    while !log.is_char_boundary(start) {
        start += 1;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, &log[start..])
}

/// Grava o erro no relatório e o mostra ao usuário.
pub fn report(kind: &str, detail: &str, hint: &str) {
    let path = app_state::store::config_file("erro.log")
        .filter(|p| append_log(p, &entry(kind, detail)).is_ok());
    let mut text = format!("{kind}\n\n{detail}\n\n{hint}");
    if let Some(p) = path {
        text.push_str(&format!("\n\nRelatório: {}", p.display()));
    }
    show(&text);
}

/// Janela com o texto no Windows (sem console, ela é o único aviso).
#[cfg(windows)]
fn show(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    // SAFETY: cadeias terminadas em NUL, vivas durante a chamada; sem janela dona
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            crate::print_win::wide(text).as_ptr(),
            crate::print_win::wide("Gera PDF Editor").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Nos outros sistemas, o terminal.
#[cfg(not(windows))]
fn show(text: &str) {
    eprintln!("{text}");
}

/// Pânico (no pacote de release o programa aborta em seguida): vai para o relatório e para a
/// janela, em vez de o app fechar sem dizer nada.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map_or(String::new(), |l| format!(" ({}:{})", l.file(), l.line()));
        let message = info.payload_as_str().unwrap_or("erro interno");
        report(
            "O Gera PDF Editor encontrou um erro e precisa fechar.",
            &format!("{message}{place}"),
            "Se puder, envie o relatório em github.com/junior9283/gera-pdf-editor/issues.",
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_only_the_tail() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("config").join("erro.log");
        append_log(&log, "primeiro\n").unwrap();
        append_log(&log, "segundo\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "primeiro\nsegundo\n"
        );
        // passando do teto, sai o começo e nunca se corta um caractere ao meio
        let big = "ação ".repeat(MAX_LOG);
        append_log(&log, &big).unwrap();
        let kept = std::fs::read_to_string(&log).unwrap();
        assert!(
            kept.len() <= MAX_LOG && kept.ends_with("ação "),
            "{}",
            kept.len()
        );
        assert!(!kept.contains("primeiro"));
    }

    #[test]
    fn an_entry_names_the_version_and_the_system() {
        let e = entry("falha", "a placa de vídeo não iniciou");
        assert!(e.contains(env!("CARGO_PKG_VERSION")) && e.contains(std::env::consts::OS));
        assert!(e.contains("falha") && e.ends_with("a placa de vídeo não iniciou\n\n"));
    }
}
