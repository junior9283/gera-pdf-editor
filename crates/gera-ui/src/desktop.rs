//! Integração com o sistema quando o app roda como AppImage: o Wayland (e o menu de aplicativos)
//! acham o ícone pelo arquivo `.desktop` instalado com o mesmo id do app, e o AppImage sozinho não
//! instala nada. Como o AppImageLauncher, o app grava o próprio `.desktop` (apontando para o
//! AppImage) e os ícones na pasta do usuário, e só regrava quando algo muda.

use std::path::{Path, PathBuf};

/// Id do app: o nome do `.desktop` e do ícone, e o `app_id` da janela.
pub const APP_ID: &str = "gera-pdf-editor";

/// Caminho entre aspas para a linha `Exec` (especificação do Desktop Entry).
fn quoted(path: &Path) -> String {
    let mut out = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn desktop_entry(image: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Gera PDF Editor\n\
         Comment=Editor de PDF\nExec={} %f\nIcon={APP_ID}\nTerminal=false\n\
         Categories=Office;Viewer;\nMimeType=application/pdf;\nStartupWMClass={APP_ID}\n",
        quoted(image)
    )
}

/// Grava `bytes` em `path` só se o conteúdo for outro; devolve se gravou.
fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

/// Instala o `.desktop` (apontando para `image`) e os ícones de `appdir` em `data_home`; devolve
/// se algo mudou.
pub fn integrate(image: &Path, appdir: &Path, data_home: &Path) -> std::io::Result<bool> {
    let mut changed = write_if_changed(
        &data_home.join(format!("applications/{APP_ID}.desktop")),
        desktop_entry(image).as_bytes(),
    )?;
    let hicolor = appdir.join("usr/share/icons/hicolor");
    for entry in std::fs::read_dir(&hicolor)?.flatten() {
        let icon = entry.path().join(format!("apps/{APP_ID}.png"));
        let Ok(bytes) = std::fs::read(&icon) else {
            continue;
        };
        let dest = data_home
            .join("icons/hicolor")
            .join(entry.file_name())
            .join(format!("apps/{APP_ID}.png"));
        changed |= write_if_changed(&dest, &bytes)?;
    }
    Ok(changed)
}

/// Pasta de dados do usuário (`$XDG_DATA_HOME` ou `~/.local/share`).
fn data_home() -> Option<PathBuf> {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
}

/// Rodando como AppImage: integra numa thread (não atrasa a abertura). Fora do AppImage (pacote
/// ou `cargo run`) não faz nada.
pub fn integrate_appimage() {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let (Some(image), Some(appdir), Some(data)) = (env("APPIMAGE"), env("APPDIR"), data_home())
    else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("integracao".into())
        .spawn(move || {
            let _ = integrate(&image, &appdir, &data);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pasta montada do AppImage (só os ícones importam).
    fn appdir(root: &std::path::Path) -> std::path::PathBuf {
        let dir = root.join("appdir");
        for size in ["32x32", "256x256"] {
            let icons = dir.join(format!("usr/share/icons/hicolor/{size}/apps"));
            std::fs::create_dir_all(&icons).unwrap();
            std::fs::write(icons.join("gera-pdf-editor.png"), size).unwrap();
        }
        dir
    }

    #[test]
    fn installs_desktop_entry_and_icons_once() {
        let root = tempfile::tempdir().unwrap();
        let (app, data) = (appdir(root.path()), root.path().join("data"));
        let image = std::path::Path::new("/home/ana/Baixados/Gera PDF \"novo\".AppImage");
        assert!(
            integrate(image, &app, &data).unwrap(),
            "primeira vez: instala"
        );
        let desktop =
            std::fs::read_to_string(data.join("applications/gera-pdf-editor.desktop")).unwrap();
        assert!(
            desktop.contains(r#"Exec="/home/ana/Baixados/Gera PDF \"novo\".AppImage" %f"#),
            "{desktop}"
        );
        assert!(desktop.contains("Icon=gera-pdf-editor\n"));
        assert!(desktop.contains("StartupWMClass=gera-pdf-editor\n"));
        let icon = data.join("icons/hicolor/256x256/apps/gera-pdf-editor.png");
        assert_eq!(std::fs::read_to_string(icon).unwrap(), "256x256");
        assert!(
            !integrate(image, &app, &data).unwrap(),
            "nada mudou: não regrava"
        );
        // o AppImage mudou de lugar: o .desktop acompanha
        let moved = std::path::Path::new("/opt/gera.AppImage");
        assert!(integrate(moved, &app, &data).unwrap());
        let desktop =
            std::fs::read_to_string(data.join("applications/gera-pdf-editor.desktop")).unwrap();
        assert!(
            desktop.contains("Exec=\"/opt/gera.AppImage\" %f"),
            "{desktop}"
        );
    }
}
