//! Arquivos JSON do app na pasta de configuração, gravados sem risco de deixar um arquivo pela metade.
use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

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
    write_atomic(path, &to_json(value)?)
}

fn to_json<T: Serialize>(value: &T) -> std::io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(std::io::Error::other)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("caminho sem pasta"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Grava arquivos JSON numa thread à parte (o fsync não trava a interface), na ordem dos pedidos:
/// uma thread só, então dois pedidos para o mesmo arquivo nunca disputam o temporário.
pub struct BackgroundSaver {
    /// `None` só durante o `drop`, para encerrar a thread.
    jobs: Option<Sender<(PathBuf, Vec<u8>)>>,
    errors: Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Default for BackgroundSaver {
    fn default() -> Self {
        let (jobs, rx) = mpsc::channel::<(PathBuf, Vec<u8>)>();
        let (err_tx, errors) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("gravacao".into())
            .spawn(move || {
                for (path, bytes) in rx {
                    if let Err(e) = write_atomic(&path, &bytes) {
                        let _ =
                            err_tx.send(format!("Não foi possível gravar {}: {e}", path.display()));
                    }
                }
            })
            .ok();
        Self {
            jobs: Some(jobs),
            errors,
            thread,
        }
    }
}

impl BackgroundSaver {
    /// Serializa agora (rápido) e grava em segundo plano. Os tipos do app sempre serializam.
    pub fn save<T: Serialize>(&self, path: PathBuf, value: &T) {
        if let (Ok(bytes), Some(jobs)) = (to_json(value), &self.jobs) {
            let _ = jobs.send((path, bytes));
        }
    }

    /// Última falha de gravação, se houver.
    pub fn take_error(&self) -> Option<String> {
        self.errors.try_iter().last()
    }
}

impl Drop for BackgroundSaver {
    /// Ao fechar o app, termina as gravações pendentes em vez de perdê-las.
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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

    fn wait_for(cond: impl Fn() -> bool) -> bool {
        (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            cond()
        })
    }

    #[test]
    fn background_saver_writes_in_order_and_reports_failures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        let saver = BackgroundSaver::default();
        for n in 1..=5 {
            saver.save(
                path.clone(),
                &Sample {
                    n,
                    s: String::new(),
                },
            );
        }
        assert!(
            wait_for(|| load_json::<Sample>(&path).n == 5),
            "o último pedido vence"
        );
        let file = dir.path().join("arquivo");
        std::fs::write(&file, b"x").unwrap();
        saver.save(file.join("x.json"), &Sample::default());
        assert!(wait_for(|| saver.take_error().is_some()));
    }

    #[test]
    fn dropping_the_saver_finishes_pending_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        let saver = BackgroundSaver::default();
        saver.save(
            path.clone(),
            &Sample {
                n: 9,
                s: "fim".into(),
            },
        );
        drop(saver); // o app fechando
        assert_eq!(load_json::<Sample>(&path).n, 9);
    }
}
