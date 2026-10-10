//! Marca d'água e Cabeçalho e rodapé (Acrobat), com numeração Bates: acrescentar, atualizar e
//! remover.
//!
//! Tudo vai no conteúdo das páginas dentro de `/Artifact <</Type/Pagination/Subtype/…>> BDC … EMC`,
//! o mesmo marcado que o Acrobat usa: remover apaga esses trechos (também os do Acrobat). A marca
//! d'água é um XObject de formulário só, desenhado em cada página; o cabeçalho e o rodapé são texto
//! por página (números, data, Bates). As escolhas ficam em `/GeraConfig` (JSON em hexadecimal) no
//! primeiro trecho, só para preencher a janela do Atualizar — lidas do arquivo, são dado não
//! confiável: validadas e limitadas.
use crate::create_export::{oriented, UprightImage};
use crate::doc::PdfDoc;
use crate::edit_text::{glyph_run, width};
use crate::error::{refused, Result};
use crate::page_content::{invert, mul, num, own_resource, page_content, Lexer, Tok};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::{Buffer, Font, Matrix};
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::path::PathBuf;

const RANGE_HINT: &str = "Use números de página, como 1-3, 7.";

/// "1-3, 7" → [(1, 3), (7, 7)], com páginas de 1 a `total`.
pub fn parse_ranges(text: &str, total: usize) -> std::result::Result<Vec<(usize, usize)>, String> {
    let num = |s: &str| {
        s.trim()
            .parse::<usize>()
            .map_err(|_| RANGE_HINT.to_string())
    };
    let mut out = Vec::new();
    for part in text.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(part)?;
                (n, n)
            }
        };
        if a == 0 {
            return Err("A primeira página é a 1.".into());
        }
        if a > b {
            return Err(format!("Intervalo invertido: {a}-{b}."));
        }
        if b > total {
            return Err(format!("O documento tem {total} páginas."));
        }
        out.push((a, b));
    }
    Ok(out)
}

/// Fonte padrão do PDF para o texto da marca e do cabeçalho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StdFont {
    #[default]
    Helvetica,
    Times,
    Courier,
}

/// Fonte, negrito, tamanho (pt) e cor do texto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLook {
    pub font: StdFont,
    pub bold: bool,
    pub size: f32,
    pub color: [u8; 3],
}

impl TextLook {
    fn load(&self) -> Result<Font> {
        let name = match (self.font, self.bold) {
            (StdFont::Helvetica, false) => "Helvetica",
            (StdFont::Helvetica, true) => "Helvetica-Bold",
            (StdFont::Times, false) => "Times-Roman",
            (StdFont::Times, true) => "Times-Bold",
            (StdFont::Courier, false) => "Courier",
            (StdFont::Courier, true) => "Courier-Bold",
        };
        Ok(Font::new(name)?)
    }

    /// Tamanho utilizável (o da janela, ou de uma configuração lida do arquivo).
    fn size(&self) -> f32 {
        if self.size.is_finite() {
            self.size.clamp(1.0, 500.0)
        } else {
            12.0
        }
    }

    fn color_ops(&self) -> String {
        let [r, g, b] = self.color.map(|c| num(c as f32 / 255.0));
        format!("{r} {g} {b} rg")
    }
}

/// Alinhamento: topo/esquerda, centro, base/direita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WatermarkSource {
    /// Texto (várias linhas, centradas).
    Text { text: String, look: TextLook },
    /// Imagem PNG ou JPEG.
    Image(PathBuf),
    /// Uma página (0-based) de outro PDF.
    PdfPage { path: PathBuf, page: i32 },
}

/// Em que páginas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSet {
    All,
    /// "1-3, 5" (páginas a partir de 1).
    Range(String),
    Even,
    Odd,
}

impl PageSet {
    /// As páginas (0-based) do documento de `total` páginas.
    pub fn pages(&self, total: i32) -> Result<Vec<i32>> {
        Ok(match self {
            PageSet::All => (0..total).collect(),
            PageSet::Even => (0..total).filter(|p| p % 2 == 1).collect(),
            PageSet::Odd => (0..total).filter(|p| p % 2 == 0).collect(),
            PageSet::Range(text) => {
                let mut out: Vec<i32> = parse_ranges(text, total.max(0) as usize)
                    .map_err(refused)?
                    .into_iter()
                    .flat_map(|(a, b)| (a as i32 - 1)..(b as i32))
                    .collect();
                out.sort_unstable();
                out.dedup();
                out
            }
        })
    }
}

