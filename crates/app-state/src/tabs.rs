//! Regras das abas: Início e Ferramentas fixas, seguidas dos documentos abertos.
use std::path::{Path, PathBuf};

pub const HOME: usize = 0;
pub const TOOLS: usize = 1;
/// Quantidade de abas fixas: os documentos começam neste índice.
pub const FIXED: usize = 2;

/// Identidade de um documento: o caminho canônico, ou o próprio caminho se não der para resolver.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), without_verbatim)
}

/// No Windows, o caminho canônico vem com o prefixo `\\?\` (`\\?\C:\pasta`,
/// `\\?\UNC\servidor\pasta`): o prefixo sai para mostrar e gravar como o usuário escreve.
fn without_verbatim(path: PathBuf) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path;
    };
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        // só unidade com letra (C:\...); outros caminhos verbatim ficam como estão
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// Nome do arquivo para a aba, com no máximo `max` caracteres (o excesso vira "…").
pub fn tab_title(path: &Path, max: usize) -> String {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned();
    shorten(&name, max)
}

/// O texto com no máximo `max` caracteres, terminando em "…" quando cortado.
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let keep: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{keep}…")
}

/// Abas abertas (só a identidade de cada documento) e qual está ativa.
#[derive(Debug, Default, Clone)]
pub struct TabList {
    docs: Vec<PathBuf>,
    active: usize,
}

