//! Edição de anotações direto no documento em memória, com desfazer/refazer próprio.
//!
//! Cada mudança é uma [`Edit`] reversível: aplicá-la devolve a inversa. Apagar (e desfazer uma
//! criação) só **esconde** a anotação (flag Hidden) — assim desfazer a traz de volta idêntica,
//! mesmo que venha de outro programa. Ao salvar, as escondidas por nós saem de verdade.
use crate::doc::{annot_id, rgb, rgb_of};
use crate::error::{PdfError, Result};
use crate::types::{PdfRect, Segment};
use mupdf::color::AnnotationColor;
use mupdf::pdf::{
    AnnotationFlags, PdfAnnotation, PdfAnnotationType, PdfDocument, PdfObject, PdfPage,
};
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
    Visibility { page: i32, id: i32, hidden: bool },
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
    /// Texto e data de modificação (`None` em `modified`: sem /M).
    Contents {
        page: i32,
        id: i32,
        text: String,
        modified: Option<String>,
    },
    /// Pontos da linha/seta (mover e puxar uma ponta).
    LinePoints {
        page: i32,
        id: i32,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// Vértices do polígono, das linhas conectadas e da nuvem (mover, puxar um vértice).
    Vertices {
        page: i32,
        id: i32,
        points: Vec<(f32, f32)>,
    },
    /// Texto explicativo: a caixa, a ponta da seta e o joelho (o ponto na caixa sai da caixa).
    Callout {
        page: i32,
        id: i32,
        rect: PdfRect,
        tip: (f32, f32),
        knee: (f32, f32),
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
    /// Grupo (/IRT com /RT /Group): `to` é o principal, `None` tira do grupo.
    Group { page: i32, id: i32, to: Option<i32> },
    /// Intensidade da borda de nuvem (/BE /I).
    Cloud { page: i32, id: i32, intensity: f32 },
    /// Várias mudanças da mesma anotação num passo só do desfazer (ex.: texto e tamanho da caixa).
    Batch(Vec<Edit>),
}

impl Edit {
    pub(crate) fn page(&self) -> i32 {
        match self {
            Self::Visibility { page, .. }
            | Self::Geometry { page, .. }
            | Self::Style { page, .. }
            | Self::Contents { page, .. }
            | Self::LinePoints { page, .. }
            | Self::Vertices { page, .. }
            | Self::Callout { page, .. }
            | Self::Fill { page, .. }
            | Self::Cloud { page, .. }
            | Self::Group { page, .. }
            | Self::Font { page, .. } => *page,
            Self::Batch(edits) => edits.first().map_or(0, Edit::page),
        }
    }
}

/// O documento inteiro guardado em memória (desfazer de operações de página) e as anotações
/// que estavam apagadas naquele momento.
pub(crate) struct Snapshot {
    pub(crate) bytes: Vec<u8>,
    pub(crate) hidden: HashSet<(i32, i32)>,
    /// Fontes acrescentadas pelo Editar texto (`PdfDoc::added_fonts`) no momento do instantâneo.
    pub(crate) added_fonts: Vec<i32>,
}

/// Um passo do desfazer/refazer: edição de anotação (com a inversa) ou instantâneo do documento.
pub(crate) enum Step {
    Annot(Edit),
    Snapshot(Snapshot),
    /// Valores de campos de formulário (o que volta ao desfazer/refazer): (página, id, valor), na
    /// ordem em que foram gravados; voltam na ordem inversa. Um preenchimento e os cálculos que ele
    /// provoca são um passo só.
    Fields(Vec<(i32, i32, String)>),
}

/// Memória máxima dos instantâneos; acima disso os passos mais antigos saem.
pub(crate) const SNAPSHOT_BUDGET: usize = 512 * 1024 * 1024;

/// Passos para desfazer e refazer e as anotações escondidas por nós.
#[derive(Default)]
pub(crate) struct History {
    pub(crate) undo: Vec<Step>,
    pub(crate) redo: Vec<Step>,
    /// (página, id) das apagadas e das criações desfeitas: saem do arquivo ao salvar.
    pub(crate) hidden: HashSet<(i32, i32)>,
}

impl History {
    /// Uma operação nova do usuário: guarda a inversa e esquece o que dava para refazer.
    pub(crate) fn record(&mut self, inverse: Edit) {
        self.undo.push(Step::Annot(inverse));
        self.redo.clear();
    }

    /// Um passo novo qualquer (ex.: valor de campo): esquece o que dava para refazer.
    pub(crate) fn record_step(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
    }

    /// Uma operação de página nova: guarda o documento de antes, dentro do orçamento de memória.
    /// Devolve se passos antigos saíram (o desfazer não alcança mais o arquivo salvo).
    pub(crate) fn record_snapshot(&mut self, before: Snapshot, budget: usize) -> bool {
        self.undo.push(Step::Snapshot(before));
        self.redo.clear();
        let size = |s: &Step| match s {
            Step::Snapshot(snap) => snap.bytes.len(),
            Step::Annot(_) | Step::Fields(_) => 0,
        };
        let mut dropped = false;
        while self.undo.iter().map(size).sum::<usize>() > budget {
            // o passo mais antigo que pesa sai, com tudo o que vem antes dele
            match self
                .undo
                .iter()
                .position(|s| matches!(s, Step::Snapshot(_)))
            {
                Some(oldest) if oldest + 1 < self.undo.len() => {
                    self.undo.drain(..=oldest);
                    dropped = true;
                }
                _ => break,
            }
        }
        dropped
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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

/// Comentário de que `a` é resposta ou status (/IRT), como o Acrobat: só com /RT ausente ou /R
/// (/RT /Group é uma anotação visível agrupada, não resposta).
pub(crate) fn reply_of(a: &PdfAnnotation) -> Result<Option<i32>> {
    let parent = match a.object().get_dict("IRT")? {
        Some(o) if o.is_indirect()? => o.as_indirect()?,
        _ => return Ok(None),
    };
    Ok(match name_entry(a, "RT")?.as_deref() {
        None | Some("R") => Some(parent),
        Some(_) => None,
    })
}

/// Texto de uma entrada do dicionário da anotação (`None` se não existe).
pub(crate) fn string_entry(a: &PdfAnnotation, key: &str) -> Result<Option<String>> {
    Ok(match a.object().get_dict(key)? {
        Some(o) => Some(o.as_string()?),
        None => None,
    })
}

/// Nome PDF de uma entrada do dicionário (`None` se não existe).
pub(crate) fn name_entry(a: &PdfAnnotation, key: &str) -> Result<Option<String>> {
    Ok(match a.object().get_dict(key)? {
        Some(o) if o.is_name()? => Some(String::from_utf8_lossy(&o.as_name()?).into_owned()),
        _ => None,
    })
}

/// Caixa de texto `id`: retângulo, texto, tamanho da fonte e borda; `None` se não é caixa de texto.
pub(crate) fn text_box_of(
    doc: &PdfDocument,
    page: i32,
    id: i32,
) -> Result<Option<(PdfRect, String, f32, f32)>> {
    let (_, a) = find(doc, page, id)?;
    if a.r#type()? != PdfAnnotationType::FreeText {
        return Ok(None);
    }
    let size = a.default_appearance()?.map_or(12.0, |da| da.size);
    let text = a.contents()?.unwrap_or_default().to_owned();
    Ok(Some((a.rect()?.into(), text, size, a.border_width()?)))
}

/// Texto solto do Preencher e assinar (`/IT /FreeTextTypeWriter`): a caixa acompanha o texto.
pub(crate) fn is_typewriter(doc: &PdfDocument, page: i32, id: i32) -> Result<bool> {
    let (_, a) = find(doc, page, id)?;
    Ok(name_entry(&a, "IT")?.as_deref() == Some("FreeTextTypeWriter"))
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

/// Teto de vértices lidos (polígono de outro programa com milhares de pontos).
pub const MAX_VERTICES: usize = 1000;

/// Vértices da anotação (vazio se não tiver), em coordenadas do MuPDF, sem os não finitos e com
/// teto: lidos direto do /Vertices, parando no teto (um array gigante não é percorrido inteiro).
/// O `bool` diz se o teto cortou vértices.
fn read_vertices(a: &PdfAnnotation, ctm: &mupdf::Matrix) -> Result<(Vec<(f32, f32)>, bool)> {
    if !matches!(
        a.r#type()?,
        PdfAnnotationType::Polygon | PdfAnnotationType::PolyLine
    ) {
        return Ok((Vec::new(), false));
    }
    let Some(raw) = a.object().get_dict("Vertices")? else {
        return Ok((Vec::new(), false));
    };
    let pairs = if raw.is_array()? { raw.len()? / 2 } else { 0 };
    let num = |k: usize| {
        raw.get_array(k as i32)
            .ok()
            .flatten()
            .and_then(|v| v.as_float().ok())
            .filter(|v| v.is_finite())
    };
    let mut out = Vec::new();
    for k in 0..pairs.min(MAX_VERTICES) {
        if let (Some(x), Some(y)) = (num(2 * k), num(2 * k + 1)) {
            let p = Point::new(x, y).transform(ctm);
            out.push((p.x, p.y));
        }
    }
    Ok((out, pairs > MAX_VERTICES))
}

/// Vértices para mostrar (até o teto).
pub(crate) fn vertices(a: &PdfAnnotation, ctm: &mupdf::Matrix) -> Result<Vec<(f32, f32)>> {
    Ok(read_vertices(a, ctm)?.0)
}

/// Vértices para editar: com mais que o teto a edição é recusada (regravar só os primeiros
/// apagaria o resto, sem volta).
fn editable_vertices(a: &PdfAnnotation, ctm: &mupdf::Matrix) -> Result<Vec<(f32, f32)>> {
    match read_vertices(a, ctm)? {
        (_, true) => Err(crate::error::refused(
            "polígono com mais de 1000 vértices: só dá para excluir ou mudar o estilo",
        )),
        (points, false) => Ok(points),
    }
}

/// Anotação agrupada a outra (/IRT com /RT /Group): o id da principal.
pub(crate) fn group_of(a: &PdfAnnotation) -> Result<Option<i32>> {
    if name_entry(a, "RT")?.as_deref() != Some("Group") {
        return Ok(None);
    }
    Ok(match a.object().get_dict("IRT")? {
        Some(o) if o.is_indirect()? => Some(o.as_indirect()?),
        _ => None,
    })
}

/// Geometria editável da anotação, lida numa busca só: decide como mover e redimensionar.
pub(crate) enum Shape {
    Callout {
        rect: PdfRect,
        tip: (f32, f32),
        knee: (f32, f32),
    },
    Vertices {
        rect: PdfRect,
        points: Vec<(f32, f32)>,
    },
    Line(Segment),
    /// Caixa, com os traços se for desenho.
    Box(PdfRect, Option<Strokes>),
}

pub(crate) fn shape_of(doc: &PdfDocument, page: i32, id: i32) -> Result<Shape> {
    let (page_ref, a) = find(doc, page, id)?;
    if let Some((rect, [tip, knee, _])) = callout(&a, &page_ref.ctm()?)? {
        return Ok(Shape::Callout { rect, tip, knee });
    }
    if a.r#type()? == PdfAnnotationType::Line {
        let (p, q) = a.line()?;
        return Ok(Shape::Line(((p.x, p.y), (q.x, q.y))));
    }
    let points = editable_vertices(&a, &page_ref.ctm()?)?;
    let (rect, ink) = geometry(&a)?;
    Ok(if points.is_empty() {
        Shape::Box(rect, ink)
    } else {
        Shape::Vertices { rect, points }
    })
}

/// Caixa do texto explicativo e a linha de chamada (ponta, joelho, ponto na caixa).
pub(crate) type CalloutGeometry = (PdfRect, [(f32, f32); 3]);

/// Distância de `p` ao segmento `a`–`b`.
pub fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Borracha do lápis: tira dos traços os pontos a menos de `radius` do caminho e corta o traço
/// onde algum trecho passou sob a borracha. Traços com menos de 2 pontos saem.
pub fn erase_strokes(
    strokes: &[Vec<(f32, f32)>],
    path: &[(f32, f32)],
    radius: f32,
) -> Vec<Vec<(f32, f32)>> {
    let near = |p: (f32, f32)| path.iter().any(|&e| (e.0 - p.0).hypot(e.1 - p.1) < radius);
    let crosses =
        |a: (f32, f32), b: (f32, f32)| path.iter().any(|&e| dist_to_segment(e, a, b) < radius);
    // apagar só tira pontos: o desenho nunca cresce, e todo traço longe da borracha fica
    let mut out = Vec::new();
    for stroke in strokes {
        let touched =
            stroke.iter().any(|&p| near(p)) || stroke.windows(2).any(|w| crosses(w[0], w[1]));
        if !touched {
            out.push(stroke.clone()); // fica como estava (até um ponto solto)
            continue;
        }
        let mut current: Vec<(f32, f32)> = Vec::new();
        for &p in stroke {
            let gone = near(p);
            if gone || current.last().is_some_and(|&q| crosses(q, p)) {
                if current.len() >= 2 {
                    out.push(std::mem::take(&mut current));
                }
                current.clear();
            }
            if !gone {
                current.push(p);
            }
        }
        if current.len() >= 2 {
            out.push(current);
        }
    }
    out
}

/// Texto explicativo (/IT /FreeTextCallout)?
pub(crate) fn is_callout(a: &PdfAnnotation) -> Result<bool> {
    Ok(name_entry(a, "IT")?.as_deref() == Some("FreeTextCallout"))
}

/// Caixa do texto explicativo (o /Rect sem as margens /RD, que abrem espaço para a linha) e a
/// linha de chamada (/CL: ponta, joelho, ponto na caixa), em coordenadas do MuPDF.
pub(crate) fn callout(a: &PdfAnnotation, ctm: &mupdf::Matrix) -> Result<Option<CalloutGeometry>> {
    if !is_callout(a)? {
        return Ok(None);
    }
    let num = |o: &mupdf::pdf::PdfObject, k: i32| {
        o.get_array(k)
            .ok()
            .flatten()
            .and_then(|v| v.as_float().ok())
            .filter(|v| v.is_finite())
    };
    let Some(cl) = a.object().get_dict("CL")? else {
        return Ok(None);
    };
    let pt = |k: i32| -> Option<(f32, f32)> {
        let p = Point::new(num(&cl, 2 * k)?, num(&cl, 2 * k + 1)?).transform(ctm);
        Some((p.x, p.y))
    };
    let line = match cl.len()? {
        6 => [pt(0), pt(1), pt(2)],
        // só ponta e fim: o joelho fica na ponta
        4 => [pt(0), pt(0), pt(1)],
        _ => return Ok(None),
    };
    let [Some(tip), Some(knee), Some(end)] = line else {
        return Ok(None);
    };
    // /Rect e /RD lidos juntos do dicionário, em pontos do PDF (o MuPDF regrava os dois ao gerar
    // a aparência; a leitura pela API pode vir de antes): a caixa é o /Rect menos as margens /RD
    // [esquerda base direita topo]
    let Some(r) = a.object().get_dict("Rect")? else {
        return Ok(None);
    };
    let [Some(x0), Some(y0), Some(x1), Some(y1)] = [0, 1, 2, 3].map(|k| num(&r, k)) else {
        return Ok(None);
    };
    let rd = match a.object().get_dict("RD")? {
        Some(rd) => [0, 1, 2, 3].map(|k| num(&rd, k).unwrap_or(0.0).max(0.0)),
        None => [0.0; 4],
    };
    let (bx0, by0) = (x0.min(x1) + rd[0], y0.min(y1) + rd[1]);
    let (bx1, by1) = (x0.max(x1) - rd[2], y0.max(y1) - rd[3]);
    let p = Point::new(bx0, by0).transform(ctm);
    let q = Point::new(bx1.max(bx0), by1.max(by0)).transform(ctm);
    let rect = PdfRect::new(p.x.min(q.x), p.y.min(q.y), p.x.max(q.x), p.y.max(q.y));
    Ok(Some((rect, [tip, knee, end])))
}

/// Ponto da borda da caixa mais perto do joelho (onde a linha de chamada encosta).
pub fn callout_end(rect: PdfRect, knee: (f32, f32)) -> (f32, f32) {
    let x = knee.0.clamp(rect.x0, rect.x1);
    let y = knee.1.clamp(rect.y0, rect.y1);
    let inside = x == knee.0 && y == knee.1;
    if !inside {
        return (x, y);
    }
    // joelho dentro da caixa: a borda mais próxima
    let d = [
        knee.0 - rect.x0,
        rect.x1 - knee.0,
        knee.1 - rect.y0,
        rect.y1 - knee.1,
    ];
    let min = d.iter().copied().fold(f32::MAX, f32::min);
    match d.iter().position(|&v| v == min) {
        Some(0) => (rect.x0, knee.1),
        Some(1) => (rect.x1, knee.1),
        Some(2) => (knee.0, rect.y0),
        _ => (knee.0, rect.y1),
    }
}

/// Grava a caixa e a linha de chamada (o MuPDF refaz /Rect e /RD ao gerar a aparência).
pub(crate) fn set_callout(
    doc: &PdfDocument,
    a: &mut PdfAnnotation,
    ctm: &mupdf::Matrix,
    rect: PdfRect,
    tip: (f32, f32),
    knee: (f32, f32),
) -> Result<()> {
    let inv = ctm.invert().unwrap_or(mupdf::Matrix::IDENTITY);
    let end = callout_end(rect, knee);
    let mut cl = doc.new_array()?;
    for (x, y) in [tip, knee, end] {
        let p = Point::new(x, y).transform(&inv);
        cl.array_push(PdfObject::new_real(p.x)?)?;
        cl.array_push(PdfObject::new_real(p.y)?)?;
    }
    // sem /RD velho: a caixa nova é o /Rect inteiro até o MuPDF recalcular
    clear_key(a, "RD")?;
    a.set_rect(Rect::from(rect))?;
    let mut obj = a.object();
    obj.dict_put("CL", cl)?;
    obj.dict_put("IT", PdfObject::new_name("FreeTextCallout")?)?;
    obj.dict_put("LE", PdfObject::new_name("OpenArrow")?)?;
    Ok(())
}

/// Borda de nuvem (/BE /S /C): a nuvem do Acrobat.
pub(crate) fn is_cloudy(a: &PdfAnnotation) -> Result<bool> {
    Ok(match a.object().get_dict("BE")? {
        Some(be) => be
            .get_dict("S")?
            .is_some_and(|s| s.as_name().is_ok_and(|n| n == b"C")),
        None => false,
    })
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
    if let Edit::Batch(edits) = edit {
        // aplica em ordem; se uma falhar, desfaz as já aplicadas (nada fica pela metade)
        let mut inverses = Vec::new();
        for e in edits {
            match apply_edit(doc, hidden, e) {
                Ok(inv) => inverses.push(inv),
                Err(err) => {
                    for inv in inverses.into_iter().rev() {
                        let _ = apply_edit(doc, hidden, inv);
                    }
                    return Err(err);
                }
            }
        }
        inverses.reverse();
        return Ok(Edit::Batch(inverses));
    }
    let (page, id) = match &edit {
        Edit::Batch(_) => unreachable!("tratado acima"),
        Edit::Visibility { page, id, .. }
        | Edit::Geometry { page, id, .. }
        | Edit::Style { page, id, .. }
        | Edit::Contents { page, id, .. }
        | Edit::LinePoints { page, id, .. }
        | Edit::Vertices { page, id, .. }
        | Edit::Callout { page, id, .. }
        | Edit::Fill { page, id, .. }
        | Edit::Cloud { page, id, .. }
        | Edit::Group { page, id, .. }
        | Edit::Font { page, id, .. } => (*page, *id),
    };
    let (mut page_ref, mut a) = find(doc, page, id)?;
    let inverse = match edit {
        Edit::Batch(_) => unreachable!("tratado acima"),
        Edit::Visibility { hidden: hide, .. } => {
            // respostas e status já são invisíveis na página (NoView ou Hidden, como o Acrobat
            // grava): apagar só os tira da lista; mexer na flag os faria aparecer ao desfazer
            if reply_of(&a)?.is_none() {
                let mut flags = a.flags()?;
                flags.set(AnnotationFlags::IS_HIDDEN, hide);
                a.set_flags(flags)?;
            }
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
        Edit::Callout {
            rect, tip, knee, ..
        } => {
            let ctm = page_ref.ctm()?;
            let (old_rect, [old_tip, old_knee, _]) = callout(&a, &ctm)?
                .ok_or_else(|| PdfError::InvalidAnnotation("não é um texto explicativo".into()))?;
            set_callout(doc, &mut a, &ctm, rect, tip, knee)?;
            Edit::Callout {
                page,
                id,
                rect: old_rect,
                tip: old_tip,
                knee: old_knee,
            }
        }
        Edit::Vertices { points, .. } => {
            let old = editable_vertices(&a, &page_ref.ctm()?)?;
            a.set_vertices(points.iter().map(|&(x, y)| Point::new(x, y)))?;
            Edit::Vertices {
                page,
                id,
                points: old,
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
                ColorEdit::Clear => clear_key(&mut a, key)?,
            }
            Edit::Fill {
                page,
                id,
                color: old,
            }
        }
        Edit::Group { to, .. } => {
            let old = group_of(&a)?;
            let mut obj = a.object();
            match to {
                Some(root) => {
                    let (_, principal) = find(doc, page, root)?;
                    obj.dict_put("IRT", principal.object())?;
                    obj.dict_put("RT", PdfObject::new_name("Group")?)?;
                }
                None => {
                    obj.dict_delete("IRT")?;
                    obj.dict_delete("RT")?;
                }
            }
            Edit::Group { page, id, to: old }
        }
        Edit::Cloud { intensity, .. } => {
            if !is_cloudy(&a)? {
                return Err(PdfError::InvalidAnnotation("não é uma nuvem".into()));
            }
            let old = a.border_effect_intensity()?;
            a.set_border_effect_intensity(intensity)?;
            Edit::Cloud {
                page,
                id,
                intensity: old,
            }
        }
        Edit::Font { size, color, .. } => {
            let da = a
                .default_appearance()?
                .ok_or_else(|| PdfError::InvalidAnnotation("anotação sem fonte".into()))?;
            let old = da.color.map(rgb_of).unwrap_or([0.0; 3]);
            a.set_default_appearance(&da.font_name, size, Some(rgb(color)))?;
            Edit::Font {
                page,
                id,
                size: da.size,
                color: old,
            }
        }
        Edit::Contents { text, modified, .. } => {
            let old = a.contents()?.unwrap_or_default().to_owned();
            let old_modified = string_entry(&a, "M")?;
            a.set_contents(&text)?;
            match &modified {
                Some(m) => a.object().dict_put("M", PdfObject::new_string(m)?)?,
                None => a.object().dict_delete("M")?,
            }
            Edit::Contents {
                page,
                id,
                text: old,
                modified: old_modified,
            }
        }
    };
    a.update()?;
    page_ref.update()?;
    Ok(inverse)
}

/// Tira uma chave do dicionário da anotação e pede ao MuPDF que refaça a aparência (mexer no
/// dicionário direto não avisa o MuPDF; reaplicar a opacidade marca a anotação como alterada).
fn clear_key(a: &mut PdfAnnotation, key: &str) -> Result<()> {
    a.object().dict_delete(key)?;
    let opacity = a.opacity()?;
    a.set_opacity(opacity)?;
    Ok(())
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
        ColorEdit::Clear => clear_key(a, "C")?,
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

#[cfg(test)]
mod tests {
    #[test]
    fn strokes_the_eraser_does_not_touch_stay_as_they_are() {
        let dot = vec![(500.0, 500.0)];
        let far = vec![(300.0, 0.0), (400.0, 0.0)];
        let hit = vec![(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)];
        let out = super::erase_strokes(&[dot.clone(), far.clone(), hit], &[(10.0, 0.0)], 3.0);
        assert!(
            out.contains(&dot),
            "o ponto solto longe da borracha fica: {out:?}"
        );
        assert!(out.contains(&far));
        assert_eq!(
            out.len(),
            2,
            "o traço tocado perdeu o ponto do meio e ficou sem pares"
        );
    }

    fn line() -> Vec<(f32, f32)> {
        (0..=10).map(|k| (k as f32 * 10.0, 0.0)).collect()
    }

    #[test]
    fn erasing_the_middle_splits_the_stroke() {
        let out = erase_strokes(&[line()], &[(50.0, 0.0)], 8.0);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].last(), Some(&(40.0, 0.0)));
        assert_eq!(out[1].first(), Some(&(60.0, 0.0)));
    }

    #[test]
    fn erasing_everything_leaves_nothing_and_far_away_changes_nothing() {
        let all: Vec<(f32, f32)> = (0..=10).map(|k| (k as f32 * 10.0, 0.0)).collect();
        assert!(erase_strokes(&[line()], &all, 6.0).is_empty());
        assert_eq!(erase_strokes(&[line()], &[(50.0, 80.0)], 8.0), vec![line()]);
    }

    #[test]
    fn erasing_between_sparse_points_cuts_the_segment() {
        let sparse = vec![(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)];
        let out = erase_strokes(&[sparse], &[(50.0, 2.0)], 8.0);
        assert_eq!(
            out,
            vec![vec![(100.0, 0.0), (200.0, 0.0)]],
            "o trecho cortado sai"
        );
    }
    use super::*;

    fn snap(size: usize) -> Snapshot {
        Snapshot {
            bytes: vec![0; size],
            hidden: HashSet::new(),
            added_fonts: Vec::new(),
        }
    }

    #[test]
    fn snapshots_respect_the_memory_limit() {
        let mut h = History::default();
        h.record(Edit::Visibility {
            page: 0,
            id: 1,
            hidden: false,
        });
        assert!(!h.record_snapshot(snap(60), 100));
        assert!(
            h.record_snapshot(snap(60), 100),
            "avisa que o salvo ficou fora de alcance"
        );
        assert_eq!(
            h.undo.len(),
            1,
            "o passo mais antigo (e o que vinha antes) saiu"
        );
        assert!(h.record_snapshot(snap(500), 100));
        assert_eq!(h.undo.len(), 1, "o mais novo fica mesmo acima do limite");
    }
}