/// Marca d'água (como a janela do Acrobat).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    pub source: WatermarkSource,
    /// Graus, sentido anti-horário.
    pub rotation: f32,
    /// 0 a 1.
    pub opacity: f32,
    /// Fração da página (a marca cabe nela); `None`: tamanho absoluto.
    pub scale: Option<f32>,
    /// Atrás do conteúdo da página (senão, na frente).
    pub behind: bool,
    /// Posição vertical (topo, centro, base) e distância em pt.
    pub vertical: (Align, f32),
    /// Posição horizontal (esquerda, centro, direita) e distância em pt.
    pub horizontal: (Align, f32),
    pub pages: PageSet,
}

/// Numeração Bates: prefixo, número com `digits` algarismos a partir de `start`, sufixo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bates {
    pub prefix: String,
    pub suffix: String,
    pub digits: u32,
    pub start: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateFormat {
    /// 10/10/2026
    #[default]
    DayMonthYear,
    /// 10/10/26
    DayMonthShort,
    /// 2026-10-10
    Iso,
}

impl DateFormat {
    pub fn pattern(self) -> &'static str {
        match self {
            DateFormat::DayMonthYear => "%d/%m/%Y",
            DateFormat::DayMonthShort => "%d/%m/%y",
            DateFormat::Iso => "%Y-%m-%d",
        }
    }
}

/// Cabeçalho e rodapé: as seis caixas aceitam `<<n>>` (número da página), `<<total>>`, `<<data>>`
/// e `<<bates>>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderFooter {
    /// Cabeçalho esquerdo, central, direito; rodapé esquerdo, central, direito.
    pub texts: [String; 6],
    pub look: TextLook,
    /// Margens superior, inferior, esquerda e direita (pt).
    pub margins: [f32; 4],
    /// Número da primeira página escolhida.
    pub start: u32,
    pub date_format: DateFormat,
    pub bates: Option<Bates>,
    pub pages: PageSet,
}

/// O que acrescentar ou remover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pagination {
    Watermark,
    HeaderFooter,
}

impl Pagination {
    /// Os /Subtype do marcado do Acrobat.
    fn subtypes(self) -> &'static [&'static [u8]] {
        match self {
            Pagination::Watermark => &[b"Watermark"],
            Pagination::HeaderFooter => &[b"Header", b"Footer"],
        }
    }
}

/// As escolhas guardadas no documento (para o Atualizar).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaginationConfig {
    Watermark(Watermark),
    HeaderFooter(HeaderFooter),
}

/// Um trecho marcado no conteúdo e as escolhas gravadas nele.
struct Marked {
    range: Range<usize>,
    config: Option<Vec<u8>>,
}

