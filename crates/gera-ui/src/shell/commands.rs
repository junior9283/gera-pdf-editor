//! Comandos da casca e a tabela única de atalhos (teclado, menus e janela de ajuda).
use crate::doc_tab::DocCommand;
use app_state::zoom::Fit;
use egui::{Key, KeyboardShortcut, Modifiers};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open,
    OpenRecent(PathBuf),
    SaveAs,
    CloseTab,
    CloseAll,
    Quit,
    Doc(DocCommand),
    ToggleDark,
    TogglePanel(app_state::prefs::LeftPanel),
    ToggleTools,
    FullScreen,
    /// Modo leitura: esconde barras e painéis (Ctrl+H).
    ReadMode,
    NextTab,
    PrevTab,
    ActivateTab(usize),
    Shortcuts,
    About,
    Properties,
    Print,
    /// Ferramentas → Comparar arquivos.
    Compare,
    /// Editar → Preferências… (Ctrl+K).
    Preferences,
    /// Ajuda → Ajuda do Gera PDF Editor (F1).
    Help,
    /// Colar (Ctrl+V): o texto da área de transferência que veio no evento, ou `None` (menu),
    /// e então a aba lê a área do sistema (também imagens).
    Paste(Option<String>),
    /// Abre uma ferramenta do catálogo pelo menu (o mesmo que clicar nela em Ferramentas).
    Tool(&'static str),
}

impl Command {
    pub fn label(&self) -> String {
        let s = match self {
            Command::Open => "Abrir…",
            Command::OpenRecent(p) => return p.display().to_string(),
            Command::SaveAs => "Salvar como…",
            Command::CloseTab => "Fechar aba",
            Command::CloseAll => "Fechar todas",
            Command::Quit => "Sair",
            Command::ToggleDark => "Modo escuro",
            Command::TogglePanel(p) => return p.label().into(),
            Command::ToggleTools => "Ferramentas",
            Command::FullScreen => "Tela cheia",
            Command::ReadMode => "Modo leitura",
            Command::NextTab => "Próxima aba",
            Command::PrevTab => "Aba anterior",
            Command::ActivateTab(_) => "Ir para a aba",
            Command::Shortcuts => "Atalhos de teclado",
            Command::About => "Sobre o Gera PDF Editor",
            Command::Properties => "Propriedades…",
            Command::Print => "Imprimir…",
            Command::Compare => "Comparar arquivos…",
            Command::Preferences => "Preferências…",
            Command::Help => "Ajuda do Gera PDF Editor",
            Command::Paste(_) => "Colar",
            Command::Tool(id) => {
                return app_state::tools_catalog::find(id)
                    .map_or(*id, |t| t.name)
                    .into()
            }
            Command::Doc(d) => match d {
                DocCommand::Save => "Salvar",
                DocCommand::Undo => "Desfazer",
                DocCommand::Redo => "Refazer",
                DocCommand::Find => "Localizar",
                DocCommand::ZoomIn => "Ampliar",
                DocCommand::ZoomOut => "Reduzir",
                DocCommand::ZoomReset => "Tamanho real",
                DocCommand::Fit(Fit::Width) => "Ajustar à largura",
                DocCommand::Fit(Fit::Page) => "Ajustar à página",
                DocCommand::Fit(Fit::None) => "Zoom livre",
                DocCommand::Fit(Fit::Height) => "Ajustar à altura",
                DocCommand::Fit(Fit::Visible) => "Ajustar ao conteúdo visível",
                DocCommand::Mode(m) => return m.label().into(),
                DocCommand::ToggleCover => "Mostrar capa sozinha",
                DocCommand::Copy => "Copiar",
                DocCommand::SelectAll => "Selecionar tudo",
                DocCommand::Delete => "Excluir",
                DocCommand::Rotate { clockwise: true } => "Girar no sentido horário",
                DocCommand::Rotate { clockwise: false } => "Girar no sentido anti-horário",
                DocCommand::NextChange => "Próxima alteração",
                DocCommand::PrevChange => "Alteração anterior",
                DocCommand::FirstPage => "Primeira página",
                DocCommand::PrevPage => "Página anterior",
                DocCommand::NextPage => "Próxima página",
                DocCommand::LastPage => "Última página",
                DocCommand::GoToPage => "Ir para página…",
                DocCommand::ZoomTo => "Zoom para…",
                DocCommand::Tool(t) => return t.label().into(),
                DocCommand::CycleMarkup => "Alternar destacar e sublinhar",
                DocCommand::Cut => "Recortar",
                DocCommand::FindNext => "Próximo resultado",
                DocCommand::FindPrev => "Resultado anterior",
                DocCommand::Revert => "Reverter",
                DocCommand::DeselectAll => "Desmarcar tudo",
                DocCommand::ViewBack => "Vista anterior",
                DocCommand::ViewForward => "Vista seguinte",
            },
        };
        s.into()
    }
}

