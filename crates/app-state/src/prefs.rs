//! Preferências da interface, guardadas entre sessões.
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MIN_PANEL: f32 = 120.0;
pub const MAX_PANEL: f32 = 600.0;

/// Painel aberto pelo trilho à esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LeftPanel {
    #[default]
    Thumbnails,
    Outline,
    Annotations,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 3] = [
        LeftPanel::Thumbnails,
        LeftPanel::Outline,
        LeftPanel::Annotations,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "Miniaturas",
            LeftPanel::Outline => "Índice",
            LeftPanel::Annotations => "Anotações",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "🖹",
            LeftPanel::Outline => "☰",
            LeftPanel::Annotations => "💬",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub dark: bool,
    pub left_panel: Option<LeftPanel>,
    pub left_width: f32,
    /// Painel de ferramentas à direita aberto?
    pub tools_panel: bool,
    pub tools_width: f32,
    /// Nome do usuário (Identidade): autor das anotações e linha dos carimbos dinâmicos.
    pub author: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            dark: false,
            left_panel: Some(LeftPanel::Thumbnails),
            left_width: 180.0,
            tools_panel: true,
            tools_width: 200.0,
            author: None,
        }
    }
}

impl Prefs {
    pub fn load(path: &Path) -> Self {
        store::load_json::<Self>(path).sanitized()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        store::save_json(path, self)
    }

    /// Abre o painel `p`, ou fecha se ele já estiver aberto.
    pub fn toggle_left(&mut self, p: LeftPanel) {
        self.left_panel = (self.left_panel != Some(p)).then_some(p);
    }

    /// Larguras dentro de `MIN_PANEL..=MAX_PANEL` (valores estranhos voltam ao padrão).
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        let fix = |w: f32, default: f32| {
            if w.is_finite() {
                w.clamp(MIN_PANEL, MAX_PANEL)
            } else {
                default
            }
        };
        self.left_width = fix(self.left_width, defaults.left_width);
        self.tools_width = fix(self.tools_width, defaults.tools_width);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = Prefs::load(&dir.path().join("prefs.json"));
        assert_eq!(p, Prefs::default());
        assert_eq!(p.left_panel, Some(LeftPanel::Thumbnails));
        assert!(p.tools_panel && !p.dark);
    }

    #[test]
    fn widths_are_kept_inside_the_limits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        std::fs::write(
            &path,
            br#"{"left_width": 5000, "tools_width": -3, "dark": true}"#,
        )
        .unwrap();
        let p = Prefs::load(&path);
        assert_eq!((p.left_width, p.tools_width), (MAX_PANEL, MIN_PANEL));
        assert!(p.dark, "os outros campos valem");
        assert_eq!(
            p.left_panel,
            Some(LeftPanel::Thumbnails),
            "campo ausente: padrão"
        );
    }

    #[test]
    fn toggling_a_panel_opens_or_closes_it() {
        let mut p = Prefs::default();
        p.toggle_left(LeftPanel::Thumbnails);
        assert_eq!(p.left_panel, None);
        p.toggle_left(LeftPanel::Outline);
        assert_eq!(p.left_panel, Some(LeftPanel::Outline));
    }

    #[test]
    fn panels_have_distinct_labels() {
        let labels: std::collections::HashSet<_> =
            LeftPanel::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(labels.len(), 3);
    }
}