/// Os trechos `/Artifact <</Type/Pagination/Subtype/…>> BDC … EMC` de `kind` (os de fora; um trecho
/// dentro de outro sai junto com ele).
fn marked(content: &[u8], kind: Pagination) -> Vec<Marked> {
    let mut lx = Lexer {
        s: content,
        i: 0,
        inline_image: false,
    };
    let mut operands: Vec<(usize, usize, Tok)> = Vec::new();
    // pilha do marcado: Some((início, escolhas)) nos trechos procurados
    let mut stack: Vec<Option<(usize, Option<Vec<u8>>)>> = Vec::new();
    let mut out = Vec::new();
    while let Some((start, tok)) = lx.next() {
        let Tok::Op(op) = tok else {
            operands.push((start, lx.i, tok));
            continue;
        };
        match op {
            b"BDC" | b"BMC" => {
                let names: Vec<&[u8]> = operands
                    .iter()
                    .filter_map(|(_, _, t)| match t {
                        Tok::Name(n) => Some(*n),
                        _ => None,
                    })
                    .collect();
                let ours = op == b"BDC"
                    && names.first() == Some(&&b"Artifact"[..])
                    && names.contains(&&b"Pagination"[..])
                    && kind.subtypes().iter().any(|s| names.contains(s))
                    && !stack.iter().any(Option::is_some);
                let config = operands
                    .iter()
                    .position(|(_, _, t)| matches!(t, Tok::Name(n) if *n == b"GeraConfig"))
                    .and_then(|i| operands.get(i + 1))
                    .map(|(a, b, _)| content[*a..*b].to_vec());
                stack.push(ours.then(|| (operands.first().map_or(start, |o| o.0), config)));
            }
            b"EMC" => {
                if let Some(Some((from, config))) = stack.pop() {
                    out.push(Marked {
                        range: from..lx.i,
                        config,
                    });
                }
            }
            _ => {}
        }
        operands.clear();
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// `<…>` em hexadecimal → bytes.
fn unhex(raw: &[u8]) -> Option<Vec<u8>> {
    let digits: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();
    digits
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

/// Escolhas lidas do arquivo, com os valores limitados.
fn sanitized(mut config: PaginationConfig) -> PaginationConfig {
    let fix = |v: f32, lo: f32, hi: f32, default: f32| {
        if v.is_finite() {
            v.clamp(lo, hi)
        } else {
            default
        }
    };
    match &mut config {
        PaginationConfig::Watermark(w) => {
            w.rotation = fix(w.rotation, -360.0, 360.0, 0.0);
            w.opacity = fix(w.opacity, 0.0, 1.0, 0.5);
            w.scale = w.scale.map(|s| fix(s, 0.01, 10.0, 0.5));
            w.vertical.1 = fix(w.vertical.1, -10_000.0, 10_000.0, 0.0);
            w.horizontal.1 = fix(w.horizontal.1, -10_000.0, 10_000.0, 0.0);
            if let WatermarkSource::Text { look, .. } = &mut w.source {
                look.size = look.size();
            }
        }
        PaginationConfig::HeaderFooter(h) => {
            h.look.size = h.look.size();
            for m in &mut h.margins {
                *m = fix(*m, 0.0, 10_000.0, 36.0);
            }
            if let Some(b) = &mut h.bates {
                b.digits = b.digits.clamp(1, 12);
            }
        }
    }
    config
}

/// Matriz do retângulo `bbox` (x0, y0, largura, altura; y para cima) para a tela da página `page`
/// (x0, y0, x1, y1; y para baixo), com a escala, a rotação e a posição da marca.
fn place(w: &Watermark, bbox: [f32; 4], page: [f32; 4]) -> Matrix {
    let [bx, by, bw, bh] = bbox;
    let [x0, y0, x1, y1] = page;
    let (pw, ph) = (x1 - x0, y1 - y0);
    let s = w
        .scale
        .map_or(1.0, |f| f * (pw / bw.max(1e-3)).min(ph / bh.max(1e-3)));
    let rad = w.rotation.to_radians();
    let (c, sn) = (rad.cos().abs(), rad.sin().abs());
    let (rw, rh) = (s * (bw * c + bh * sn), s * (bw * sn + bh * c));
    let at = |(align, off): (Align, f32), lo: f32, hi: f32, size: f32| match align {
        Align::Start => lo + off + size / 2.0,
        Align::Center => (lo + hi) / 2.0 + off,
        Align::End => hi - off - size / 2.0,
    };
    let cx = at(w.horizontal, x0, x1, rw);
    let cy = at(w.vertical, y0, y1, rh);
    let mut m = Matrix::new_translate(-(bx + bw / 2.0), -(by + bh / 2.0));
    m = mul(&m, &Matrix::new_scale(s, -s));
    // na tela o y desce: anti-horário visto é o ângulo negativo
    m = mul(&m, &Matrix::new_rotate(-w.rotation));
    mul(&m, &Matrix::new_translate(cx, cy))
}

fn matrix_ops(m: &Matrix) -> String {
    [m.a, m.b, m.c, m.d, m.e, m.f].map(num).join(" ")
}

/// Primeiro nome `prefixN` livre no dicionário.
fn free_name(dict: &PdfObject, prefix: &str) -> Result<String> {
    for n in 0.. {
        let name = format!("{prefix}{n}");
        if dict.get_dict(name.as_str())?.is_none() {
            return Ok(name);
        }
    }
    unreachable!("sempre há um nome livre")
}

/// O início do trecho marcado, com as escolhas só no primeiro.
fn artifact(subtype: &str, config: Option<&PaginationConfig>) -> Result<String> {
    let config = match config {
        Some(c) => format!(
            "/GeraConfig <{}>",
            hex(&serde_json::to_vec(c).map_err(|e| refused(e.to_string()))?)
        ),
        None => String::new(),
    };
    Ok(format!(
        " /Artifact <</Type/Pagination/Subtype/{subtype}{config}>> BDC "
    ))
}

impl PdfDoc {
    /// Acrescenta a marca d'água (`replace`: no lugar das que já existem — o Atualizar).
    pub fn add_watermark(&mut self, w: &Watermark, replace: bool) -> Result<()> {
        self.require_modifiable("acrescentar a marca d'água")?;
        let pages = w.pages.pages(self.page_count()?)?;
        self.with_snapshot(|d| {
            if replace {
                d.strip(Pagination::Watermark)?;
            }
            let (form, bbox) = d.watermark_form(&w.source)?;
            let opacity = if w.opacity.is_finite() {
                w.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let gs = d
                .inner
                .new_object_from_str(&format!("<</Type/ExtGState/ca {0}/CA {0}>>", num(opacity)))?;
            let gs = d.inner.add_object(&gs)?;
            let config = PaginationConfig::Watermark(w.clone());
            for (i, &page) in pages.iter().enumerate() {
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                let mut page_obj = pdf_page.object();
                let to_screen = page_obj.page_ctm()?;
                let b = d.load(page)?.bounds()?;
                let mut xobjects = own_resource(&d.inner, &mut page_obj, "XObject")?;
                let xo = free_name(&xobjects, "GeraWm")?;
                xobjects.dict_put(xo.as_str(), form.clone())?;
                let mut states = own_resource(&d.inner, &mut page_obj, "ExtGState")?;
                let gs_name = free_name(&states, "GeraGs")?;
                states.dict_put(gs_name.as_str(), gs.clone())?;
                let m = place(w, bbox, [b.x0, b.y0, b.x1, b.y1]);
                let ops = format!(
                    "{}q {} cm {} cm /{gs_name} gs /{xo} Do Q EMC ",
                    artifact("Watermark", (i == 0).then_some(&config))?,
                    matrix_ops(&invert(&to_screen)?),
                    matrix_ops(&m),
                );
                if w.behind {
                    pdf_page.insert_contents(&mut d.inner, ops.as_bytes(), false)?;
                } else {
                    // o conteúdo antigo entre q/Q: a marca não herda matriz, cor nem recorte dele
                    pdf_page.wrap_contents(&mut d.inner)?;
                    pdf_page.insert_contents(&mut d.inner, ops.as_bytes(), true)?;
                }
            }
            Ok(())
        })
    }

    /// O XObject de formulário da marca e a caixa dele (x0, y0, largura, altura).
    fn watermark_form(&mut self, source: &WatermarkSource) -> Result<(PdfObject, [f32; 4])> {
        let (content, resources, bbox) = match source {
            WatermarkSource::Text { text, look } => {
                let font = look.load()?;
                let size = look.size();
                let lines: Vec<&str> = text.lines().collect();
                if text.trim().is_empty() {
                    return Err(refused("escreva o texto da marca d'água"));
                }
                let widths = lines
                    .iter()
                    .map(|l| width(&font, l, size))
                    .collect::<Result<Vec<f32>>>()?;
                let w = widths.iter().copied().fold(size, f32::max);
                let lh = 1.2 * size;
                let h = lines.len() as f32 * lh;
                let mut ops = format!("BT {} /F0 {} Tf ", look.color_ops(), num(size));
                for (i, (line, lw)) in lines.iter().zip(&widths).enumerate() {
                    let y = h - i as f32 * lh - 0.95 * size;
                    ops.push_str(&format!(
                        "1 0 0 1 {} {} Tm {} ",
                        num((w - lw) / 2.0),
                        num(y),
                        glyph_run(&font, line)?
                    ));
                }
                ops.push_str("ET");
                let mut res = self.inner.new_object_from_str("<</Font<<>>>>")?;
                let mut fonts = res.get_dict("Font")?.expect("acabou de ser posto");
                fonts.dict_put("F0", self.inner.add_font(&font)?)?;
                res.dict_put("Font", fonts)?;
                (ops, res, [0.0, 0.0, w, h])
            }
            WatermarkSource::Image(path) => {
                let up = UprightImage::open(path)?;
                let img = self.inner.add_image(&up.img)?;
                let [a, b, c, d, e, f] = oriented(up.orientation, up.width, up.height);
                let ops = format!("q {} cm /Im0 Do Q", [a, b, c, d, e, f].map(num).join(" "));
                let mut res = self.inner.new_object_from_str("<</XObject<<>>>>")?;
                let mut x = res.get_dict("XObject")?.expect("acabou de ser posto");
                x.dict_put("Im0", img)?;
                res.dict_put("XObject", x)?;
                (ops, res, [0.0, 0.0, up.width, up.height])
            }
            WatermarkSource::PdfPage { path, page } => {
                let src = PdfDocument::open(path.as_path())
                    .map_err(|e| refused(format!("não foi possível abrir o PDF: {e}")))?;
                if src.needs_password()? {
                    return Err(refused("o PDF escolhido é protegido por senha"));
                }
                if *page < 0 || *page >= src.page_count()? {
                    return Err(refused("o PDF escolhido não tem essa página"));
                }
                let obj = src.find_page(*page)?;
                let media = obj
                    .get_dict_inheritable("MediaBox")?
                    .ok_or_else(|| refused("a página escolhida não tem tamanho"))?;
                let mut v = [0.0; 4];
                for (i, x) in v.iter_mut().enumerate() {
                    *x = media
                        .get_array(i as i32)?
                        .map(|n| n.as_float())
                        .transpose()?
                        .unwrap_or(0.0);
                }
                let content = String::from_utf8_lossy(&page_content(&obj)?).into_owned();
                let res = match obj.get_dict_inheritable("Resources")? {
                    Some(r) => self.inner.graft_object(&r)?,
                    None => self.inner.new_dict()?,
                };
                (content, res, [v[0], v[1], v[2] - v[0], v[3] - v[1]])
            }
        };
        let [x, y, w, h] = bbox;
        let mut dict = self.inner.new_object_from_str(&format!(
            "<</Type/XObject/Subtype/Form/BBox[{} {} {} {}]>>",
            num(x),
            num(y),
            num(x + w),
            num(y + h)
        ))?;
        dict.dict_put("Resources", resources)?;
        let form =
            self.inner
                .add_stream(&Buffer::from_bytes(content.as_bytes())?, Some(&dict), false)?;
        Ok((form, bbox))
    }

    /// Acrescenta o cabeçalho e rodapé (`replace`: no lugar dos que já existem).
    pub fn add_header_footer(&mut self, h: &HeaderFooter, replace: bool) -> Result<()> {
        self.require_modifiable("acrescentar o cabeçalho e rodapé")?;
        let total = self.page_count()?;
        let pages = h.pages.pages(total)?;
        if h.texts.iter().all(|t| t.trim().is_empty()) {
            return Err(refused("escreva o texto de alguma das caixas"));
        }
        let today = chrono::Local::now()
            .format(h.date_format.pattern())
            .to_string();
        self.with_snapshot(|d| {
            if replace {
                d.strip(Pagination::HeaderFooter)?;
            }
            let font = h.look.load()?;
            let size = h.look.size();
            let config = PaginationConfig::HeaderFooter(h.clone());
            let [mt, mb, ml, mr] = h.margins;
            for (i, &page) in pages.iter().enumerate() {
                let number = h.start as usize + i;
                let expand = |t: &str| {
                    let mut t = t
                        .replace("<<n>>", &number.to_string())
                        .replace("<<total>>", &total.to_string())
                        .replace("<<data>>", &today);
                    if let Some(b) = &h.bates {
                        let digits = b.digits.clamp(1, 12) as usize;
                        let n = b.start as usize + i;
                        t = t.replace(
                            "<<bates>>",
                            &format!("{}{n:0digits$}{}", b.prefix, b.suffix),
                        );
                    }
                    t
                };
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                let mut page_obj = pdf_page.object();
                let to_screen = page_obj.page_ctm()?;
                let b = d.load(page)?.bounds()?;
                let name = d.font_resource(&mut page_obj, &font)?;
                let lh = 1.2 * size;
                let mut ops = String::new();
                for (slots, subtype) in [(0..3, "Header"), (3..6, "Footer")] {
                    let mut runs = String::new();
                    for slot in slots {
                        let text = expand(&h.texts[slot]);
                        let lines: Vec<&str> = text.lines().collect();
                        if text.trim().is_empty() {
                            continue;
                        }
                        let n = lines.len();
                        for (k, line) in lines.iter().enumerate() {
                            let w = width(&font, line, size)?;
                            let x = match slot % 3 {
                                0 => b.x0 + ml,
                                1 => (b.x0 + b.x1 - w) / 2.0,
                                _ => b.x1 - mr - w,
                            };
                            // a caixa das letras (ascendente a descendente) fica dentro da margem
                            let y = if slot < 3 {
                                b.y0 + mt + font.ascender() * size + k as f32 * lh
                            } else {
                                b.y1 - mb + font.descender() * size - (n - 1 - k) as f32 * lh
                            };
                            runs.push_str(&format!(
                                "1 0 0 -1 {} {} Tm {} ",
                                num(x),
                                num(y),
                                glyph_run(&font, line)?
                            ));
                        }
                    }
                    if runs.is_empty() {
                        continue;
                    }
                    let first = i == 0 && ops.is_empty();
                    ops.push_str(&format!(
                        "{}q {} cm BT {} /{name} {} Tf {runs}ET Q EMC ",
                        artifact(subtype, first.then_some(&config))?,
                        matrix_ops(&invert(&to_screen)?),
                        h.look.color_ops(),
                        num(size),
                    ));
                }
                pdf_page.wrap_contents(&mut d.inner)?;
                pdf_page.insert_contents(&mut d.inner, ops.as_bytes(), true)?;
            }
            Ok(())
        })
    }

    /// Tira as marcas d'água (ou os cabeçalhos e rodapés) de todas as páginas — também as do
    /// Acrobat. Devolve em quantas páginas havia.
    pub fn remove_pagination(&mut self, kind: Pagination) -> Result<usize> {
        self.require_modifiable("remover")?;
        self.with_snapshot(|d| d.strip(kind))
    }

    fn strip(&mut self, kind: Pagination) -> Result<usize> {
        let mut touched = 0;
        for page in 0..self.page_count()? {
            let mut page_obj = self.inner.find_page(page)?;
            let content = page_content(&page_obj)?;
            let found = marked(&content, kind);
            if found.is_empty() {
                continue;
            }
            let mut out = Vec::with_capacity(content.len());
            let mut at = 0;
            for m in &found {
                out.extend(&content[at..m.range.start]);
                at = m.range.end;
            }
            out.extend(&content[at..]);
            let stream = self
                .inner
                .add_stream(&Buffer::from_bytes(&out)?, None, false)?;
            page_obj.dict_put("Contents", stream)?;
            touched += 1;
        }
        Ok(touched)
    }

    /// Há marca d'água (ou cabeçalho e rodapé) em alguma página?
    pub fn has_pagination(&self, kind: Pagination) -> Result<bool> {
        for page in 0..self.page_count()? {
            if !marked(&page_content(&self.inner.find_page(page)?)?, kind).is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// As escolhas gravadas pelo app na marca (ou no cabeçalho) existente, para o Atualizar.
    pub fn pagination_config(&self, kind: Pagination) -> Result<Option<PaginationConfig>> {
        for page in 0..self.page_count()? {
            let content = page_content(&self.inner.find_page(page)?)?;
            for m in marked(&content, kind) {
                let Some(config) = m
                    .config
                    .as_deref()
                    .and_then(unhex)
                    .and_then(|b| serde_json::from_slice::<PaginationConfig>(&b).ok())
                else {
                    continue;
                };
                let matches = matches!(
                    (&config, kind),
                    (PaginationConfig::Watermark(_), Pagination::Watermark)
                        | (PaginationConfig::HeaderFooter(_), Pagination::HeaderFooter)
                );
                if matches {
                    return Ok(Some(sanitized(config)));
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_marked_content_is_removed_with_its_artifact() {
        let content = b"q /Artifact <</Subtype /Watermark /Type /Pagination>> BDC /OC /x BDC (a) Tj EMC EMC Q \
            /Span <</MCID 1>> BDC (b) Tj EMC";
        let found = marked(content, Pagination::Watermark);
        assert_eq!(found.len(), 1);
        let r = found[0].range.clone();
        assert!(content[r.clone()].starts_with(b"/Artifact") && content[r].ends_with(b"EMC EMC"));
        assert!(marked(content, Pagination::HeaderFooter).is_empty());
    }

    #[test]
    fn the_config_hex_round_trips() {
        let c = PaginationConfig::HeaderFooter(HeaderFooter {
            texts: Default::default(),
            look: TextLook {
                font: StdFont::Times,
                bold: false,
                size: 9999.0,
                color: [1, 2, 3],
            },
            margins: [36.0; 4],
            start: 1,
            date_format: DateFormat::Iso,
            bates: None,
            pages: PageSet::Odd,
        });
        let raw = format!("<{}>", hex(&serde_json::to_vec(&c).unwrap()));
        let back: PaginationConfig =
            serde_json::from_slice(&unhex(raw.as_bytes()).unwrap()).unwrap();
        let PaginationConfig::HeaderFooter(h) = sanitized(back) else {
            panic!()
        };
        assert_eq!(h.look.size, 500.0, "tamanho absurdo é limitado");
    }

    #[test]
    fn page_sets_pick_pages() {
        assert_eq!(PageSet::Odd.pages(5).unwrap(), [0, 2, 4]);
        assert_eq!(PageSet::Range("3, 1-2".into()).pages(5).unwrap(), [0, 1, 2]);
    }
}
