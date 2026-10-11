//! Resumir comentários, como o Acrobat: um PDF novo com os comentários (e as respostas) listados,
//! em 4 layouts. A página do documento entra como Form XObject, com as anotações desenhadas por
//! cima a partir das aparências delas, e as linhas de conexão saem de cada comentário até o item.
use crate::compare_report::{content_page, file_name, report_fonts, text_at};
use crate::doc::{write_atomic, PdfDoc};
use crate::error::{refused, Result};
use crate::page_content::{form_xobject, matrix_ops, mul, num, page_parts};
use crate::pdf_obj::{get, name_of};
use crate::text_fit::text_width;
use crate::types::AnnotInfo;
use mupdf::pdf::{Encryption, PdfDocument, PdfGraftMap, PdfObject, PdfWriteOptions};
use mupdf::{Font, Matrix, Size};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Layout do resumo (as 4 opções do Acrobat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryLayout {
    /// Documento e comentários com linhas de conexão em páginas separadas.
    ConnectorSeparate,
    /// Somente comentários.
    CommentsOnly,
    /// Documento e comentários com linhas de conexão em páginas únicas.
    ConnectorSingle,
    /// Documento e comentários com números de sequência em páginas separadas.
    NumbersSeparate,
}

impl SummaryLayout {
    pub const ALL: [Self; 4] = [
        Self::ConnectorSeparate,
        Self::CommentsOnly,
        Self::ConnectorSingle,
        Self::NumbersSeparate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ConnectorSeparate => {
                "Documento e comentários com linhas de conexão em páginas separadas"
            }
            Self::CommentsOnly => "Somente comentários",
            Self::ConnectorSingle => {
                "Documento e comentários com linhas de conexão em páginas únicas"
            }
            Self::NumbersSeparate => {
                "Documento e comentários com números de sequência em páginas separadas"
            }
        }
    }
}

/// Ordem dos comentários.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarySort {
    Page,
    Author,
    Date,
    Type,
}

impl SummarySort {
    pub const ALL: [Self; 4] = [Self::Page, Self::Author, Self::Date, Self::Type];

    pub fn label(self) -> &'static str {
        match self {
            Self::Page => "Página",
            Self::Author => "Autor",
            Self::Date => "Data",
            Self::Type => "Tipo",
        }
    }
}

/// Tamanho da fonte do resumo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryFont {
    Small,
    Medium,
    Large,
}

