//! Edição de anotações direto no documento em memória, com desfazer/refazer próprio.
//!
//! Cada mudança é uma [`Edit`] reversível: aplicá-la devolve a inversa. Apagar (e desfazer uma
//! criação) só **esconde** a anotação (flag Hidden) — assim desfazer a traz de volta idêntica,
//! mesmo que venha de outro programa. Ao salvar, as escondidas por nós saem de verdade.
use crate::doc::{annot_id, rgb_of};
use crate::error::{PdfError, Result};
use crate::types::{PdfRect, Segment};
use mupdf::color::AnnotationColor;
use mupdf::pdf::{AnnotationFlags, PdfAnnotation, PdfAnnotationType, PdfDocument, PdfPage};
use mupdf::{Point, Rect};
use std::collections::HashSet;

/// Traços de um desenho (pontos da página).
pub(crate) type Strokes = Vec<Vec<(f32, f32)>>;

/// O que uma `Style` faz com a cor (/C).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColorEdit {
    Keep,
    /// A cor exata (inclusive cinza ou CMYK de outros programas, ao desfazer).
    Set(AnnotationColor),
    /// Sem cor (a anotação original não tinha /C).
    Clear,
}

/// Mudança elementar e reversível numa anotação.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Edit {
    /// Esconde (apagar, desfazer criar) ou mostra de novo.
    Visibility {
        page: i32,
        id: i32,
        hidden: bool,
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
        color: ColorEdit,
        opacity: f32,
        width: Option<f32>,
    },
    Contents {
        page: i32,
        id: i32,
        text: String,
    },
    /// Pontos da linha/seta (mover e puxar uma ponta).
    LinePoints {
        page: i32,
        id: i32,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// Preenchimento das formas (/IC) ou fundo da caixa de texto (/C).
    Fill {
        page: i32,
        id: i32,
        color: ColorEdit,
    },
    /// Fonte da caixa de texto (/DA).
    Font {
        page: i32,
        id: i32,
        size: f32,
        color: [f32; 3],
    },
}

