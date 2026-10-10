//! Links das páginas: para onde cada um leva, já filtrado pelo que é seguro seguir.
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::types::{LinkInfo, LinkTarget, PdfRect};
use mupdf::pdf::{FileSpec, LinkAction, PdfAction, PdfDestination};
use mupdf::DestinationKind;
use std::path::{Component, Path, PathBuf};

/// Esquemas que podem abrir (depois do aviso de segurança), como no Acrobat.
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

impl PdfDoc {
    /// Links da página `page`. Os que não se resolvem (malformados, destino inexistente) ficam de
    /// fora: um PDF quebrado nunca derruba a leitura.
    pub fn links(&self, page: i32) -> Result<Vec<LinkInfo>> {
        self.check_page(page)?;
        let count = self.page_count()?;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let pdf_page = self.inner.load_pdf_page(page)?;
        Ok(pdf_page
            .resolved_links()?
            .flatten()
            .filter_map(|link| {
                let rect = PdfRect::from(link.bounds);
                let usable = rect.x1 > rect.x0 && rect.y1 > rect.y0;
                let target = target_of(link.action, count, dir)?;
                usable.then_some(LinkInfo { page, rect, target })
            })
            .collect())
    }

    /// Links de todas as páginas (na abertura). Uma página ilegível só perde os seus.
    pub(crate) fn all_links(&self) -> Vec<LinkInfo> {
        (0..self.page_count().unwrap_or(0))
            .flat_map(|p| self.links(p).unwrap_or_default())
            .collect()
    }
}

fn target_of(action: LinkAction, page_count: i32, dir: &Path) -> Option<LinkTarget> {
    let action = match action {
        LinkAction::Action(a) => a,
        LinkAction::Dest(d) => PdfAction::GoTo(d),
    };
    match action {
        PdfAction::GoTo(PdfDestination::Page { page, kind }) => {
            let page = i32::try_from(page).ok().filter(|p| *p < page_count)?;
            Some(LinkTarget::Page {
                page,
                at: point_of(kind),
            })
        }
        PdfAction::GoTo(PdfDestination::Named(_)) => None, // o MuPDF não achou o nome
        PdfAction::GoToR { file, dest } => {
            let page = match dest {
                PdfDestination::Page { page, .. } => i32::try_from(page).ok(),
                PdfDestination::Named(_) => None,
            };
            Some(match file {
                FileSpec::Path(p) => LinkTarget::File {
                    path: absolute(dir, &p),
                    page,
                },
                FileSpec::Url(u) => web(u),
            })
        }
        PdfAction::Launch(FileSpec::Path(p)) if is_pdf(&p) => Some(LinkTarget::File {
            path: absolute(dir, &p),
            page: None,
        }),
        PdfAction::Launch(FileSpec::Path(p) | FileSpec::Url(p)) => Some(LinkTarget::Blocked(p)),
        PdfAction::Uri(u) => Some(web(u)),
    }
}

/// Ponto de destino (canto superior esquerdo da vista), quando o destino diz qual.
fn point_of(kind: DestinationKind) -> Option<(f32, f32)> {
    let (x, y) = match kind {
        DestinationKind::XYZ { left, top, .. } => (left, top),
        DestinationKind::FitH { top } | DestinationKind::FitBH { top } => (None, top),
        DestinationKind::FitV { left } | DestinationKind::FitBV { left } => (left, None),
        DestinationKind::FitR { left, top, .. } => (Some(left), Some(top)),
        DestinationKind::Fit | DestinationKind::FitB => (None, None),
    };
    let ok = |v: Option<f32>| v.filter(|v| v.is_finite());
    match (ok(x), ok(y)) {
        (None, None) => None,
        (x, y) => Some((x.unwrap_or(0.0), y.unwrap_or(0.0))),
    }
}

/// Endereço web: só os esquemas permitidos abrem; o resto é bloqueado.
fn web(uri: String) -> LinkTarget {
    let scheme = uri
        .split_once(':')
        .map(|(s, _)| s.trim().to_ascii_lowercase());
    if scheme.is_some_and(|s| ALLOWED_SCHEMES.contains(&s.as_str())) {
        LinkTarget::Uri(uri)
    } else {
        LinkTarget::Blocked(uri)
    }
}

fn is_pdf(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Caminho relativo ao documento vira absoluto, sem `.` e `..`.
fn absolute(dir: &Path, path: &str) -> PathBuf {
    let joined = dir.join(path);
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_cleaned() {
        assert_eq!(
            absolute(Path::new("/a/b"), "../c/./d.pdf"),
            PathBuf::from("/a/c/d.pdf")
        );
        assert_eq!(absolute(Path::new("/a"), "/x.pdf"), PathBuf::from("/x.pdf"));
    }

    #[test]
    fn schemes_are_case_insensitive() {
        assert!(matches!(web("HTTPS://x".into()), LinkTarget::Uri(_)));
        assert!(matches!(web("sem-esquema".into()), LinkTarget::Blocked(_)));
    }
}
