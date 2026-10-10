//! Editar imagens (modo Editar PDF do Acrobat): mover, redimensionar, girar, excluir, substituir
//! e adicionar imagens da página.
//!
//! A imagem é um XObject desenhado por `/Nome Do` no conteúdo da página. Editar troca só esse
//! operador por `q X cm /Nome Do Q` (ou o apaga), no mesmo lugar: a ordem de empilhamento, o
//! objeto da imagem e o resto da página ficam como estavam. Imagem desenhada mais de uma vez,
//! dentro de um formulário (Form XObject) ou embutida (BI/EI) não é editável.
use crate::create_export::{oriented, UprightImage};
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::types::PdfRect;
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::text_page::TextBlockType;
use mupdf::{Buffer, Matrix, Rect, TextPageFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Imagem da página.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    /// Nome no /XObject da página ("" para imagem de formulário ou embutida).
    pub name: String,
    /// Caixa na tela (y para baixo, com a rotação da página).
    pub rect: PdfRect,
    pub editable: bool,
}

/// Edição de uma imagem; medidas na tela.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageEdit {
    Move {
        dx: f32,
        dy: f32,
    },
    /// Nova caixa (a imagem é esticada para ela).
    Resize {
        rect: PdfRect,
    },
    /// 90° em torno do centro.
    Rotate {
        clockwise: bool,
    },
    Delete,
    /// Outra imagem no lugar, no maior tamanho que cabe na caixa da antiga, centrada.
    Replace {
        path: PathBuf,
    },
}

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = a.clone();
    m.concat(b.clone());
    m
}

fn invert(m: &Matrix) -> Result<Matrix> {
    m.invert()
        .ok_or_else(|| refused("imagem com transformação inválida"))
}

/// Caixa do quadrado unitário pela matriz.
fn unit_bbox(m: &Matrix) -> PdfRect {
    Rect::new(0.0, 0.0, 1.0, 1.0).transform(m).into()
}

/// Matriz que leva a caixa `from` para a caixa `to`.
fn box_to_box(from: &PdfRect, to: &PdfRect) -> Matrix {
    let sx = to.width() / from.width().max(1e-3);
    let sy = to.height() / from.height().max(1e-3);
    Matrix::new(sx, 0.0, 0.0, sy, to.x0 - from.x0 * sx, to.y0 - from.y0 * sy)
}

/// Matriz de tela que desenha a imagem em pé (`up`) ocupando a caixa `r`.
fn upright_in(up: &UprightImage, r: &PdfRect) -> Matrix {
    let [a, b, c, d, e, f] = oriented(up.orientation, r.width(), r.height());
    // `oriented` é de uma caixa com y para cima a partir de (0, 0); a tela tem y para baixo
    let flip = Matrix::new(1.0, 0.0, 0.0, -1.0, r.x0, r.y1);
    mul(&Matrix::new(a, b, c, d, e, f), &flip)
}

/// A maior caixa com a proporção `w`×`h` que cabe em `r`, centrada.
fn fit_in(w: f32, h: f32, r: &PdfRect) -> PdfRect {
    let s = (r.width() / w).min(r.height() / h);
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    PdfRect::new(
        cx - w * s / 2.0,
        cy - h * s / 2.0,
        cx + w * s / 2.0,
        cy + h * s / 2.0,
    )
}

// ---------- leitura do conteúdo da página ----------

enum Tok<'a> {
    Num(f32),
    /// Nome ainda com as sequências `#xx`.
    Name(&'a [u8]),
    Op(&'a [u8]),
    Other,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0)
}

fn is_delim(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Tokenizador do conteúdo: pula comentários, strings e dados de imagem embutida, para que um
/// "Do" dentro deles não conte.
struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
    /// Achou imagem embutida (BI…ID…EI).
    inline_image: bool,
}

