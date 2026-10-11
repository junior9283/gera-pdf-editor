//! Arquivos abertos recentemente (aba Início e Arquivo → Abrir recente).
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Recentes guardados (a preferência escolhe quantos mostrar, até isso).
pub const MAX_RECENTS: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    /// Segundos desde 1970 (UTC).
    pub opened_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentFiles {
    items: Vec<Recent>,
}

impl RecentFiles {
    pub fn load(path: &Path) -> Self {
        let mut r: Self = store::load_json(path);
        r.items.truncate(MAX_RECENTS);
        r
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        store::save_json(path, self)
    }

    /// O mais recente primeiro.
    pub fn items(&self) -> &[Recent] {
        &self.items
    }

    pub fn add(&mut self, path: PathBuf, now: u64) {
        self.items.retain(|r| r.path != path);
        self.items.insert(
            0,
            Recent {
                path,
                opened_at: now,
            },
        );
        self.items.truncate(MAX_RECENTS);
    }

    /// Preferências → Documentos → Esquecer os documentos recentes.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|r| r.path != path);
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Há quanto tempo, em texto curto ("agora", "há 5 min", "há 3 h", "há 2 dias").
pub fn ago(now: u64, then: u64) -> String {
    match now.saturating_sub(then) {
        0..=59 => "agora".into(),
        s @ 60..=3599 => format!("há {} min", s / 60),
        s @ 3600..=86_399 => format!("há {} h", s / 3600),
        s => match s / 86_400 {
            1 => "há 1 dia".into(),
            d => format!("há {d} dias"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: usize) -> PathBuf {
        PathBuf::from(format!("/d/{n}.pdf"))
    }

    #[test]
    fn newest_first_without_duplicates_and_limited() {
        let mut r = RecentFiles::default();
        for n in 0..MAX_RECENTS + 5 {
            r.add(p(n), n as u64);
        }
        assert_eq!(r.items().len(), MAX_RECENTS);
        assert_eq!(r.items()[0].path, p(MAX_RECENTS + 4));
        r.add(p(10), 100);
        assert_eq!(
            r.items()[0],
            Recent {
                path: p(10),
                opened_at: 100
            }
        );
        assert_eq!(r.items().iter().filter(|x| x.path == p(10)).count(), 1);
        r.remove(&p(10));
        assert!(r.items().iter().all(|x| x.path != p(10)));
    }

    #[test]
    fn load_tolerates_extra_fields_and_trims_long_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recentes.json");
        let items: Vec<String> = (0..MAX_RECENTS + 10)
            .map(|n| format!(r#"{{"path":"/d/{n}.pdf","opened_at":{n},"cor":"azul"}}"#))
            .collect();
        std::fs::write(
            &path,
            format!(r#"{{"items":[{}],"versao":9}}"#, items.join(",")),
        )
        .unwrap();
        let r = RecentFiles::load(&path);
        assert_eq!(r.items().len(), MAX_RECENTS);
        assert_eq!(r.items()[0].path, p(0));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recentes.json");
        let mut r = RecentFiles::default();
        r.add(p(1), 5);
        r.save(&path).unwrap();
        assert_eq!(RecentFiles::load(&path), r);
    }

    #[test]
    fn ago_is_short_and_human() {
        assert_eq!(ago(1000, 1000), "agora");
        assert_eq!(
            ago(1000, 2000),
            "agora",
            "datas no futuro contam como agora"
        );
        assert_eq!(ago(10_000, 10_000 - 300), "há 5 min");
        assert_eq!(ago(100_000, 100_000 - 7200), "há 2 h");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400), "há 1 dia");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "há 3 dias");
    }
}
