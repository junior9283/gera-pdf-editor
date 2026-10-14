//! Arquivos JSON do app na pasta de configuração, gravados sem risco de deixar um arquivo pela metade.
use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const APP_DIR: &str = "gera-pdf-editor";

/// Pasta base de `kind` ("config" ou "cache"). Com `$XDG_CONFIG_HOME`/`$XDG_CACHE_HOME`
/// definidos, eles mandam (também nos testes). No Linux, `~/.config` ou `~/.cache`; no Windows, a
/// versão portátil guarda em `dados\` ao lado do .exe, ou em `%APPDATA%\Gera PDF Editor` se
/// a pasta do .exe não for gravável.
fn base_dir(kind: &str) -> Option<PathBuf> {
    let xdg = if kind == "config" {
        "XDG_CONFIG_HOME"
    } else {
        "XDG_CACHE_HOME"
    };
    if let Some(dir) = std::env::var_os(xdg).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join(APP_DIR));
    }
    #[cfg(windows)]
    {
        // resolvida uma vez: o teste de gravação não se repete a cada preferência salva
        static BASE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        let base = BASE.get_or_init(|| {
            let fallback =
                std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Gera PDF Editor"));
            let exe = std::env::current_exe().ok()?;
            portable_base(exe.parent()?, fallback)
        });
        Some(base.as_ref()?.join(kind))
    }
    #[cfg(not(windows))]
    {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(home.join(format!(".{kind}")).join(APP_DIR))
    }
}

/// `dados` ao lado do executável, se der para gravar ali (pendrive, pasta do usuário); senão
/// `fallback` (Arquivos de Programas, mídia somente leitura).
#[cfg_attr(not(windows), allow(dead_code))]
fn portable_base(exe_dir: &Path, fallback: Option<PathBuf>) -> Option<PathBuf> {
    let dir = exe_dir.join("dados");
    let probe = dir.join(".gravavel");
    let writable = std::fs::create_dir_all(&dir).is_ok()
        && std::fs::write(&probe, b"").is_ok()
        && std::fs::remove_file(&probe).is_ok();
    if writable {
        Some(dir)
    } else {
        fallback
    }
}

/// Pasta pessoal do usuário (`HOME`; no Windows, `USERPROFILE`).
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Arquivo `name` na pasta de configuração do app.
pub fn config_file(name: &str) -> Option<PathBuf> {
    Some(base_dir("config")?.join(name))
}

/// PDF novo criado pelo app (Criar PDF, abrir imagem) na pasta de documentos criados (no cache):
/// "<stem>.pdf", "<stem> (2).pdf"…, sem sobrescrever.
pub fn created_pdf(stem: &str) -> Option<PathBuf> {
    let dir = base_dir("cache")?.join("criados");
    std::fs::create_dir_all(&dir).ok()?;
    let first = dir.join(format!("{stem}.pdf"));
    if !first.exists() {
        return Some(first);
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).pdf")))
        .find(|p| !p.exists())
}

/// É uma imagem que o Criar PDF aceita (pela extensão)?
pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "tif" | "tiff" | "bmp" | "gif"
        )
    })
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
    #[test]
    fn portable_dir_next_to_exe() {
        let exe = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let base = portable_base(exe.path(), Some(other.path().to_path_buf())).unwrap();
        assert_eq!(base, exe.path().join("dados"));
        assert!(base.is_dir());
        assert_eq!(
            std::fs::read_dir(&base).unwrap().count(),
            0,
            "o teste de escrita não fica"
        );
    }

    #[cfg(unix)]
    #[test]
    fn falls_back_when_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let exe = tempfile::tempdir().unwrap();
        std::fs::set_permissions(exe.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let other = tempfile::tempdir().unwrap();
        let base = portable_base(exe.path(), Some(other.path().join("Gera PDF Editor")));
        std::fs::set_permissions(exe.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(base, Some(other.path().join("Gera PDF Editor")));
        assert!(!exe.path().join("dados").exists());
    }

    use super::*;

    #[test]
    fn images_are_recognised_by_extension() {
        assert!(is_image(Path::new("/x/Foto.JPG")) && is_image(Path::new("a.tiff")));
        assert!(!is_image(Path::new("a.pdf")) && !is_image(Path::new("sem_extensao")));
    }

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