impl<'a> Lexer<'a> {
    fn word(&mut self) -> &'a [u8] {
        let s = self.s;
        let start = self.i;
        while self.i < s.len() && !is_ws(s[self.i]) && !is_delim(s[self.i]) {
            self.i += 1;
        }
        &s[start..self.i]
    }

    /// Pula os dados depois de ID até o EI (cercado de espaço) que fecha a imagem embutida.
    fn skip_inline_data(&mut self) {
        self.inline_image = true;
        let s = self.s;
        let mut j = self.i + 1;
        while j + 2 <= s.len() {
            if is_ws(s[j - 1])
                && &s[j..j + 2] == b"EI"
                && s.get(j + 2).is_none_or(|&c| is_ws(c) || is_delim(c))
            {
                self.i = j + 2;
                return;
            }
            j += 1;
        }
        self.i = s.len();
    }

    /// Próximo token e onde começa.
    fn next(&mut self) -> Option<(usize, Tok<'a>)> {
        let s = self.s;
        loop {
            while self.i < s.len() && is_ws(s[self.i]) {
                self.i += 1;
            }
            if s.get(self.i) == Some(&b'%') {
                while self.i < s.len() && !matches!(s[self.i], b'\r' | b'\n') {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
        let start = self.i;
        let c = *s.get(self.i)?;
        let tok = match c {
            b'(' => {
                let mut depth = 0usize;
                while self.i < s.len() {
                    match s[self.i] {
                        b'\\' => self.i += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                self.i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                Tok::Other
            }
            b'<' if s.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                Tok::Other
            }
            b'<' => {
                while self.i < s.len() && s[self.i] != b'>' {
                    self.i += 1;
                }
                self.i += 1;
                Tok::Other
            }
            b'>' if s.get(self.i + 1) == Some(&b'>') => {
                self.i += 2;
                Tok::Other
            }
            b'/' => {
                self.i += 1;
                Tok::Name(self.word())
            }
            _ if is_delim(c) => {
                self.i += 1;
                Tok::Other
            }
            _ => {
                let w = self.word();
                if w == b"ID" {
                    self.skip_inline_data();
                    Tok::Other
                } else {
                    match std::str::from_utf8(w).ok().and_then(|t| t.parse().ok()) {
                        Some(n) => Tok::Num(n),
                        None => Tok::Op(w),
                    }
                }
            }
        };
        Some((start, tok))
    }
}

/// Nome do PDF sem as sequências `#xx`.
fn decode_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = raw
            .get(i + 1..i + 3)
            .filter(|_| raw[i] == b'#')
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(raw[i]);
                i += 1;
            }
        }
    }
    out
}

