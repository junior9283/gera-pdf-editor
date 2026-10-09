//! Edição de anotações direto no documento em memória, com desfazer/refazer próprio.
//!
//! Cada mudança é uma [`Edit`] reversível: aplicá-la devolve a inversa. Apagar (e desfazer uma
//! criação) só **esconde** a anotação (flag Hidden) — assim desfazer a traz de volta idêntica,
//! mesmo que venha de outro programa. Ao salvar, as escondidas por nós saem de verdade.
use crate::doc::{annot_id, rgb_of};
use crate::error::{PdfError, Result};
use crate::types::PdfRect;
use mupdf::color::AnnotationColor;
use mupdf::pdf::{AnnotationFlags, PdfAnnotation, PdfAnnotationType, PdfDocument, PdfPage};
use mupdf::{Point, Rect};
use std::collections::HashSet;

/// Traços de um desenho (pontos da página).
pub(crate) type Strokes = Vec<Vec<(f32, f32)>>;

/// Mudança elementar e reversível numa anotação.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Edit {
    Hide {
        page: i32,
        id: i32,
    },
    Show {
        page: i32,
        id: i32,
    },
    /// Retângulo e, para desenho, os traços (mover e redimensionar).
    Geometry {
        page: i32,
        id: i32,
        rect: PdfRect,
        ink: Option<Strokes>,
    },
    Style {
        page: i32,
        id: i32,
        color: Option<[f32; 3]>,
        opacity: f32,
        width: Option<f32>,
    },
    Contents {
        page: i32,
        id: i32,
        text: String,
    },
}

impl Edit {
    pub(crate) fn page(&self) -> i32 {
        match self {
            Self::Hide { page, .. }
            | Self::Show { page, .. }
            | Self::Geometry { page, .. }
            | Self::Style { page, .. }
            | Self::Contents { page, .. } => *page,
        }
    }
}

/// Inversas para desfazer, mudanças para refazer e as anotações escondidas por nós.
#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// (página, id) das apagadas e das criações desfeitas: saem do arquivo ao salvar.
    pub(crate) hidden: HashSet<(i32, i32)>,
}