impl SummaryFont {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Pequeno",
            Self::Medium => "Médio",
            Self::Large => "Grande",
        }
    }

    fn size(self) -> f32 {
        match self {
            Self::Small => 8.0,
            Self::Medium => 10.0,
            Self::Large => 12.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryOptions {
    pub layout: SummaryLayout,
    pub sort: SummarySort,
    /// Só estes comentários (os que o filtro do painel mostra); `None`: todos.
    pub only: Option<Vec<(i32, i32)>>,
    /// Intervalo de páginas (0-based, inclusivo); `None`: todas.
    pub pages: Option<(i32, i32)>,
    pub font: SummaryFont,
    pub include_empty: bool,
}

/// Página do resumo só com comentários: A4 em pé.
const A4: (f32, f32) = (595.0, 842.0);
const MARGIN: f32 = 36.0;
/// Tetos: comentários, páginas do resultado e caracteres do texto de cada comentário.
const MAX_ITEMS: usize = 5000;
const MAX_PAGES: usize = 2000;
const MAX_CHARS: usize = 2000;
/// Cor das linhas de conexão e dos números quando o comentário não tem cor.
const LINK_BLUE: [f32; 3] = [0.2, 0.4, 0.8];

/// Um comentário do resumo, com as respostas e o número de sequência.
struct Item {
    root: AnnotInfo,
    replies: Vec<AnnotInfo>,
    number: usize,
}

/// Linha já quebrada: negrito, recuo e texto.
struct Line {
    bold: bool,
    indent: f32,
    text: String,
}

/// O PDF do resumo em construção.
struct Out {
    doc: PdfDocument,
    helv: PdfObject,
    bold: PdfObject,
    metrics: Font,
    size: f32,
    pages: usize,
}

impl Out {
    fn new(size: f32) -> Result<Self> {
        let mut doc = PdfDocument::new();
        let (helv, bold) = report_fonts(&mut doc)?;
        Ok(Self {
            doc,
            helv,
            bold,
            metrics: Font::new("Helvetica")?,
            size,
            pages: 0,
        })
    }

    fn page(&mut self, (w, h): (f32, f32), ops: &str, xobjects: Option<PdfObject>) -> Result<()> {
        self.pages += 1;
        if self.pages > MAX_PAGES {
            return Err(refused("o resumo passaria de 2000 páginas"));
        }
        let mut fonts = self.doc.new_dict()?;
        fonts.dict_put("F1", self.helv.clone())?;
        fonts.dict_put("F2", self.bold.clone())?;
        let mut res = self.doc.new_dict()?;
        res.dict_put("Font", fonts)?;
        if let Some(x) = xobjects {
            res.dict_put("XObject", x)?;
        }
        content_page(
            &mut self.doc,
            Size {
                width: w,
                height: h,
            },
            res,
            ops,
        )?;
        Ok(())
    }

    /// Quebra `text` em linhas de até `width` pontos (palavra longa demais é cortada). Mede só a
    /// palavra nova e soma à largura da linha.
    fn wrap(&self, text: &str, width: f32) -> Result<Vec<String>> {
        let measure = |s: &str| text_width(&self.metrics, s, self.size);
        let space = measure(" ")?;
        let mut out = Vec::new();
        for para in text.split('\n') {
            let (mut line, mut used) = (String::new(), 0.0);
            for word in para.split_whitespace() {
                let w = measure(word)?;
                let need = if line.is_empty() { w } else { used + space + w };
                if need <= width {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(word);
                    used = need;
                    continue;
                }
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    used = 0.0;
                }
                if w <= width {
                    line.push_str(word);
                    used = w;
                    continue;
                }
                // a palavra sozinha não cabe: corta por caracteres
                for c in word.chars() {
                    let cw = measure(c.encode_utf8(&mut [0; 4]))?;
                    if used + cw > width && !line.is_empty() {
                        out.push(std::mem::take(&mut line));
                        used = 0.0;
                    }
                    line.push(c);
                    used += cw;
                }
            }
            out.push(line);
        }
        Ok(out)
    }

    /// As linhas de um item: cabeçalho (número, tipo, autor, data), texto e respostas.
    fn lines(&self, item: &Item, numbered: bool, width: f32) -> Result<Vec<Line>> {
        let head = |a: &AnnotInfo| {
            let author = if a.author.trim().is_empty() {
                "Sem autor"
            } else {
                a.author.trim()
            };
            let date = short_date(if a.modified.is_empty() {
                &a.created
            } else {
                &a.modified
            });
            (author.to_string(), date)
        };
        let (author, date) = head(&item.root);
        let mut header = format!("{} — {author}", item.root.kind.label());
        if !date.is_empty() {
            header.push_str(&format!(" — {date}"));
        }
        if numbered {
            header = format!("{}. {header}", item.number);
        } else {
            header = format!("Página {}: {header}", item.root.page + 1);
        }
        let mut out = Vec::new();
        self.push_wrapped(&mut out, &header, true, 0.0, width)?;
        self.push_wrapped(&mut out, &capped(&item.root.contents), false, 0.0, width)?;
        let indent = self.size * 1.5;
        for r in &item.replies {
            let (author, date) = head(r);
            let label = match date.is_empty() {
                true => format!("Resposta de {author}: {}", capped(&r.contents)),
                false => format!("Resposta de {author} ({date}): {}", capped(&r.contents)),
            };
            self.push_wrapped(&mut out, &label, false, indent, width)?;
        }
        Ok(out)
    }

    /// Acrescenta `text` quebrado na largura que sobra depois do recuo.
    fn push_wrapped(
        &self,
        out: &mut Vec<Line>,
        text: &str,
        bold: bool,
        indent: f32,
        width: f32,
    ) -> Result<()> {
        for text in self.wrap(text, width - indent)? {
            out.push(Line { bold, indent, text });
        }
        Ok(())
    }

    /// Distribui os blocos em colunas, uma por página: a primeira em `first` (x, topo), as de
    /// continuação em `rest`. Devolve, por página, as operações e onde cada bloco começa (a altura
    /// da linha de conexão).
    fn flow(
        &self,
        blocks: &[Vec<Line>],
        first: (f32, f32),
        rest: (f32, f32),
    ) -> Vec<(String, Vec<f32>)> {
        let mut pages = Vec::new();
        let (mut ops, mut starts) = (String::new(), Vec::new());
        let (mut x, mut y) = first;
        for lines in blocks {
            if y - self.height(lines) < MARGIN && !starts.is_empty() {
                pages.push((std::mem::take(&mut ops), std::mem::take(&mut starts)));
                (x, y) = rest;
            }
            starts.push(y - self.size * 0.6);
            let (more, end) = self.draw(lines, x, y);
            ops.push_str(&more);
            y = end - self.size * 0.9;
        }
        pages.push((ops, starts));
        pages
    }

    /// Desenha as linhas a partir de `y` (topo, coordenadas do PDF).
    fn draw(&self, lines: &[Line], x: f32, mut y: f32) -> (String, f32) {
        let mut ops = String::new();
        for l in lines {
            y -= self.size * 1.25;
            let font = if l.bold { "F2" } else { "F1" };
            ops.push_str(&text_at(font, self.size, 0.0, x + l.indent, y, &l.text));
        }
        (ops, y)
    }

    fn height(&self, lines: &[Line]) -> f32 {
        lines.len() as f32 * self.size * 1.25 + self.size * 0.9
    }
}

fn capped(s: &str) -> String {
    match s.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => format!("{}…", &s[..cut]),
        None => s.to_string(),
    }
}

