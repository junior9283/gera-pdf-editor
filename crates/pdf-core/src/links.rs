//! Links das páginas: para onde cada um leva, já filtrado pelo que é seguro seguir.
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::types::{
    InitialLayout, InitialPanel, InitialView, InitialZoom, LinkInfo, LinkTarget, PdfRect,
};
use mupdf::pdf::{FileSpec, LinkAction, PdfAction, PdfDestination, PdfObject, PdfPage};
use mupdf::DestinationKind;
use std::path::{Component, Path, PathBuf};

/// Esquemas que podem abrir (depois do aviso de segurança), como no Acrobat.
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];
/// Endereço maior que isso é recusado (um PDF malicioso poderia travar a dica e o aviso).
const MAX_URI_BYTES: usize = 8 * 1024;
/// O alvo recusado guarda só o começo, para a mensagem.
const BLOCKED_SHOWN_CHARS: usize = 200;

impl PdfDoc {
    /// Links da página `page`. Os que não se resolvem (malformados, destino inexistente) ficam de
    /// fora: um PDF quebrado nunca derruba a leitura.
    pub fn links(&self, page: i32) -> Result<Vec<LinkInfo>> {
        self.check_page(page)?;
        self.links_in(&self.inner.load_pdf_page(page)?, page, self.page_count()?)
    }

    /// Links da página já carregada (`count`: páginas do documento).
    pub(crate) fn links_in(
        &self,
        pdf_page: &PdfPage,
        page: i32,
        count: i32,
    ) -> Result<Vec<LinkInfo>> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        Ok(pdf_page
            .resolved_links()?
            .flatten()
            .filter_map(|link| {
                let rect = PdfRect::from(link.bounds);
                let usable = rect.width() > 0.0 && rect.height() > 0.0;
                let target = target_of(link.action, count, dir)?;
                usable.then_some(LinkInfo { page, rect, target })
            })
            .collect())
    }
}

impl PdfDoc {
    /// Como o arquivo pede para ser aberto. O que não se entende é ignorado.
    pub(crate) fn initial_view(&self) -> InitialView {
        let Ok(catalog) = self.inner.catalog() else {
            return InitialView::default();
        };
        let name = |key: &str| crate::pages::name_of(&catalog, key).unwrap_or_default();
        let mode = name("PageMode");
        let mut view = InitialView {
            panel: match mode.as_str() {
                "UseOutlines" => Some(InitialPanel::Bookmarks),
                "UseThumbs" => Some(InitialPanel::Thumbnails),
                "UseAttachments" => Some(InitialPanel::Attachments),
                _ => None,
            },
            full_screen: mode == "FullScreen",
            layout: match name("PageLayout").as_str() {
                "SinglePage" => Some(InitialLayout::Single),
                "OneColumn" => Some(InitialLayout::OneColumn),
                "TwoColumnLeft" => Some(InitialLayout::TwoColumn { cover: false }),
                "TwoColumnRight" => Some(InitialLayout::TwoColumn { cover: true }),
                "TwoPageLeft" => Some(InitialLayout::TwoPage { cover: false }),
                "TwoPageRight" => Some(InitialLayout::TwoPage { cover: true }),
                _ => None,
            },
            ..InitialView::default()
        };
        if let Some((page, zoom)) = catalog
            .get_dict("OpenAction")
            .ok()
            .flatten()
            .and_then(|a| self.open_action(&a))
        {
            view.page = Some(page);
            view.zoom = zoom;
        }
        view
    }

    /// Página e ampliação de `/OpenAction` (destino, ou ação GoTo). Outras ações (JavaScript…)
    /// são ignoradas.
    fn open_action(&self, action: &PdfObject) -> Option<(i32, Option<InitialZoom>)> {
        let dest = if action.is_dict().ok()? {
            let kind = action.get_dict("S").ok()??;
            (kind.as_name().ok()? == b"GoTo").then_some(())?;
            action.get_dict("D").ok()??
        } else {
            action.clone()
        };
        let count = self.page_count().ok()?;
        if dest.is_string().ok()? || dest.is_name().ok()? {
            let name = if dest.is_string().ok()? {
                dest.as_string().ok()?.to_string()
            } else {
                String::from_utf8_lossy(&dest.as_name().ok()?).into_owned()
            };
            let d = self
                .inner
                .resolve_link(&format!("#nameddest={name}"))
                .ok()??;
            let page = i32::try_from(d.loc.page_number)
                .ok()
                .filter(|p| *p < count)?;
            return Some((page, None));
        }
        (dest.is_array().ok()? && dest.len().ok()? >= 2).then_some(())?;
        let page_obj = dest.get_array(0).ok()??;
        let page = self.inner.lookup_page_number(&page_obj).ok()?;
        (0..count).contains(&page).then_some(())?;
        let kind = dest.get_array(1).ok()??.as_name().ok()?;
        let zoom = match kind.as_slice() {
            b"Fit" | b"FitB" => Some(InitialZoom::FitPage),
            b"FitH" | b"FitBH" => Some(InitialZoom::FitWidth),
            b"XYZ" => dest
                .get_array(4)
                .ok()
                .flatten()
                .and_then(|z| z.as_float().ok())
                .filter(|z| z.is_finite() && *z > 0.0)
                .map(InitialZoom::Factor),
            _ => None,
        };
        Some((page, zoom))
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
                FileSpec::Path(p) if is_pdf(&p) => LinkTarget::File {
                    path: absolute(dir, &p),
                    page,
                },
                FileSpec::Path(p) => blocked(p),
                FileSpec::Url(u) => web(u),
            })
        }
        PdfAction::Launch(FileSpec::Path(p)) if is_pdf(&p) => Some(LinkTarget::File {
            path: absolute(dir, &p),
            page: None,
        }),
        PdfAction::Launch(FileSpec::Path(p) | FileSpec::Url(p)) => Some(blocked(p)),
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
    if uri.len() <= MAX_URI_BYTES && scheme.is_some_and(|s| ALLOWED_SCHEMES.contains(&s.as_str())) {
        LinkTarget::Uri(uri)
    } else {
        blocked(uri)
    }
}

/// Alvo recusado, com só o começo do texto (que vem do PDF e pode ser enorme).
fn blocked(target: String) -> LinkTarget {
    let mut shown: String = target.chars().take(BLOCKED_SHOWN_CHARS).collect();
    if shown.len() < target.len() {
        shown.push('…');
    }
    LinkTarget::Blocked(shown)
}

/// PDF de nome razoável (um caminho enorme vindo do PDF é recusado).
fn is_pdf(path: &str) -> bool {
    path.len() <= 4096
        && Path::new(path)
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
    fn remote_target_must_be_a_pdf() {
        let goto_r = |p: &str| PdfAction::GoToR {
            file: FileSpec::Path(p.into()),
            dest: PdfDestination::Page {
                page: 0,
                kind: DestinationKind::Fit,
            },
        };
        let t = target_of(
            LinkAction::Action(goto_r("../../bin/sh")),
            1,
            Path::new("/a"),
        );
        assert!(matches!(t, Some(LinkTarget::Blocked(_))), "{t:?}");
        let t = target_of(LinkAction::Action(goto_r("x.PDF")), 1, Path::new("/a"));
        assert!(matches!(t, Some(LinkTarget::File { .. })), "{t:?}");
    }

    #[test]
    fn schemes_are_case_insensitive() {
        assert!(matches!(web("HTTPS://x".into()), LinkTarget::Uri(_)));
        assert!(matches!(web("sem-esquema".into()), LinkTarget::Blocked(_)));
    }
}