/// Nome do PDF com os caracteres especiais em `#xx`.
fn encode_name(name: &str) -> String {
    let mut out = String::from("/");
    for &b in name.as_bytes() {
        if b <= b' ' || b >= 0x7f || b == b'#' || is_delim(b) {
            out.push_str(&format!("#{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// Um `/Nome Do` do conteúdo: onde está e a matriz em vigor (da imagem para o espaço da página).
struct Draw {
    name: Vec<u8>,
    range: std::ops::Range<usize>,
    ctm: Matrix,
}

/// O conteúdo lido: os `Do` e se há imagem embutida.
struct Scan {
    draws: Vec<Draw>,
    inline_image: bool,
}

impl Scan {
    /// Quantas vezes cada nome é desenhado.
    fn times(&self) -> HashMap<&[u8], usize> {
        let mut times = HashMap::new();
        for d in &self.draws {
            *times.entry(d.name.as_slice()).or_insert(0) += 1;
        }
        times
    }
}

/// Os `Do` do conteúdo, com a matriz de cada um (pilha q/Q e cm).
fn scan(content: &[u8]) -> Scan {
    let mut lx = Lexer {
        s: content,
        i: 0,
        inline_image: false,
    };
    let mut ctm = Matrix::IDENTITY;
    let mut stack = Vec::new();
    let mut operands: Vec<(usize, Tok)> = Vec::new();
    let mut draws = Vec::new();
    while let Some((start, tok)) = lx.next() {
        let Tok::Op(op) = tok else {
            operands.push((start, tok));
            continue;
        };
        match op {
            b"q" => stack.push(ctm.clone()),
            b"Q" => ctm = stack.pop().unwrap_or(Matrix::IDENTITY),
            b"cm" => {
                let n: Vec<f32> = operands
                    .iter()
                    .rev()
                    .take(6)
                    .filter_map(|(_, t)| match t {
                        Tok::Num(v) => Some(*v),
                        _ => None,
                    })
                    .collect();
                if let [f, e, d, c, b, a] = n[..] {
                    ctm = mul(&Matrix::new(a, b, c, d, e, f), &ctm);
                }
            }
            b"Do" => {
                if let Some((at, Tok::Name(name))) = operands.last() {
                    draws.push(Draw {
                        name: decode_name(name),
                        range: *at..lx.i,
                        ctm: ctm.clone(),
                    });
                }
            }
            _ => {}
        }
        operands.clear();
    }
    Scan {
        draws,
        inline_image: lx.inline_image,
    }
}

/// Conteúdo da página (os fluxos de /Contents, um depois do outro).
fn page_content(page_obj: &PdfObject) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let Some(contents) = page_obj.get_dict("Contents")? else {
        return Ok(out);
    };
    if contents.is_array()? {
        for i in 0..contents.len()? as i32 {
            if let Some(s) = contents.get_array(i)? {
                out.extend(s.read_stream()?);
                out.push(b'\n');
            }
        }
    } else {
        out.extend(contents.read_stream()?);
    }
    Ok(out)
}

/// O /XObject da página (próprio ou herdado).
fn xobjects(page_obj: &PdfObject) -> Result<Option<PdfObject>> {
    Ok(page_obj
        .get_dict_inheritable("Resources")?
        .map(|r| r.get_dict("XObject"))
        .transpose()?
        .flatten())
}

/// /Subtype do XObject `name` (Image, Form), se existir.
fn xobject_kind(xobjects: Option<&PdfObject>, name: &[u8]) -> Result<Option<Vec<u8>>> {
    let (Some(xobjects), Ok(key)) = (xobjects, std::str::from_utf8(name)) else {
        return Ok(None);
    };
    let Some(x) = xobjects.get_dict(key)? else {
        return Ok(None);
    };
    Ok(x.get_dict("Subtype")?.and_then(|s| s.as_name().ok()))
}

/// O /XObject próprio da página, para acrescentar ou tirar imagens: os recursos (herdados ou de
/// outras páginas) são copiados, para não mudar as outras páginas.
fn own_xobjects(doc: &PdfDocument, page_obj: &mut PdfObject) -> Result<PdfObject> {
    let copy = |o: Option<PdfObject>| -> Result<PdfObject> {
        Ok(match o {
            Some(d) if d.is_dict()? => d.copy_dict()?,
            _ => doc.new_dict()?,
        })
    };
    let mut res = copy(page_obj.get_dict_inheritable("Resources")?)?;
    let xobjects = copy(res.get_dict("XObject")?)?;
    res.dict_put("XObject", xobjects)?;
    page_obj.dict_put("Resources", res)?;
    Ok(page_obj
        .get_dict("Resources")?
        .and_then(|r| r.get_dict("XObject").ok().flatten())
        .expect("acabou de ser posto"))
}

/// Acrescenta a imagem do arquivo aos recursos da página com um nome livre; devolve o nome.
fn add_xobject(
    doc: &mut PdfDocument,
    page_obj: &mut PdfObject,
    up: &UprightImage,
) -> Result<String> {
    let image = doc.add_image(&up.img)?;
    let mut xobjects = own_xobjects(doc, page_obj)?;
    let name = (0..)
        .map(|n| format!("GeraIm{n}"))
        .find(|n| xobjects.get_dict(n.as_str()).ok().flatten().is_none())
        .expect("sempre há um nome livre");
    xobjects.dict_put(name.as_str(), image)?;
    Ok(name)
}

/// Número para o conteúdo da página: até 6 casas, sem zeros sobrando.
fn num(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

fn draw_ops(x: &Matrix, name: &str) -> String {
    let m = [x.a, x.b, x.c, x.d, x.e, x.f].map(num).join(" ");
    format!("q {m} cm {} Do Q", encode_name(name))
}

impl PdfDoc {
    /// Imagens da página, na ordem em que são desenhadas.
    pub fn page_images(&self, page: i32) -> Result<Vec<PageImage>> {
        self.check_page(page)?;
        let page_obj = self.inner.find_page(page)?;
        let to_screen = page_obj.page_ctm()?;
        let scan = scan(&page_content(&page_obj)?);
        let xobjects = xobjects(&page_obj)?;
        let times = scan.times();
        let mut images = Vec::new();
        let mut in_forms = scan.inline_image;
        for d in &scan.draws {
            match xobject_kind(xobjects.as_ref(), &d.name)?.as_deref() {
                Some(b"Image") => images.push(PageImage {
                    name: String::from_utf8_lossy(&d.name).into_owned(),
                    rect: unit_bbox(&mul(&d.ctm, &to_screen)),
                    editable: times[d.name.as_slice()] == 1,
                }),
                _ => in_forms = true,
            }
        }
        if !in_forms {
            return Ok(images);
        }
        // as que o conteúdo da página não desenha direto (formulário, embutida): só o contorno
        let text = self
            .load(page)?
            .to_text_page(TextPageFlags::PRESERVE_IMAGES)?;
        for block in text.blocks() {
            if block.r#type() != TextBlockType::Image {
                continue;
            }
            let rect = PdfRect::from(block.bounds());
            if !images.iter().any(|i| i.rect.near(&rect, 1.0)) {
                images.push(PageImage {
                    name: String::new(),
                    rect,
                    editable: false,
                });
            }
        }
        Ok(images)
    }

    /// Edita a imagem `name` da página. Desfazer volta o documento todo de antes.
    pub fn edit_image(&mut self, page: i32, name: &str, edit: ImageEdit) -> Result<()> {
        self.guarded_page_edit(page, "editar a imagem", |doc| {
            doc.apply_image_edit(page, name, edit)
        })
    }

    /// Acrescenta a imagem do arquivo centrada em `center` (tela), no tamanho natural dela
    /// (reduzida para caber na página), por cima do conteúdo.
    pub fn add_image(&mut self, page: i32, path: &Path, center: (f32, f32)) -> Result<()> {
        self.guarded_page_edit(page, "acrescentar a imagem", |doc| {
            let up = UprightImage::open(path)?;
            let mut pdf_page = doc.inner.load_pdf_page(page)?;
            let mut page_obj = pdf_page.object();
            let to_screen = page_obj.page_ctm()?;
            let b = doc.load(page)?.bounds()?;
            let fit = (0.9 * (b.x1 - b.x0) / up.width)
                .min(0.9 * (b.y1 - b.y0) / up.height)
                .min(1.0);
            let (w, h) = (up.width * fit, up.height * fit);
            // centrada no clique, sem sair da página
            let x0 = (center.0 - w / 2.0).clamp(b.x0, b.x1 - w);
            let y0 = (center.1 - h / 2.0).clamp(b.y0, b.y1 - h);
            let screen = upright_in(&up, &PdfRect::new(x0, y0, x0 + w, y0 + h));
            let name = add_xobject(&mut doc.inner, &mut page_obj, &up)?;
            let x = mul(&screen, &invert(&to_screen)?);
            // o conteúdo antigo entre q/Q: a imagem nova não herda matriz nem recorte dele
            pdf_page.wrap_contents(&mut doc.inner)?;
            pdf_page.insert_contents(&mut doc.inner, draw_ops(&x, &name).as_bytes(), true)?;
            Ok(())
        })
    }

    /// Roda `edit` na página com as recusas de alteração; em erro o documento volta como
    /// estava, e em sucesso o desfazer ganha o instantâneo de antes.
    fn guarded_page_edit(
        &mut self,
        page: i32,
        action: &str,
        edit: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        self.require_modifiable(action)?;
        self.check_page(page)?;
        self.with_snapshot(edit)
    }

    fn apply_image_edit(&mut self, page: i32, name: &str, edit: ImageEdit) -> Result<()> {
        let mut page_obj = self.inner.find_page(page)?;
        let to_screen = page_obj.page_ctm()?;
        let content = page_content(&page_obj)?;
        let scan = scan(&content);
        let mut mine = scan.draws.iter().filter(|d| d.name == name.as_bytes());
        let (Some(draw), None) = (mine.next(), mine.next()) else {
            return Err(refused(
                "esta imagem não pode ser editada (aparece mais de uma vez ou não está na página)",
            ));
        };
        if xobject_kind(xobjects(&page_obj)?.as_ref(), &draw.name)?.as_deref() != Some(b"Image") {
            return Err(refused("não é uma imagem da página"));
        }
        let screen = mul(&draw.ctm, &to_screen);
        let bbox = unit_bbox(&screen);
        let (new_screen, new_name) = match edit {
            ImageEdit::Delete => (None, String::new()),
            ImageEdit::Move { dx, dy } => (
                Some(mul(&screen, &Matrix::new_translate(dx, dy))),
                name.to_owned(),
            ),
            ImageEdit::Resize { rect } => {
                if rect.width() < 1.0 || rect.height() < 1.0 {
                    return Err(refused("tamanho pequeno demais"));
                }
                (
                    Some(mul(&screen, &box_to_box(&bbox, &rect))),
                    name.to_owned(),
                )
            }
            ImageEdit::Rotate { clockwise } => {
                let (cx, cy) = ((bbox.x0 + bbox.x1) / 2.0, (bbox.y0 + bbox.y1) / 2.0);
                let mut turn = Matrix::new_translate(-cx, -cy);
                turn.concat(Matrix::new_rotate(if clockwise { 90.0 } else { -90.0 }));
                turn.concat(Matrix::new_translate(cx, cy));
                (Some(mul(&screen, &turn)), name.to_owned())
            }
            ImageEdit::Replace { path } => {
                let up = UprightImage::open(&path)?;
                let target = fit_in(up.width, up.height, &bbox);
                let new_name = add_xobject(&mut self.inner, &mut page_obj, &up)?;
                (Some(upright_in(&up, &target)), new_name)
            }
        };
        let ops = match new_screen {
            None => String::new(),
            // a matriz nova, relativa à que já vale onde o Do está
            Some(s) => {
                let x = mul(&mul(&s, &invert(&to_screen)?), &invert(&draw.ctm)?);
                draw_ops(&x, &new_name)
            }
        };
        let mut out = content[..draw.range.start].to_vec();
        out.extend(ops.as_bytes());
        out.extend(&content[draw.range.end..]);
        let stream = self
            .inner
            .add_stream(&Buffer::from_bytes(&out)?, None, false)?;
        page_obj.dict_put("Contents", stream)?;
        if new_name != name {
            // excluída ou substituída: a imagem antiga sai dos recursos (e do arquivo, ao salvar)
            own_xobjects(&self.inner, &mut page_obj)?.dict_delete(name)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do_inside_strings_comments_and_inline_images_is_ignored() {
        let content = b"BT (a /X Do) Tj ET % /Y Do\nBI /W 1 /H 1 ID \x00/Z Do EI\n\
                        q 2 0 0 3 10 20 cm /Im#20A Do Q";
        let draws = scan(content).draws;
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].name, b"Im A");
        let m = &draws[0].ctm;
        assert_eq!((m.a, m.d, m.e, m.f), (2.0, 3.0, 10.0, 20.0));
        assert_eq!(&content[draws[0].range.clone()], b"/Im#20A Do");
        assert!(scan(content).inline_image && !scan(b"/A Do").inline_image);
    }

    #[test]
    fn nested_cm_composes_and_q_restores() {
        let draws = scan(b"q 1 0 0 1 5 5 cm q 2 0 0 2 0 0 cm /A Do Q /B Do Q /C Do").draws;
        let e = |i: usize| (draws[i].ctm.a, draws[i].ctm.e);
        assert_eq!(e(0), (2.0, 5.0));
        assert_eq!(e(1), (1.0, 5.0));
        assert_eq!(e(2), (1.0, 0.0));
    }

    #[test]
    fn numbers_are_short_and_exact_enough() {
        assert_eq!(num(200.0), "200");
        assert_eq!(num(0.3), "0.3");
        assert_eq!(num(-0.0000001), "0");
        assert_eq!(num(-12.5), "-12.5");
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(encode_name("Im A#"), "/Im#20A#23");
        assert_eq!(decode_name(b"Im#20A#23"), b"Im A#");
    }
}