impl Edit {
    pub(crate) fn page(&self) -> i32 {
        match self {
            Self::Visibility { page, .. }
            | Self::Geometry { page, .. }
            | Self::Style { page, .. }
            | Self::Contents { page, .. }
            | Self::LinePoints { page, .. }
            | Self::Fill { page, .. }
            | Self::Font { page, .. } => *page,
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
        step(doc, &mut self.hidden, &mut self.undo, &mut self.redo)
    }

    pub(crate) fn redo(&mut self, doc: &PdfDocument) -> Result<Option<i32>> {
        step(doc, &mut self.hidden, &mut self.redo, &mut self.undo)
    }
}

/// Aplica o topo de `from` e guarda a inversa em `to`; em erro, nada muda.
fn step(
    doc: &PdfDocument,
    hidden: &mut HashSet<(i32, i32)>,
    from: &mut Vec<Edit>,
    to: &mut Vec<Edit>,
) -> Result<Option<i32>> {
    let Some(edit) = from.pop() else {
        return Ok(None);
    };
    let page = edit.page();
    match apply_edit(doc, hidden, edit.clone()) {
        Ok(inverse) => {
            to.push(inverse);
            Ok(Some(page))
        }
        Err(e) => {
            from.push(edit);
            Err(e)
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

/// Traços no formato do MuPDF (o inverso de `strokes_of`).
pub(crate) fn ink_points(
    strokes: &[Vec<(f32, f32)>],
) -> impl Iterator<Item = impl Iterator<Item = Point> + '_> + '_ {
    strokes
        .iter()
        .map(|s| s.iter().map(|&(x, y)| Point::new(x, y)))
}

/// Retângulo e traços atuais (o estado que uma `Geometry` restaura).
fn geometry(a: &PdfAnnotation) -> Result<(PdfRect, Option<Strokes>)> {
    // o desenho não tem /Rect próprio: o MuPDF o calcula a partir dos traços
    if is_ink(a)? {
        Ok((a.bounds()?.into(), Some(strokes_of(a)?)))
    } else {
        Ok((a.rect()?.into(), None))
    }
}

/// Pontos atuais da linha `id`, ou `None` se a anotação não é linha.
pub(crate) fn line_of(doc: &PdfDocument, page: i32, id: i32) -> Result<Option<Segment>> {
    let (_, a) = find(doc, page, id)?;
    if a.r#type()? != PdfAnnotationType::Line {
        return Ok(None);
    }
    let (p, q) = a.line()?;
    Ok(Some(((p.x, p.y), (q.x, q.y))))
}

/// Geometria atual da anotação `id`.
pub(crate) fn geometry_of(
    doc: &PdfDocument,
    page: i32,
    id: i32,
) -> Result<(PdfRect, Option<Strokes>)> {
    geometry(&find(doc, page, id)?.1)
}

/// Aplica `edit` e devolve a mudança inversa (lida do estado de antes).
pub(crate) fn apply_edit(
    doc: &PdfDocument,
    hidden: &mut HashSet<(i32, i32)>,
    edit: Edit,
) -> Result<Edit> {
    let (page, id) = match &edit {
        Edit::Visibility { page, id, .. }
        | Edit::Geometry { page, id, .. }
        | Edit::Style { page, id, .. }
        | Edit::Contents { page, id, .. }
        | Edit::LinePoints { page, id, .. }
        | Edit::Fill { page, id, .. }
        | Edit::Font { page, id, .. } => (*page, *id),
    };
    let (mut page_ref, mut a) = find(doc, page, id)?;
    let inverse = match edit {
        Edit::Visibility { hidden: hide, .. } => {
            let mut flags = a.flags()?;
            flags.set(AnnotationFlags::IS_HIDDEN, hide);
            a.set_flags(flags)?;
            if hide {
                hidden.insert((page, id));
            } else {
                hidden.remove(&(page, id));
            }
            Edit::Visibility {
                page,
                id,
                hidden: !hide,
            }
        }
        Edit::Geometry { rect, ink, .. } => {
            let (old_rect, old_ink) = geometry(&a)?;
            match &ink {
                Some(strokes) => a.set_ink_list(ink_points(strokes))?,
                None => a.set_rect(Rect::from(rect))?,
            }
            Edit::Geometry {
                page,
                id,
                rect: old_rect,
                ink: old_ink,
            }
        }
        Edit::Style {
            color,
            opacity,
            width,
            ..
        } => {
            // só a espessura que vai mudar precisa voltar (vale para qualquer tipo com borda)
            let old_width = width.map(|_| a.border_width()).transpose()?;
            let old_color = match a.color()? {
                Some(c) => ColorEdit::Set(c),
                None => ColorEdit::Clear,
            };
            let old_opacity = a.opacity()?;
            if let Err(e) = set_style(&mut a, color, opacity, width) {
                // nada pela metade: volta ao estilo de antes
                let _ = set_style(&mut a, old_color, old_opacity, old_width);
                return Err(e);
            }
            Edit::Style {
                page,
                id,
                color: old_color,
                opacity: old_opacity,
                width: old_width,
            }
        }
        Edit::LinePoints { a: p, b: q, .. } => {
            let (op, oq) = a.line()?;
            a.set_line(Point::new(p.0, p.1), Point::new(q.0, q.1))?;
            Edit::LinePoints {
                page,
                id,
                a: (op.x, op.y),
                b: (oq.x, oq.y),
            }
        }
        Edit::Fill { color, .. } => {
            // a caixa de texto guarda o fundo em /C; as formas, em /IC
            let text_box = a.r#type()? == PdfAnnotationType::FreeText;
            let (key, current) = if text_box {
                ("C", a.color()?)
            } else {
                ("IC", a.interior_color()?)
            };
            let old = current.map_or(ColorEdit::Clear, ColorEdit::Set);
            match color {
                ColorEdit::Keep => {}
                ColorEdit::Set(c) if text_box => a.set_color(c)?,
                ColorEdit::Set(c) => a.set_interior_color(c)?,
                ColorEdit::Clear => a.object().dict_delete(key)?,
            }
            Edit::Fill {
                page,
                id,
                color: old,
            }
        }
        Edit::Font {
            size,
            color: [red, green, blue],
            ..
        } => {
            let da = a
                .default_appearance()?
                .ok_or_else(|| PdfError::InvalidAnnotation("anotação sem fonte".into()))?;
            let old = da.color.map(rgb_of).unwrap_or([0.0; 3]);
            let color = AnnotationColor::Rgb { red, green, blue };
            a.set_default_appearance(&da.font_name, size, Some(color))?;
            Edit::Font {
                page,
                id,
                size: da.size,
                color: old,
            }
        }
        Edit::Contents { text, .. } => {
            let old = a.contents()?.unwrap_or_default().to_owned();
            a.set_contents(&text)?;
            Edit::Contents {
                page,
                id,
                text: old,
            }
        }
    };
    a.update()?;
    page_ref.update()?;
    Ok(inverse)
}

fn set_style(
    a: &mut PdfAnnotation,
    color: ColorEdit,
    opacity: f32,
    width: Option<f32>,
) -> Result<()> {
    match color {
        ColorEdit::Keep => {}
        ColorEdit::Set(c) => a.set_color(c)?,
        ColorEdit::Clear => a.object().dict_delete("C")?,
    }
    a.set_opacity(opacity.clamp(0.0, 1.0))?;
    if let Some(w) = width {
        a.set_border_width(w.max(0.0))?;
    }
    Ok(())
}

/// Desloca o retângulo e os traços.
pub(crate) fn moved(
    rect: PdfRect,
    ink: Option<Strokes>,
    dx: f32,
    dy: f32,
) -> (PdfRect, Option<Strokes>) {
    let r = rect.translated(dx, dy);
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
