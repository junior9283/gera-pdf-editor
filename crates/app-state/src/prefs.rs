//! Preferências da interface, guardadas entre sessões.
use crate::layout::ViewMode;
use crate::link_policy::{site_key, LinkDecision, MAX_SITES};
use crate::store;
use crate::view_history::ViewEntry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    Signatures,
    Attachments,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 5] = [
        LeftPanel::Thumbnails,
        LeftPanel::Outline,
        LeftPanel::Annotations,
        LeftPanel::Signatures,
        LeftPanel::Attachments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "Miniaturas",
            LeftPanel::Outline => "Marcadores",
            LeftPanel::Annotations => "Anotações",
            LeftPanel::Signatures => "Assinaturas",
            LeftPanel::Attachments => "Anexos",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "🖹",
            LeftPanel::Outline => "☰",
            LeftPanel::Annotations => "💬",
            LeftPanel::Signatures => "✒",
            LeftPanel::Attachments => "📎",
        }
    }
}

/// Zoom dos documentos novos (Preferências → Exibição de página).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum DefaultZoom {
    /// Ajustar à largura (o que o app sempre fez).
    #[default]
    Auto,
    FitPage,
    FitWidth,
    FitHeight,
    Level(f32),
}

/// Maior número de recentes que dá para escolher (o que a lista guarda).
pub const MAX_RECENT_SETTING: usize = crate::recents::MAX_RECENTS;
/// Arquivos com a última vista lembrada.
pub const MAX_LAST_VIEWS: usize = 200;

/// Clicar no painel aberto o fecha; em outro, troca para ele.
pub fn toggled(open: Option<LeftPanel>, p: LeftPanel) -> Option<LeftPanel> {
    (open != Some(p)).then_some(p)
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
    /// Escolhas lembradas no aviso de segurança dos links (site → permitir/bloquear).
    pub link_sites: BTreeMap<String, LinkDecision>,
    /// Usar aceleradores de tecla única (V, H, Z, S, U, D, X, K).
    pub single_key: bool,
    /// Layout dos documentos novos; `None`: o do app (rolagem contínua).
    pub default_mode: Option<ViewMode>,
    pub default_zoom: DefaultZoom,
    /// Restaurar a última vista ao reabrir documentos.
    pub restore_views: bool,
    /// Quantos recentes mostrar.
    pub max_recents: usize,
    /// Última vista de cada arquivo (o mais recente no fim).
    pub last_views: Vec<(String, ViewEntry)>,
    /// Largura das miniaturas de página (Ampliar/Reduzir miniaturas).
    pub thumb_width: f32,
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
            link_sites: BTreeMap::new(),
            single_key: false,
            default_mode: None,
            default_zoom: DefaultZoom::Auto,
            restore_views: true,
            max_recents: 20,
            last_views: Vec::new(),
            thumb_width: 110.0,
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

    /// Guarda a vista do arquivo `path` (o mais antigo sai depois de `MAX_LAST_VIEWS`).
    pub fn remember_view(&mut self, path: &str, view: ViewEntry) {
        self.last_views.retain(|(p, _)| p != path);
        if self.last_views.len() >= MAX_LAST_VIEWS {
            self.last_views.remove(0);
        }
        self.last_views.push((path.to_owned(), view));
    }

    pub fn last_view(&self, path: &str) -> Option<ViewEntry> {
        self.last_views
            .iter()
            .find(|(p, _)| p == path)
            .map(|&(_, v)| v)
    }

    /// Abre o painel `p`, ou fecha se ele já estiver aberto.
    pub fn toggle_left(&mut self, p: LeftPanel) {
        self.left_panel = toggled(self.left_panel, p);
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
        // só chaves de site bem formadas, até o teto (arquivo editado à mão ou corrompido)
        self.link_sites = std::mem::take(&mut self.link_sites)
            .into_iter()
            .filter(|(k, _)| site_key(k).as_deref() == Some(k.as_str()))
            .take(MAX_SITES)
            .collect();
        self.max_recents = self.max_recents.clamp(1, MAX_RECENT_SETTING);
        self.thumb_width = if self.thumb_width.is_finite() {
            self.thumb_width.clamp(80.0, 320.0)
        } else {
            defaults.thumb_width
        };
        if let DefaultZoom::Level(z) = self.default_zoom {
            if !(z.is_finite() && z > 0.0) {
                self.default_zoom = DefaultZoom::Auto;
            }
        }
        let skip = self.last_views.len().saturating_sub(MAX_LAST_VIEWS);
        self.last_views.drain(..skip);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_defaults_match_acrobat() {
        let p = Prefs::default();
        assert!(
            !p.single_key,
            "aceleradores de tecla única desligados de fábrica"
        );
        assert!(p.restore_views);
        assert_eq!(p.max_recents, 20);
        assert_eq!((p.default_mode, p.default_zoom), (None, DefaultZoom::Auto));
    }

    #[test]
    fn last_views_are_capped() {
        use crate::view_history::ViewEntry;
        let mut p = Prefs::default();
        let e = ViewEntry {
            page: 3,
            y: 1.0,
            zoom: 1.5,
            fit: crate::zoom::Fit::None,
        };
        for i in 0..MAX_LAST_VIEWS + 30 {
            p.remember_view(&format!("/doc{i}.pdf"), e);
        }
        assert_eq!(p.last_views.len(), MAX_LAST_VIEWS);
        assert_eq!(p.last_view("/doc0.pdf"), None, "o mais antigo sai");
        assert_eq!(
            p.last_view(&format!("/doc{}.pdf", MAX_LAST_VIEWS + 29)),
            Some(e)
        );
        // de novo o mesmo arquivo: vira o mais recente, sem duplicar
        p.remember_view("/doc40.pdf", e);
        assert_eq!(p.last_views.len(), MAX_LAST_VIEWS);
    }

    #[test]
    fn bad_prefs_are_sanitized() {
        let mut p = Prefs {
            max_recents: 0,
            ..Prefs::default()
        };
        assert_eq!(p.clone().sanitized().max_recents, 1);
        p.max_recents = 999;
        assert_eq!(p.clone().sanitized().max_recents, MAX_RECENT_SETTING);
        p.default_zoom = DefaultZoom::Level(f32::NAN);
        assert_eq!(p.sanitized().default_zoom, DefaultZoom::Auto);
    }

    #[test]
    fn bad_link_sites_are_dropped() {
        let mut p = Prefs::default();
        p.link_sites
            .insert("https://ex.com".into(), LinkDecision::Allow);
        p.link_sites
            .insert("HTTPS://Ex.com/x".into(), LinkDecision::Allow);
        p.link_sites.insert("lixo".into(), LinkDecision::Block);
        let p = p.sanitized();
        assert_eq!(p.link_sites.keys().collect::<Vec<_>>(), ["https://ex.com"]);
    }

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
        assert_eq!(labels.len(), LeftPanel::ALL.len());
    }
}