/// Ajuda do app: o README do projeto (endereço fixo, não vem de PDF).
pub const HELP_URL: &str = "https://github.com/junior9283/gera-pdf-editor#readme";

/// Atalhos de teclado. Os com Shift vêm antes dos sem: o egui aceita Shift a mais num atalho
/// que não o pede, e testar `Ctrl+Z` antes consumiria `Ctrl+Shift+Z`.
pub fn shortcuts() -> Vec<(KeyboardShortcut, Command)> {
    let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
    let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
    let alt = |k| KeyboardShortcut::new(Modifiers::ALT, k);
    let doc = Command::Doc;
    vec![
        (cmd_shift(Key::S), Command::SaveAs),
        (cmd_shift(Key::A), doc(DocCommand::DeselectAll)),
        (
            KeyboardShortcut::new(Modifiers::SHIFT, Key::F3),
            doc(DocCommand::FindPrev),
        ),
        (cmd_shift(Key::G), doc(DocCommand::FindPrev)),
        (
            KeyboardShortcut::new(Modifiers::NONE, Key::F3),
            doc(DocCommand::FindNext),
        ),
        (cmd(Key::G), doc(DocCommand::FindNext)),
        (
            KeyboardShortcut::new(Modifiers::NONE, Key::F1),
            Command::Help,
        ),
        // navegação de páginas: os do Acrobat
        (cmd_shift(Key::N), doc(DocCommand::GoToPage)),
        (cmd_shift(Key::PageUp), doc(DocCommand::FirstPage)),
        (cmd_shift(Key::PageDown), doc(DocCommand::LastPage)),
        (cmd_shift(Key::ArrowUp), doc(DocCommand::FirstPage)),
        (cmd_shift(Key::ArrowDown), doc(DocCommand::LastPage)),
        (cmd(Key::PageUp), doc(DocCommand::PrevPage)),
        (cmd(Key::PageDown), doc(DocCommand::NextPage)),
        (alt(Key::ArrowLeft), doc(DocCommand::ViewBack)),
        (alt(Key::ArrowRight), doc(DocCommand::ViewForward)),
        (cmd_shift(Key::Z), doc(DocCommand::Redo)),
        // girar vista: os mesmos do Acrobat; antes do zoom, que usa as mesmas teclas sem Shift
        (
            cmd_shift(Key::Plus),
            doc(DocCommand::Rotate { clockwise: true }),
        ),
        (
            cmd_shift(Key::Minus),
            doc(DocCommand::Rotate { clockwise: false }),
        ),
        (cmd_shift(Key::Tab), Command::PrevTab),
        (cmd(Key::O), Command::Open),
        (cmd(Key::S), doc(DocCommand::Save)),
        (cmd(Key::W), Command::CloseTab),
        (cmd(Key::Q), Command::Quit),
        (cmd(Key::Z), doc(DocCommand::Undo)),
        (cmd(Key::F), doc(DocCommand::Find)),
        (cmd(Key::C), doc(DocCommand::Copy)),
        (cmd(Key::A), doc(DocCommand::SelectAll)),
        // mostrados nos menus; o egui entrega Ctrl+X e Ctrl+V como eventos (ver `from_keyboard`)
        (cmd(Key::X), doc(DocCommand::Cut)),
        (cmd(Key::V), Command::Paste(None)),
        (cmd(Key::Tab), Command::NextTab),
        // resultado da comparação: como no Acrobat
        (cmd(Key::Period), doc(DocCommand::NextChange)),
        (cmd(Key::Comma), doc(DocCommand::PrevChange)),
        (cmd(Key::D), Command::Properties),
        (cmd(Key::P), Command::Print),
        (
            KeyboardShortcut::new(Modifiers::NONE, Key::Delete),
            doc(DocCommand::Delete),
        ),
        (cmd(Key::Equals), doc(DocCommand::ZoomIn)),
        (cmd(Key::Plus), doc(DocCommand::ZoomIn)),
        (cmd(Key::Minus), doc(DocCommand::ZoomOut)),
        (cmd(Key::Num0), doc(DocCommand::Fit(Fit::Page))),
        (cmd(Key::Num1), doc(DocCommand::ZoomReset)),
        (cmd(Key::Num2), doc(DocCommand::Fit(Fit::Width))),
        (cmd(Key::Num3), doc(DocCommand::Fit(Fit::Visible))),
        (cmd(Key::Y), doc(DocCommand::ZoomTo)),
        (cmd(Key::L), Command::FullScreen),
        (cmd(Key::K), Command::Preferences),
        (cmd(Key::H), Command::ReadMode),
        (
            KeyboardShortcut::new(Modifiers::NONE, Key::F11),
            Command::FullScreen,
        ),
    ]
}

