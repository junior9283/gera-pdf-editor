//! Falhas na abertura e erros fatais. No Windows o app não tem console, então um erro (a placa de
//! vídeo que não inicia, um pânico) fazia a janela "abrir e fechar" sem nenhum aviso: agora vai
//! para um relatório (`erro.log`, na pasta de configurações) e, no Windows, para uma janela.
use std::io::Write;
use std::path::Path;

/// O relatório fica com no máximo isto (o mais antigo sai).
const MAX_LOG: usize = 64 * 1024;

/// Uma entrada do relatório: versão, sistema e o motivo.
fn entry(kind: &str, detail: &str) -> String {
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "[{when}] {kind} — Gera PDF Editor {} ({} {})\n{detail}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Acrescenta `text` ao relatório `path`, mantendo só o final se passar de `MAX_LOG`.
fn append_log(path: &Path, text: &str) -> std::io::Result<()> {
    let mut old = std::fs::read(path).unwrap_or_default();
    old.extend_from_slice(text.as_bytes());
    let keep = old.len().saturating_sub(MAX_LOG);
    // não corta no meio de um caractere UTF-8
    let start = (keep..old.len())
        .find(|&i| std::str::from_utf8(&old[i..]).is_ok())
        .unwrap_or(old.len());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(path)?.write_all(&old[start..])
}

/// Grava o erro no relatório e o mostra ao usuário. Devolve onde ficou o relatório, se gravou.
pub fn report(kind: &str, detail: &str, hint: &str) -> Option<std::path::PathBuf> {
    let path = app_state::store::config_file("erro.log");
    let saved = path
        .as_ref()
        .filter(|p| append_log(p, &entry(kind, detail)).is_ok());
    let mut text = format!("{kind}\n\n{detail}\n\n{hint}");
    if let Some(p) = saved {
        text.push_str(&format!("\n\nRelatório: {}", p.display()));
    }
    eprintln!("{text}");
    show(&text);
    saved.cloned()
}

/// Janela com o texto (no Windows; nos outros sistemas o texto já foi para o terminal).
#[cfg(windows)]
fn show(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    // SAFETY: cadeias terminadas em NUL, vivas durante a chamada; sem janela dona
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Gera PDF Editor").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show(_text: &str) {}

/// Pânico (no pacote de release o programa aborta em seguida): vai para o relatório e para a
/// janela, em vez de o app fechar sem dizer nada.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map_or(String::new(), |l| format!(" ({}:{})", l.file(), l.line()));
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "erro interno".into());
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