impl History {
    /// Uma operação nova do usuário: guarda a inversa e esquece o que dava para refazer.
    pub(crate) fn record(&mut self, inverse: Edit) {
        self.undo.push(inverse);
        self.redo.clear();
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Desfaz o último passo; devolve a página mudada (`None` se não havia o que desfazer).
    pub(crate) fn undo(&mut self, doc: &PdfDocument) -> Result<Option<i32>> {
        let Some(inverse) = self.undo.pop() else {
            return Ok(None);
        };
        let page = inverse.page();
        match apply_edit(doc, &mut self.hidden, inverse.clone()) {
            Ok(forward) => {
                self.redo.push(forward);
                Ok(Some(page))
            }
            Err(e) => {
                self.undo.push(inverse);
                Err(e)
            }
        }
    }

    pub(crate) fn redo(&mut self, doc: &PdfDocument) -> Result<Option<i32>> {
        let Some(forward) = self.redo.pop() else {
            return Ok(None);
        };
        let page = forward.page();
        match apply_edit(doc, &mut self.hidden, forward.clone()) {
            Ok(inverse) => {
                self.undo.push(inverse);
                Ok(Some(page))
            }
            Err(e) => {
                self.redo.push(forward);
                Err(e)
            }
        }
    }
}

/// A anotação `id` da página `page` (com a página, para atualizar a aparência depois).
pub(crate) fn find(doc: &PdfDocument, page: i32, id: i32) -> Result<(PdfPage, PdfAnnotation)> {
    let page_ref = doc.load_pdf_page(page)?;
    let annot = page_ref
        .annotations()
        .find(|a| annot_id(a).ok() == Some(id))
        .ok_or_else(|| PdfError::InvalidAnnotation("anotação não encontrada".into()))?;
    Ok((page_ref, annot))
}

fn is_ink(a: &PdfAnnotation) -> Result<bool> {
    Ok(a.r#type()? == PdfAnnotationType::Ink)
}

fn strokes_of(a: &PdfAnnotation) -> Result<Strokes> {
    Ok(a.ink_list()?
        .into_iter()
        .map(|s| s.into_iter().map(|p| (p.x, p.y)).collect())
        .collect())
}

/// Retângulo e traços atuais (o estado que uma `Geometry` restaura).
pub(crate) fn geometry_of(
    doc: &PdfDocument,
    page: i32,
    id: i32,
) -> Result<(PdfRect, Option<Strokes>)> {
    let (_, a) = find(doc, page, id)?;
    // o desenho não tem /Rect próprio: o MuPDF o calcula a partir dos traços
    if is_ink(&a)? {
        Ok((a.bounds()?.into(), Some(strokes_of(&a)?)))
    } else {
        Ok((a.rect()?.into(), None))
    }
}

/// Aplica `edit` e devolve a mudança inversa (lida do estado de antes).
pub(crate) fn apply_edit(
    doc: &PdfDocument,
    hidden: &mut HashSet<(i32, i32)>,
    edit: Edit,
) -> Result<Edit> {
    let inverse = match edit {
        Edit::Hide { page, id } | Edit::Show { page, id } => {
            let hide = matches!(edit, Edit::Hide { .. });
            let (mut page_ref, mut a) = find(doc, page, id)?;
            let mut flags = a.flags()?;
            flags.set(AnnotationFlags::IS_HIDDEN, hide);
            a.set_flags(flags)?;
            a.update()?;
            page_ref.update()?;
            if hide {
                hidden.insert((page, id));
                Edit::Show { page, id }
            } else {
                hidden.remove(&(page, id));
                Edit::Hide { page, id }
            }
        }
        Edit::Geometry {
            page,
            id,
            rect,
            ink,
        } => {
            let (old_rect, old_ink) = geometry_of(doc, page, id)?;
            let (mut page_ref, mut a) = find(doc, page, id)?;
            match &ink {
                Some(strokes) => a.set_ink_list(
                    strokes
                        .iter()
                        .map(|s| s.iter().map(|&(x, y)| Point::new(x, y))),
                )?,
                None => a.set_rect(Rect::from(rect))?,
            }
            a.update()?;
            page_ref.update()?;
            Edit::Geometry {
                page,
                id,
                rect: old_rect,
                ink: old_ink,
            }
        }
        Edit::Style {
            page,
            id,
            color,
            opacity,
            width,
        } => {
            let (mut page_ref, mut a) = find(doc, page, id)?;
            let old = Edit::Style {
                page,
                id,
                color: a.color()?.map(rgb_of),
                opacity: a.opacity()?,
                width: if is_ink(&a)? {
                    Some(a.border_width()?)
                } else {
                    None
                },
            };
            if let Some([red, green, blue]) = color {
                a.set_color(AnnotationColor::Rgb { red, green, blue })?;
            }
            a.set_opacity(opacity.clamp(0.0, 1.0))?;
            if let Some(w) = width {
                a.set_border_width(w.max(0.0))?;
            }
            a.update()?;
            page_ref.update()?;
            old
        }
        Edit::Contents { page, id, text } => {
            let (mut page_ref, mut a) = find(doc, page, id)?;
            let old = a.contents()?.unwrap_or_default().to_owned();
            a.set_contents(&text)?;
            a.update()?;
            page_ref.update()?;
            Edit::Contents {
                page,
                id,
                text: old,
            }
        }
    };
    Ok(inverse)
}

/// Desloca o retângulo e os traços.
pub(crate) fn moved(
    rect: PdfRect,
    ink: Option<Strokes>,
    dx: f32,
    dy: f32,
) -> (PdfRect, Option<Strokes>) {
    let r = PdfRect::new(rect.x0 + dx, rect.y0 + dy, rect.x1 + dx, rect.y1 + dy);
    let ink = ink.map(|s| {
        s.into_iter()
            .map(|st| st.into_iter().map(|(x, y)| (x + dx, y + dy)).collect())
            .collect()
    });
    (r, ink)
}

/// Leva os traços para que os limites do desenho passem de `from` para `to`. Os limites incluem
/// uma margem (espessura do traço) em volta dos pontos: ela é mantida, só os pontos escalam.
pub(crate) fn scaled(from: PdfRect, to: PdfRect, ink: Option<Strokes>) -> Option<Strokes> {
    let ink = ink?;
    let points = ink.iter().flatten();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in points {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    if x0 > x1 {
        return Some(ink); // desenho sem pontos
    }
    let (pad_l, pad_t, pad_r, pad_b) = (x0 - from.x0, y0 - from.y0, from.x1 - x1, from.y1 - y1);
    let inner = PdfRect::new(to.x0 + pad_l, to.y0 + pad_t, to.x1 - pad_r, to.y1 - pad_b);
    let sx = inner.width() / (x1 - x0).max(0.001);
    let sy = inner.height() / (y1 - y0).max(0.001);
    Some(
        ink.into_iter()
            .map(|st| {
                st.into_iter()
                    .map(|(x, y)| (inner.x0 + (x - x0) * sx, inner.y0 + (y - y0) * sy))
                    .collect()
            })
            .collect(),
    )
}