/// Aceleradores de tecla única do Acrobat (Preferências → Geral; desligados de fábrica).
fn single_keys() -> Vec<(KeyboardShortcut, Command)> {
    use app_state::tools::Tool;
    let key = |k| KeyboardShortcut::new(Modifiers::NONE, k);
    let tool = |t| Command::Doc(DocCommand::Tool(t));
    vec![
        // Shift+U antes do U (o egui aceita Shift a mais num atalho sem Shift)
        (
            KeyboardShortcut::new(Modifiers::SHIFT, Key::U),
            Command::Doc(DocCommand::CycleMarkup),
        ),
        (key(Key::V), tool(Tool::Select)),
        (key(Key::H), tool(Tool::Hand)),
        (key(Key::Z), tool(Tool::Zoom)),
        (key(Key::S), tool(Tool::Note)),
        (key(Key::U), tool(Tool::Highlight)),
        (key(Key::D), tool(Tool::Ink)),
        (key(Key::X), tool(Tool::TextBox)),
        (key(Key::K), tool(Tool::Stamp)),
    ]
}

/// A tabela de atalhos, com as teclas únicas quando a preferência está ligada.
/// Montada uma vez (o teclado a percorre a cada quadro, e cada item de menu busca nela).
pub fn shortcuts_for(single_key: bool) -> &'static [(KeyboardShortcut, Command)] {
    static PLAIN: std::sync::LazyLock<Vec<(KeyboardShortcut, Command)>> =
        std::sync::LazyLock::new(shortcuts);
    static WITH_KEYS: std::sync::LazyLock<Vec<(KeyboardShortcut, Command)>> =
        std::sync::LazyLock::new(|| {
            let mut all = shortcuts();
            all.extend(single_keys());
            all
        });
    if single_key {
        &WITH_KEYS
    } else {
        &PLAIN
    }
}

/// Primeiro atalho do comando (para mostrar no menu).
pub fn shortcut_of(cmd: &Command) -> Option<KeyboardShortcut> {
    shortcuts_for(false)
        .iter()
        .find(|(_, c)| c == cmd)
        .map(|(s, _)| *s)
}

/// Atalho escrito como no Acrobat, com símbolos ("Ctrl+Shift++", "Ctrl+="), não nomes de tecla.
pub fn shortcut_text(sc: &KeyboardShortcut) -> String {
    let m = sc.modifiers;
    let mut text = String::new();
    for (on, name) in [
        (m.command || m.ctrl, "Ctrl+"),
        (m.shift, "Shift+"),
        (m.alt, "Alt+"),
    ] {
        if on {
            text.push_str(name);
        }
    }
    text.push_str(match sc.logical_key {
        Key::Minus => "-", // o egui usa o sinal tipográfico "−"
        key => key.symbol_or_name(),
    });
    text
}

/// Dica de botão: rótulo do comando e, se houver, o atalho ("Salvar (Ctrl+S)").
pub fn hover(cmd: &Command) -> String {
    match shortcut_of(cmd) {
        Some(sc) => format!("{} ({})", cmd.label(), shortcut_text(&sc)),
        None => cmd.label(),
    }
}

/// Enquanto o usuário digita, estes atalhos são do campo de texto.
fn belongs_to_text_fields(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Doc(
            DocCommand::Undo
                | DocCommand::Redo
                | DocCommand::ZoomIn
                | DocCommand::ZoomOut
                | DocCommand::Copy
                | DocCommand::SelectAll
                | DocCommand::Delete
                // Ctrl+Y também é Refazer nos campos de texto
                | DocCommand::ZoomTo
        )
    )
}

