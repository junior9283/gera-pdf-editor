//! Arquivo ▸ Imprimir pela janela do sistema: a "Imprimir" do Windows ou a do portal no Linux
//! (KDE, GNOME), aberta numa thread. Sem portal, o app usa o próprio diálogo (CUPS).
use crate::print_job::PrintKind;
use std::sync::mpsc::{channel, Receiver};

/// A janela do app (HWND), dona da janela "Imprimir": fica desabilitada enquanto ela está aberta.
#[cfg(windows)]
static OWNER: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

/// Guarda a janela do app (chamado ao criar o app).
#[cfg(windows)]
pub fn set_owner(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = cc.window_handle() {
        if let RawWindowHandle::Win32(w) = handle.as_raw() {
            let _ = OWNER.set(w.hwnd.get());
        }
    }
}

/// Páginas já escolhidas (1-based), como nas miniaturas: [(1, 3), (7, 7)].
pub type Preset = Option<Vec<(u32, u32)>>;

/// O que a janela respondeu.
pub enum Outcome {
    Chosen(PrintKind),
    Cancelled,
    /// Não há janela do sistema (Linux sem portal): o app abre o próprio diálogo com as páginas
    /// já escolhidas.
    #[cfg(unix)]
    Unavailable(Preset),
    #[cfg(windows)]
    Failed(String),
}

/// Abre a janela do sistema para imprimir `title` (`total` páginas; `landscape`: a primeira é
/// deitada).
pub fn ask(
    ctx: &egui::Context,
    title: String,
    total: usize,
    landscape: bool,
    preset: Preset,
) -> Receiver<Outcome> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    #[cfg(windows)]
    let show = {
        // a janela do app; sem ela (não deveria acontecer), a janela ativa desta thread
        let owner = OWNER.get().copied().unwrap_or_else(|| {
            // SAFETY: só lê a janela ativa desta thread (a do app)
            unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() as isize }
        });
        let _ = title;
        move || windows(owner, total, landscape, preset)
    };
    #[cfg(unix)]
    let show = {
        let _ = (total, landscape);
        move || portal(&title, preset)
    };
    let _ = std::thread::Builder::new()
        .name("janela-imprimir".into())
        .spawn(move || {
            let _ = tx.send(show());
            ctx.request_repaint();
        });
    rx
}

#[cfg(windows)]
fn windows(owner: isize, total: usize, landscape: bool, preset: Preset) -> Outcome {
    match crate::print_win::ask(owner, total, landscape, preset) {
        Ok(Some(choice)) => Outcome::Chosen(PrintKind::Windows(choice)),
        Ok(None) => Outcome::Cancelled,
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(target_os = "linux")]
fn portal(title: &str, preset: Preset) -> Outcome {
    use crate::portal_print::{prepare, session, Prepared};
    match session().and_then(|conn| prepare(&conn, title, preset.as_deref())) {
        Ok(Prepared::Token(token)) => Outcome::Chosen(PrintKind::Portal {
            token,
            title: title.to_string(),
        }),
        Ok(Prepared::Cancelled) => Outcome::Cancelled,
        // sem sessão D-Bus ou sem portal de impressão
        Err(_) => Outcome::Unavailable(preset),
    }
}

/// Outros Unix: sem portal, o diálogo do app.
#[cfg(all(unix, not(target_os = "linux")))]
fn portal(_title: &str, preset: Preset) -> Outcome {
    Outcome::Unavailable(preset)
}