/// Ordena os comentários pelo critério (a página e a posição desempatam).
fn sort_items(items: &mut [Item], sort: SummarySort) {
    let place = |a: &AnnotInfo| (a.page, a.rect.y0 as i64, a.rect.x0 as i64);
    match sort {
        SummarySort::Page => items.sort_by_key(|i| place(&i.root)),
        SummarySort::Author => {
            items.sort_by_key(|i| (i.root.author.to_lowercase(), place(&i.root)))
        }
        SummarySort::Date => items.sort_by(|a, b| {
            date_key(&a.root)
                .cmp(date_key(&b.root))
                .then(place(&a.root).cmp(&place(&b.root)))
        }),
        SummarySort::Type => items.sort_by_key(|i| (i.root.kind.label(), place(&i.root))),
    }
}

fn date_key(a: &AnnotInfo) -> &str {
    if a.created.is_empty() {
        &a.modified
    } else {
        &a.created
    }
}

/// Grava em `dest` o resumo dos comentários de `doc`. Devolve quantos comentários entraram.
pub fn summarize_comments(doc: &PdfDoc, opts: &SummaryOptions, dest: &Path) -> Result<usize> {
    let count = doc.page_count()?;
    let (first, last) = match opts.pages {
        Some((a, b)) => (a.max(0), b.min(count - 1)),
        None => (0, count - 1),
    };
    if first > last {
        return Err(refused("intervalo de páginas inválido"));
    }
    // comentários por página, cada um com as respostas
    let only: Option<HashSet<(i32, i32)>> = opts.only.as_ref().map(|o| o.iter().copied().collect());
    let mut pages: Vec<(i32, Vec<Item>)> = Vec::new();
    let mut total = 0;
    for p in first..=last {
        let annots = doc.annotations(p)?;
        let mut replies: HashMap<i32, Vec<AnnotInfo>> = HashMap::new();
        for r in annots.iter().filter(|r| r.state.is_none()) {
            if let Some(parent) = r.reply_to {
                replies.entry(parent).or_default().push(r.clone());
            }
        }
        let mut items: Vec<Item> = annots
            .iter()
            .filter(|a| !a.is_reply())
            .filter(|a| only.as_ref().is_none_or(|o| o.contains(&(p, a.id))))
            .take(MAX_ITEMS - total)
            .map(|a| {
                let mut replies = replies.remove(&a.id).unwrap_or_default();
                replies.sort_by(|x, y| date_key(x).cmp(date_key(y)));
                Item {
                    root: a.clone(),
                    replies,
                    number: 0,
                }
            })
            .collect();
        sort_items(&mut items, opts.sort);
        total += items.len();
        pages.push((p, items));
    }
    if total == 0 {
        return Err(refused("não há comentários para resumir"));
    }
    let mut n = 0;
    for (_, items) in &mut pages {
        for i in items.iter_mut() {
            n += 1;
            i.number = n;
        }
    }
    let title = format!("Resumo de comentários em {}", file_name(&doc.path));
    let mut out = Out::new(opts.font.size())?;
    match opts.layout {
        SummaryLayout::CommentsOnly => comments_only(&mut out, pages, opts, &title)?,
        layout => {
            let mut map = out.doc.new_graft_map()?;
            for (p, items) in pages {
                if items.is_empty() && !opts.include_empty {
                    continue;
                }
                with_document(&mut out, &mut map, doc, p, &items, layout)?;
            }
        }
    }
    let mut info = out.doc.new_dict()?;
    info.dict_put("Title", PdfObject::new_string(&title)?)?;
    info.dict_put("Creator", PdfObject::new_string("Gera PDF Editor")?)?;
    let info = out.doc.add_object(&info)?;
    out.doc.trailer()?.dict_put("Info", info)?;
    let mut w = PdfWriteOptions::default();
    w.set_encryption(Encryption::None).set_compress(true);
    write_atomic(&out.doc, dest, w)?;
    Ok(total)
}