/// Comandos pedidos pelo teclado neste quadro (as teclas são consumidas).
pub fn from_keyboard(ctx: &egui::Context, single_key: bool) -> Vec<Command> {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|i| {
        let mut out: Vec<Command> = shortcuts_for(single_key)
            .iter()
            // digitando, as teclas sem Ctrl/Alt (letras das teclas únicas) são do campo
            .filter(|(s, c)| {
                let plain = !s.modifiers.command && !s.modifiers.alt && !s.modifiers.ctrl;
                !(typing && (belongs_to_text_fields(c) || plain))
            })
            .filter(|(s, _)| i.consume_shortcut(s))
            .map(|(_, c)| c.clone())
            .collect();
        // o egui entrega Ctrl+C, Ctrl+X e Ctrl+V como eventos, não como teclas; num campo de
        // texto são dele
        if !typing {
            for e in &i.events {
                match e {
                    egui::Event::Copy => out.push(Command::Doc(DocCommand::Copy)),
                    egui::Event::Cut => out.push(Command::Doc(DocCommand::Cut)),
                    egui::Event::Paste(t) => out.push(Command::Paste(Some(t.clone()))),
                    _ => {}
                }
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shortcut_is_listed_twice() {
        let all = shortcuts();
        for (n, (a, _)) in all.iter().enumerate() {
            assert!(all[n + 1..].iter().all(|(b, _)| b != a), "{a:?} repetido");
        }
    }

    #[test]
    fn shift_variants_come_before_their_plain_twins() {
        // o egui aceita Shift a mais num atalho sem Shift: o com Shift precisa ser testado antes
        let all = shortcuts();
        for (i, (plain, _)) in all
            .iter()
            .enumerate()
            .filter(|(_, (s, _))| !s.modifiers.shift)
        {
            let shifted = all
                .iter()
                .position(|(s, _)| s.logical_key == plain.logical_key && s.modifiers.shift);
            if let Some(j) = shifted {
                assert!(j < i, "{plain:?} vem antes da versão com Shift");
            }
        }
    }

    #[test]
    fn rotation_uses_the_acrobat_shortcuts() {
        let rotate_cw = shortcut_of(&Command::Doc(DocCommand::Rotate { clockwise: true })).unwrap();
        let rotate_ccw =
            shortcut_of(&Command::Doc(DocCommand::Rotate { clockwise: false })).unwrap();
        let ctrl_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        assert_eq!(
            (rotate_cw.modifiers, rotate_cw.logical_key),
            (ctrl_shift, Key::Plus)
        );
        assert_eq!(
            (rotate_ccw.modifiers, rotate_ccw.logical_key),
            (ctrl_shift, Key::Minus)
        );
    }

    #[test]
    fn main_commands_have_shortcuts_and_labels() {
        for cmd in [
            Command::Open,
            Command::SaveAs,
            Command::Doc(DocCommand::Save),
            Command::Doc(DocCommand::Undo),
            Command::Doc(DocCommand::Rotate { clockwise: true }),
            Command::FullScreen,
        ] {
            assert!(shortcut_of(&cmd).is_some(), "{cmd:?}");
            assert!(!cmd.label().is_empty());
        }
    }

    #[test]
    fn navigation_shortcuts_match_acrobat() {
        let text = |c| shortcut_text(&shortcut_of(&Command::Doc(c)).unwrap());
        assert_eq!(text(DocCommand::GoToPage), "Ctrl+Shift+N");
        assert_eq!(text(DocCommand::ViewBack), "Alt+⏴");
        assert_eq!(text(DocCommand::ViewForward), "Alt+⏵");
        assert_eq!(text(DocCommand::PrevPage), "Ctrl+PageUp");
        assert_eq!(text(DocCommand::NextPage), "Ctrl+PageDown");
        assert_eq!(text(DocCommand::FirstPage), "Ctrl+Shift+PageUp");
        assert_eq!(text(DocCommand::LastPage), "Ctrl+Shift+PageDown");
        let all = shortcuts();
        let has = |m, k, c: DocCommand| {
            all.iter()
                .any(|(s, x)| s.modifiers == m && s.logical_key == k && *x == Command::Doc(c))
        };
        let cs = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        assert!(has(cs, Key::ArrowUp, DocCommand::FirstPage));
        assert!(has(cs, Key::ArrowDown, DocCommand::LastPage));
    }

    #[test]
    fn single_key_shortcuts_only_with_the_preference() {
        use app_state::tools::Tool;
        let plain = |all: &[(KeyboardShortcut, Command)], k: Key| {
            all.iter()
                .find(|(s, _)| s.modifiers == Modifiers::NONE && s.logical_key == k)
                .map(|(_, c)| c.clone())
        };
        let off = shortcuts_for(false);
        for k in [
            Key::V,
            Key::H,
            Key::Z,
            Key::S,
            Key::U,
            Key::D,
            Key::X,
            Key::K,
        ] {
            assert_eq!(plain(off, k), None, "{k:?} sem a preferência");
        }
        let on = shortcuts_for(true);
        let tool = |t| Some(Command::Doc(DocCommand::Tool(t)));
        assert_eq!(plain(on, Key::V), tool(Tool::Select));
        assert_eq!(plain(on, Key::H), tool(Tool::Hand));
        assert_eq!(plain(on, Key::Z), tool(Tool::Zoom));
        assert_eq!(plain(on, Key::S), tool(Tool::Note));
        assert_eq!(plain(on, Key::U), tool(Tool::Highlight));
        assert_eq!(plain(on, Key::D), tool(Tool::Ink));
        assert_eq!(plain(on, Key::X), tool(Tool::TextBox));
        assert_eq!(plain(on, Key::K), tool(Tool::Stamp));
        assert!(on.iter().any(|(s, c)| s.modifiers == Modifiers::SHIFT
            && s.logical_key == Key::U
            && *c == Command::Doc(DocCommand::CycleMarkup)));
        let k = shortcut_of(&Command::Preferences).unwrap();
        assert_eq!(shortcut_text(&k), "Ctrl+K");
    }

    #[test]
    fn search_and_help_keys_match_acrobat() {
        let text = |c: Command| shortcut_text(&shortcut_of(&c).unwrap());
        assert_eq!(text(Command::Doc(DocCommand::FindNext)), "F3");
        assert_eq!(text(Command::Doc(DocCommand::FindPrev)), "Shift+F3");
        let all = shortcuts();
        let has = |m: Modifiers, k: Key, c: DocCommand| {
            all.iter()
                .any(|(s, x)| s.modifiers == m && s.logical_key == k && *x == Command::Doc(c))
        };
        assert!(has(Modifiers::COMMAND, Key::G, DocCommand::FindNext));
        assert!(has(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::G,
            DocCommand::FindPrev
        ));
        assert_eq!(text(Command::Help), "F1");
        assert!(HELP_URL.starts_with("https://github.com/junior9283/gera-pdf-editor"));
    }

    #[test]
    fn zoom_shortcuts_match_acrobat() {
        let text = |c| shortcut_text(&shortcut_of(&Command::Doc(c)).unwrap());
        assert_eq!(text(DocCommand::Fit(Fit::Visible)), "Ctrl+3");
        assert_eq!(text(DocCommand::ZoomTo), "Ctrl+Y");
        assert_eq!(text(DocCommand::Fit(Fit::Page)), "Ctrl+0");
        assert_eq!(text(DocCommand::Fit(Fit::Width)), "Ctrl+2");
    }

    #[test]
    fn tool_commands_are_labelled_from_the_catalog() {
        assert_eq!(
            Command::Tool(app_state::tools_catalog::EXPORT).label(),
            app_state::tools_catalog::find(app_state::tools_catalog::EXPORT)
                .unwrap()
                .name
        );
    }

    #[test]
    fn panel_commands_have_labels() {
        for p in app_state::prefs::LeftPanel::ALL {
            assert!(!Command::TogglePanel(p).label().is_empty());
        }
        assert!(!Command::ToggleTools.label().is_empty());
    }

    #[test]
    fn shortcuts_are_written_with_symbols_like_acrobat() {
        let text = |cmd| shortcut_text(&shortcut_of(&cmd).unwrap());
        assert_eq!(
            text(Command::Doc(DocCommand::Rotate { clockwise: true })),
            "Ctrl+Shift++"
        );
        assert_eq!(
            text(Command::Doc(DocCommand::Rotate { clockwise: false })),
            "Ctrl+Shift+-"
        );
        assert_eq!(text(Command::Doc(DocCommand::ZoomIn)), "Ctrl+=");
        assert_eq!(text(Command::Doc(DocCommand::ZoomOut)), "Ctrl+-");
        assert_eq!(text(Command::SaveAs), "Ctrl+Shift+S");
        assert_eq!(text(Command::FullScreen), "Ctrl+L");
        assert_eq!(text(Command::ReadMode), "Ctrl+H");
    }
}
