//! Carimbos personalizados do usuário, como no Acrobat: imagens (PNG) em categorias, guardadas
//! em `<configuração>/stamps/<categoria>/<nome>.png`. Nada sai dessa pasta: nomes são saneados e
//! links simbólicos ignorados.
use std::io;
use std::path::{Path, PathBuf};

/// Maior nome de carimbo ou de categoria (caracteres).
const MAX_NAME: usize = 64;
/// Tetos da leitura do catálogo.
const MAX_CATEGORIES: usize = 200;
const MAX_STAMPS: usize = 500;
const EXT: &str = "png";

/// A pasta dos carimbos, na configuração do app.
pub fn base_dir() -> Option<PathBuf> {
    crate::store::config_file("stamps")
}

/// Nome aceitável para arquivo ou pasta: sem separadores, `..`, ponto inicial nem caracteres de
/// controle; até 64 caracteres.
pub fn sanitize(name: &str) -> Option<String> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control());
    ok.then(|| name.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct StampEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub name: String,
    pub stamps: Vec<StampEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalog {
    pub categories: Vec<Category>,
}

/// Entradas reais (não links) de `dir`, em ordem de nome.
fn entries(dir: &Path, want_dir: bool, cap: usize) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_ok_and(|t| {
                !t.is_symlink() && t.is_dir() == want_dir && (want_dir || t.is_file())
            })
        })
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .take(cap)
        .collect();
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

impl Catalog {
    /// Lê as categorias (pastas) e os carimbos (PNG) de `base`; vazias ficam de fora.
    pub fn load(base: &Path) -> Self {
        let categories = entries(base, true, MAX_CATEGORIES)
            .into_iter()
            .filter_map(|(name, dir)| {
                sanitize(&name)?;
                let stamps: Vec<StampEntry> = entries(&dir, false, MAX_STAMPS)
                    .into_iter()
                    .filter_map(|(file, path)| {
                        let stem = file.strip_suffix(&format!(".{EXT}"))?;
                        Some(StampEntry {
                            name: sanitize(stem)?,
                            path,
                        })
                    })
                    .collect();
                (!stamps.is_empty()).then_some(Category { name, stamps })
            })
            .collect();
        Self { categories }
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// Caminho do carimbo, conferindo os nomes e que a pasta da categoria não é um link.
fn stamp_path(base: &Path, category: &str, name: &str) -> io::Result<(PathBuf, PathBuf)> {
    let category = sanitize(category).ok_or_else(|| invalid("nome de categoria inválido"))?;
    let name = sanitize(name).ok_or_else(|| invalid("nome de carimbo inválido"))?;
    let dir = base.join(category);
    if dir.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
        return Err(invalid("a categoria não é uma pasta"));
    }
    Ok((dir.clone(), dir.join(format!("{name}.{EXT}"))))
}

/// Copia a imagem `src` (já pronta, PNG) para o catálogo; devolve onde ficou.
pub fn create(base: &Path, category: &str, name: &str, src: &Path) -> io::Result<PathBuf> {
    let (dir, dest) = stamp_path(base, category, name)?;
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "já existe um carimbo com esse nome nessa categoria",
        ));
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

/// Muda o nome do carimbo.
pub fn rename(base: &Path, category: &str, name: &str, to: &str) -> io::Result<()> {
    let (_, from) = stamp_path(base, category, name)?;
    let (_, dest) = stamp_path(base, category, to)?;
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "nome já usado",
        ));
    }
    std::fs::rename(from, dest)
}

/// Leva o carimbo para outra categoria (criada se não existir); a antiga, vazia, sai.
pub fn move_to(base: &Path, category: &str, name: &str, to: &str) -> io::Result<()> {
    let (old_dir, from) = stamp_path(base, category, name)?;
    let (new_dir, dest) = stamp_path(base, to, name)?;
    if dest.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "nome já usado na categoria",
        ));
    }
    std::fs::create_dir_all(&new_dir)?;
    std::fs::rename(from, dest)?;
    let _ = std::fs::remove_dir(old_dir); // só sai se ficou vazia
    Ok(())
}

/// Exclui o carimbo (e a categoria, se ficar vazia).
pub fn remove(base: &Path, category: &str, name: &str) -> io::Result<()> {
    let (dir, path) = stamp_path(base, category, name)?;
    std::fs::remove_file(path)?;
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal_and_control_chars() {
        assert_eq!(
            sanitize("  Aprovado pela diretoria ").as_deref(),
            Some("Aprovado pela diretoria")
        );
        for bad in [
            "", "   ", "..", ".", "../x", "a/b", "a\\b", "a\u{0}b", "a\nb", ".oculto",
        ] {
            assert_eq!(sanitize(bad), None, "{bad:?}");
        }
        assert_eq!(sanitize(&"x".repeat(65)), None);
        assert!(sanitize(&"é".repeat(64)).is_some());
    }

    #[test]
    fn catalog_lists_categories_and_stamps() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("stamps");
        let src = dir.path().join("fonte.png");
        std::fs::write(&src, b"png").unwrap();
        let made = create(&base, "Jurídico", "Aprovado", &src).unwrap();
        assert!(made.starts_with(&base) && made.exists());
        create(&base, "Jurídico", "Revisado", &src).unwrap();
        create(&base, "Pessoal", "Assinatura", &src).unwrap();
        assert!(
            create(&base, "Jurídico", "Aprovado", &src).is_err(),
            "nome repetido"
        );
        assert!(create(&base, "../fora", "x", &src).is_err());
        let cat = Catalog::load(&base);
        let names: Vec<(&str, Vec<&str>)> = cat
            .categories
            .iter()
            .map(|c| {
                (
                    c.name.as_str(),
                    c.stamps.iter().map(|s| s.name.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("Jurídico", vec!["Aprovado", "Revisado"]),
                ("Pessoal", vec!["Assinatura"])
            ]
        );
        rename(&base, "Jurídico", "Revisado", "Conferido").unwrap();
        move_to(&base, "Jurídico", "Conferido", "Pessoal").unwrap();
        remove(&base, "Jurídico", "Aprovado").unwrap();
        let cat = Catalog::load(&base);
        assert_eq!(cat.categories.len(), 1, "categoria vazia some");
        assert_eq!(cat.categories[0].stamps.len(), 2);
        // link simbólico para fora da pasta não entra no catálogo
        std::os::unix::fs::symlink(dir.path(), base.join("Pessoal").join("link.png")).unwrap();
        assert_eq!(Catalog::load(&base).categories[0].stamps.len(), 2);
    }
}