/// Layout "Somente comentários": os itens em sequência em páginas A4, com o título na primeira.
fn comments_only(
    out: &mut Out,
    pages: Vec<(i32, Vec<Item>)>,
    opts: &SummaryOptions,
    title: &str,
) -> Result<()> {
    let width = A4.0 - 2.0 * MARGIN;
    let by_page = opts.sort == SummarySort::Page;
    let mut blocks: Vec<Vec<Line>> = Vec::new();
    if by_page {
        for (p, items) in &pages {
            if items.is_empty() && !opts.include_empty {
                continue;
            }
            let mut heading = Vec::new();
            out.push_wrapped(&mut heading, &format!("Página {}", p + 1), true, 0.0, width)?;
            if items.is_empty() {
                out.push_wrapped(&mut heading, "Sem comentários.", false, 0.0, width)?;
            }
            blocks.push(heading);
            for i in items {
                blocks.push(out.lines(i, true, width)?);
            }
        }
    } else {
        let mut all: Vec<Item> = pages.into_iter().flat_map(|(_, items)| items).collect();
        sort_items(&mut all, opts.sort);
        for i in &all {
            blocks.push(out.lines(i, false, width)?);
        }
    }
    let title = text_at("F2", 16.0, 0.0, MARGIN, A4.1 - MARGIN - 16.0, title);
    let pages = out.flow(
        &blocks,
        (MARGIN, A4.1 - MARGIN - 34.0),
        (MARGIN, A4.1 - MARGIN),
    );
    for (k, (ops, _)) in pages.into_iter().enumerate() {
        let ops = if k == 0 { format!("{title}{ops}") } else { ops };
        out.page(A4, &ops, None)?;
    }
    Ok(())
}

/// Linha reta entre dois pontos.
fn line_ops(color: [f32; 3], a: (f32, f32), b: (f32, f32)) -> String {
    format!(
        "q {} {} {} RG 0.75 w {} {} m {} {} l S Q\n",
        num(color[0]),
        num(color[1]),
        num(color[2]),
        num(a.0),
        num(a.1),
        num(b.0),
        num(b.1)
    )
}

/// Círculo cheio com o número branco no meio (layout de números de sequência).
fn badge(out: &Out, color: [f32; 3], (x, y): (f32, f32), n: usize) -> Result<String> {
    let r = 7.0;
    // a moldura arredondada do carimbo, com raio igual à metade do lado, é um círculo
    let mut ops = format!(
        "q {} {} {} rg {} f Q\n",
        num(color[0]),
        num(color[1]),
        num(color[2]),
        crate::stamp::rounded_rect(x - r, y - r, x + r, y + r, r)
    );
    let label = n.to_string();
    let size = if label.len() > 2 { 5.0 } else { 7.0 };
    let w = text_width(&out.metrics, &label, size)?;
    ops.push_str(&format!(
        "q BT /F2 {} Tf 1 g {} {} Td ({label}) Tj ET Q\n",
        num(size),
        num(x - w / 2.0),
        num(y - size * 0.35)
    ));
    Ok(ops)
}

/// A página do documento posta no alto à esquerda do resultado: as operações e os XObjects.
struct Placed {
    ops: String,
    xobjects: PdfObject,
    /// Origem da tela da página de origem e altura da página do resultado.
    origin: (f32, f32),
    height: f32,
}

impl Placed {
    /// Ponto da tela da página de origem no resultado (origem embaixo).
    fn to_out(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x - self.origin.0, self.height - (y - self.origin.1))
    }
}

