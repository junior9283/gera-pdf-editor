//! Arquivo ▸ Imprimir pela janela do sistema: a "Imprimir" do Windows ou a do portal no Linux
//! (KDE, GNOME), aberta numa thread. Sem portal, o app usa o próprio diálogo (CUPS).
use crate::print_job::PrintKind;
use std::sync::mpsc::{channel, Receiver};

/// O que a janela respondeu.
pub enum Outcome {
    Chosen(PrintKind),
    Cancelled,
    /// Não há janela do sistema (Linux sem portal): o app abre o próprio diálogo.
    #[cfg_attr(windows, allow(dead_code))]
    Unavailable,
    #[cfg_attr(not(windows), allow(dead_code))]
    Failed(String),
}

/// Abre a janela do sistema para imprimir `title` (`total` páginas; `landscape`: a primeira é
/// deitada; `preset`: páginas já escolhidas, 1-based).
pub fn ask(
    ctx: &egui::Context,
    title: String,
    total: usize,
    landscape: bool,
    preset: Option<Vec<(u32, u32)>>,
) -> Receiver<Outcome> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    // a janela do app é a dona da janela de impressão (fica desabilitada enquanto ela está aberta)
    #[cfg(windows)]
    // SAFETY: só lê a janela ativa desta thread (a do app)
    let owner =
        unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() } as isize;
    let _ = std::thread::Builder::new()
        .name("janela-imprimir".into())
        .spawn(move || {
            #[cfg(windows)]
            let outcome = match crate::print_win::ask(owner, total, landscape, preset) {
                Ok(Some(choice)) => Outcome::Chosen(PrintKind::Windows(choice)),
                Ok(None) => Outcome::Cancelled,
                Err(e) => Outcome::Failed(e),
            };
            #[cfg(target_os = "linux")]
            let outcome = portal(&title, preset);
            #[cfg(not(any(windows, target_os = "linux")))]
            let outcome = Outcome::Unavailable;
            let _ = (&title, total, landscape);
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
    rx
}

#[cfg(target_os = "linux")]
fn portal(title: &str, preset: Option<Vec<(u32, u32)>>) -> Outcome {
    use crate::portal_print::{prepare, session, Prepared};
    let ranges = preset.map(|r| {
        r.iter()
            .map(|(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    });
    match session().and_then(|conn| prepare(&conn, title, ranges.as_deref())) {
        Ok(Prepared::Token(token)) => Outcome::Chosen(PrintKind::Portal {
            token,
            title: title.to_string(),
        }),
        Ok(Prepared::Cancelled) => Outcome::Cancelled,
        // sem sessão D-Bus ou sem portal de impressão
        Err(_) => Outcome::Unavailable,
    }
}