impl TabList {
    pub fn len(&self) -> usize {
        FIXED + self.docs.len()
    }
    /// As abas fixas sempre existem.
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn docs(&self) -> &[PathBuf] {
        &self.docs
    }
    pub fn activate(&mut self, tab: usize) {
        if tab < self.len() {
            self.active = tab;
        }
    }
    /// Posição do documento da aba `tab` em `docs`, se for uma aba de documento.
    pub fn doc_index(&self, tab: usize) -> Option<usize> {
        tab.checked_sub(FIXED).filter(|&i| i < self.docs.len())
    }
    /// Caminho do documento da aba `tab`, se for uma aba de documento.
    pub fn doc_path(&self, tab: usize) -> Option<&Path> {
        self.doc_index(tab).map(|i| self.docs[i].as_path())
    }
    pub fn find(&self, path: &Path) -> Option<usize> {
        self.docs.iter().position(|p| p == path).map(|i| FIXED + i)
    }
    /// Ativa a aba de `path`, criando-a se preciso. Devolve `(aba, criada)`.
    pub fn open(&mut self, path: PathBuf) -> (usize, bool) {
        if let Some(tab) = self.find(&path) {
            self.active = tab;
            return (tab, false);
        }
        self.docs.push(path);
        self.active = self.len() - 1;
        (self.active, true)
    }
    /// Fecha a aba `tab` (as fixas não fecham). Devolve a posição do documento removido.
    pub fn close(&mut self, tab: usize) -> Option<usize> {
        let i = self.doc_index(tab)?;
        self.docs.remove(i);
        if self.active == tab {
            self.active = if tab < self.len() {
                tab
            } else if tab > FIXED {
                tab - 1
            } else {
                HOME
            };
        } else if self.active > tab {
            self.active -= 1;
        }
        Some(i)
    }
    pub fn rename(&mut self, tab: usize, path: PathBuf) {
        if let Some(i) = self.doc_index(tab) {
            self.docs[i] = path;
        }
    }
    pub fn next(&mut self) {
        self.active = (self.active + 1) % self.len();
    }
    pub fn prev(&mut self) {
        self.active = (self.active + self.len() - 1) % self.len();
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn verbatim_prefix_is_removed() {
        let p = |s: &str| without_verbatim(PathBuf::from(s));
        assert_eq!(
            p(r"\\?\C:\gera\Pasta Ação\a.pdf"),
            PathBuf::from(r"C:\gera\Pasta Ação\a.pdf")
        );
        assert_eq!(
            p(r"\\?\UNC\srv\docs\a.pdf"),
            PathBuf::from(r"\\srv\docs\a.pdf")
        );
        assert_eq!(
            p(r"\\?\Volume{1}\a.pdf"),
            PathBuf::from(r"\\?\Volume{1}\a.pdf")
        );
        assert_eq!(p("/home/x/a.pdf"), PathBuf::from("/home/x/a.pdf"));
    }

    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!("/docs/{name}.pdf"))
    }

    fn three() -> TabList {
        let mut t = TabList::default();
        for n in ["a", "b", "c"] {
            t.open(p(n));
        }
        t
    }

    #[test]
    fn starts_on_home_with_the_fixed_tabs() {
        let t = TabList::default();
        assert_eq!((t.len(), t.active()), (FIXED, HOME));
        assert_eq!(t.doc_index(HOME), None);
        assert_eq!(t.doc_index(TOOLS), None);
    }

    #[test]
    fn opening_activates_and_opening_again_reuses_the_tab() {
        let mut t = three();
        assert_eq!(t.active(), FIXED + 2);
        assert_eq!(t.open(p("a")), (FIXED, false));
        assert_eq!(t.active(), FIXED);
        assert_eq!(t.len(), FIXED + 3);
    }

    #[test]
    fn closing_the_active_tab_picks_the_right_then_left_neighbour_then_home() {
        let mut t = three();
        t.activate(FIXED + 1); // b
        assert_eq!(t.close(FIXED + 1), Some(1));
        assert_eq!(t.docs(), [p("a"), p("c")]);
        assert_eq!(t.active(), FIXED + 1, "c ficou no lugar de b");
        assert_eq!(t.close(FIXED + 1), Some(1)); // c, a última
        assert_eq!(t.active(), FIXED, "volta para a vizinha da esquerda");
        assert_eq!(t.close(FIXED), Some(0));
        assert_eq!(t.active(), HOME, "sem documentos, volta ao Início");
    }

    #[test]
    fn closing_another_tab_keeps_the_same_document_active() {
        let mut t = three(); // ativa: c
        t.close(FIXED);
        assert_eq!(t.doc_index(t.active()).map(|i| &t.docs()[i]), Some(&p("c")));
    }

    #[test]
    fn fixed_tabs_never_close() {
        let mut t = three();
        assert_eq!(t.close(HOME), None);
        assert_eq!(t.close(TOOLS), None);
        assert_eq!(t.close(99), None);
        assert_eq!(t.len(), FIXED + 3);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut t = three();
        t.next();
        assert_eq!(t.active(), HOME);
        t.prev();
        assert_eq!(t.active(), FIXED + 2);
    }

    #[test]
    fn rename_follows_save_as() {
        let mut t = three();
        assert_eq!(t.doc_path(HOME), None);
        assert_eq!(t.doc_path(FIXED), Some(p("a").as_path()));
        t.rename(FIXED, p("novo"));
        assert_eq!(t.find(&p("novo")), Some(FIXED));
        assert_eq!(t.find(&p("a")), None);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_unifies_relative_and_symlinked_paths() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.pdf");
        std::fs::write(&real, b"%PDF").unwrap();
        let link = dir.path().join("atalho.pdf");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let dotted = dir.path().join(".").join("real.pdf");
        assert_eq!(canonical(&link), canonical(&real));
        assert_eq!(canonical(&dotted), canonical(&real));
        let missing = dir.path().join("sumiu.pdf");
        assert_eq!(
            canonical(&missing),
            missing,
            "inexistente: devolve o próprio caminho"
        );
    }

    #[test]
    fn titles_are_the_file_name_truncated_on_char_boundaries() {
        assert_eq!(
            tab_title(Path::new("/x/relatório.pdf"), 40),
            "relatório.pdf"
        );
        assert_eq!(tab_title(Path::new("/x/ação-çãoçãoção.pdf"), 6), "ação-…");
        assert_eq!(tab_title(Path::new("/"), 10), "/");
    }
}