/// A página do documento como desenho: o conteúdo e as anotações (das aparências delas), no
/// canto superior esquerdo de uma página do resultado de altura `height`.
fn place_page(
    out: &mut Out,
    map: &mut PdfGraftMap,
    doc: &PdfDoc,
    page: i32,
    bounds: mupdf::Rect,
    height: f32,
) -> Result<Placed> {
    let (ox, oy) = (0.0, 0.0);
    let (bx, by) = (bounds.x0, bounds.y0);
    let (w, h) = (bounds.width(), bounds.height());
    let src = doc.inner.find_page(page)?;
    let (content, res, bbox) = page_parts(map, &out.doc, &src)?;
    let mut xobjects = out.doc.new_dict()?;
    xobjects.dict_put("P", form_xobject(&mut out.doc, &content, res, bbox)?)?;
    // espaço do PDF da página de origem → tela dela → resultado (origem embaixo)
    let to_out = Matrix::new(1.0, 0.0, 0.0, -1.0, ox - bx, height - oy + by);
    let m = mul(&src.page_ctm()?, &to_out);
    let mut ops = format!(
        "q {} {} {} {} re W n {} cm /P Do\n",
        num(ox),
        num(height - oy - h),
        num(w),
        num(h),
        matrix_ops(&m)
    );
    // as anotações por cima, como estão na tela (respostas e escondidas não aparecem)
    let pdf_page = doc.inner.load_pdf_page(page)?;
    let shown: HashSet<i32> = doc
        .annotations(page)?
        .iter()
        .filter(|a| !a.is_reply())
        .map(|a| a.id)
        .collect();
    for (k, annot) in pdf_page.annotations().enumerate() {
        let id = annot.object().as_indirect()?;
        if !shown.contains(&id) {
            continue;
        }
        let Some(dict) = annot.object().resolve()? else {
            continue;
        };
        let (Some(ap), Some(rect)) = (appearance(&dict), get(&dict, "Rect")) else {
            continue;
        };
        let Some(place) = fit_matrix(&ap, &rect) else {
            continue;
        };
        let name = format!("A{k}");
        xobjects.dict_put(name.as_str(), map.graft_object(&ap)?)?;
        ops.push_str(&format!(
            "q {} cm /{name} Do Q\n",
            matrix_ops(&mul(&place, &m))
        ));
    }
    ops.push_str("Q\n");
    Ok(Placed {
        ops,
        xobjects,
        origin: (bx, by),
        height,
    })
}

/// Aparência normal da anotação (/AP /N; com estados, o de /AS).
fn appearance(dict: &PdfObject) -> Option<PdfObject> {
    let n = get(dict, "AP").and_then(|ap| get(&ap, "N"))?;
    if n.is_stream().unwrap_or(false) {
        return Some(n);
    }
    let state = get(dict, "AS").and_then(|s| name_of(&s))?;
    get(&n, &state).filter(|s| s.is_stream().unwrap_or(false))
}

fn floats4(o: &PdfObject) -> Option<[f32; 4]> {
    let v = |k| {
        o.get_array(k)
            .ok()
            .flatten()?
            .as_float()
            .ok()
            .filter(|f| f.is_finite())
    };
    Some([v(0)?, v(1)?, v(2)?, v(3)?])
}

/// A matriz que leva a aparência ao /Rect da anotação (regra do PDF: a BBox transformada pela
/// /Matrix do formulário encaixa no retângulo).
fn fit_matrix(ap: &PdfObject, rect: &PdfObject) -> Option<Matrix> {
    let [rx0, ry0, rx1, ry1] = floats4(rect)?;
    let [bx0, by0, bx1, by1] = get(ap, "BBox").and_then(|b| floats4(&b))?;
    let fm = get(ap, "Matrix")
        .and_then(|m| {
            let v = |k| m.get_array(k).ok().flatten()?.as_float().ok();
            Some(Matrix::new(v(0)?, v(1)?, v(2)?, v(3)?, v(4)?, v(5)?))
        })
        .unwrap_or(Matrix::IDENTITY);
    let pts = [(bx0, by0), (bx1, by0), (bx0, by1), (bx1, by1)]
        .map(|(x, y)| (x * fm.a + y * fm.c + fm.e, x * fm.b + y * fm.d + fm.f));
    let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min);
    let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
    let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let (tw, th) = (max_x - min_x, max_y - min_y);
    if tw <= 0.0 || th <= 0.0 {
        return None;
    }
    let (sx, sy) = ((rx1 - rx0).abs() / tw, (ry1 - ry0).abs() / th);
    let (x0, y0) = (rx0.min(rx1), ry0.min(ry1));
    Some(Matrix::new(
        sx,
        0.0,
        0.0,
        sy,
        x0 - min_x * sx,
        y0 - min_y * sy,
    ))
}

