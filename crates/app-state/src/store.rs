//! Arquivos JSON do app na pasta de configuração, gravados sem risco de deixar um arquivo pela metade.
use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "gera-pdf-editor";

/// Arquivo `name` na pasta de configuração do app (`$XDG_CONFIG_HOME` ou `~/.config`).
pub fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(APP_DIR).join(name))
}

/// Lê `path`; arquivo ausente, ilegível ou inválido vira o valor padrão (e não é apagado).
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Grava em um temporário na mesma pasta, força ao disco e só então troca o arquivo.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("caminho sem pasta"))?;
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(&bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Sample {
        n: u32,
        s: String,
    }

    #[test]
    fn saves_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/pasta/x.json");
        let v = Sample {
            n: 7,
            s: "olá".into(),
        };
        save_json(&path, &v).unwrap();
        assert_eq!(load_json::<Sample>(&path), v);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "o temporário some"
        );
    }

    #[test]
    fn corrupt_or_missing_file_gives_default_and_keeps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        assert_eq!(load_json::<Sample>(&path), Sample::default());
        std::fs::write(&path, b"{ isto nao e json").unwrap();
        assert_eq!(load_json::<Sample>(&path), Sample::default());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"{ isto nao e json",
            "nada é apagado ao ler"
        );
    }

    #[test]
    fn unwritable_location_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("arquivo");
        std::fs::write(&file, b"x").unwrap();
        // a "pasta" de destino é um arquivo comum
        assert!(save_json(&file.join("x.json"), &Sample::default()).is_err());
    }
}