/// Ponto do comentário de onde sai a linha (canto superior esquerdo, na tela da origem).
fn anchor(a: &AnnotInfo) -> (f32, f32) {
    (a.rect.x0, a.rect.y0)
}

fn color_of(a: &AnnotInfo) -> [f32; 3] {
    line_color(a.color.unwrap_or(LINK_BLUE))
}

/// A cor do comentário para linhas e números: as claras (o amarelo do destaque) escurecem para
/// aparecer no branco.
fn line_color(c: [f32; 3]) -> [f32; 3] {
    let luma = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    if luma > 0.6 {
        c.map(|v| v * 0.6 / luma)
    } else {
        c
    }
}

/// Data do PDF como "11/10/2026 15:14" (vazio se não der para ler).
fn short_date(pdf: &str) -> String {
    let d = pdf.trim().trim_start_matches("D:").as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        d.get(r)
            .filter(|s| s.iter().all(u8::is_ascii_digit))
            .map(|s| String::from_utf8_lossy(s).into_owned())
    };
    match (
        digits(0..4),
        digits(4..6),
        digits(6..8),
        digits(8..10),
        digits(10..12),
    ) {
        (Some(y), Some(m), Some(day), Some(h), Some(min)) => format!("{day}/{m}/{y} {h}:{min}"),
        _ => String::new(),
    }
}

/// Layouts com o documento: a página (com as anotações) e os comentários dela.
fn with_document(
    out: &mut Out,
    map: &mut PdfGraftMap,
    doc: &PdfDoc,
    page: i32,
    items: &[Item],
    layout: SummaryLayout,
) -> Result<()> {
    let bounds = doc.inner.load_page(page)?.bounds()?;
    let (w, h) = (bounds.width(), bounds.height());
    let single = layout == SummaryLayout::ConnectorSingle;
    let numbered = layout == SummaryLayout::NumbersSeparate;
    // a coluna dos comentários: à direita na mesma página, ou numa página do mesmo tamanho
    let col_x = if single { w + MARGIN } else { MARGIN };
    let col_w = (w - 2.0 * MARGIN).max(60.0);
    let blocks: Vec<Vec<Line>> = items
        .iter()
        .map(|i| out.lines(i, true, col_w))
        .collect::<Result<_>>()?;
    // a primeira coluna (com as linhas de conexão) e as de continuação
    let mut columns = out
        .flow(&blocks, (col_x, h - MARGIN), (MARGIN, h - MARGIN))
        .into_iter();
    let (mut column, starts) = columns.next().unwrap_or_default();
    let page_size = if single { (2.0 * w, h) } else { (w, h) };
    let mut placed = place_page(out, map, doc, page, bounds, page_size.1)?;
    let mut ops = std::mem::take(&mut placed.ops);
    for (item, start) in items.iter().zip(starts) {
        let color = color_of(&item.root);
        let a = placed.to_out(anchor(&item.root));
        if numbered {
            ops.push_str(&badge(out, color, a, item.number)?);
        } else if single {
            ops.push_str(&line_ops(color, a, (col_x - 4.0, start)));
        } else {
            // em páginas separadas: até a borda na página do documento e da borda até o item
            ops.push_str(&line_ops(color, a, (w, start)));
            column.push_str(&line_ops(color, (0.0, start), (col_x - 4.0, start)));
        }
    }
    if single {
        if items.is_empty() {
            let at = h - MARGIN - out.size;
            ops.push_str(&text_at("F1", out.size, 0.4, col_x, at, "Sem comentários."));
        }
        ops.push_str(&column);
    }
    out.page(page_size, &ops, Some(placed.xobjects))?;
    if !single && !items.is_empty() {
        out.page((w, h), &column, None)?;
    }
    for (more, _) in columns {
        out.page((w, h), &more, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_like_the_panel() {
        assert_eq!(short_date("D:20261011151413-03'00'"), "11/10/2026 15:14");
        assert_eq!(short_date("D:2026"), "");
        assert_eq!(short_date("lixo"), "");
    }

    #[test]
    fn light_colors_get_darker_lines() {
        let yellow = line_color([1.0, 1.0, 0.0]);
        assert!(yellow.iter().all(|&v| v < 0.8), "{yellow:?}");
        assert_eq!(line_color([0.8, 0.1, 0.1]), [0.8, 0.1, 0.1]);
    }
}
