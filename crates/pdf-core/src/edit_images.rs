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
use crate::page_content::{
    decode_name, free_name, invert, is_delim, matrix_ops, mul, own_resource, page_content, Lexer,
    Tok,
};
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
    /// Se o `Do` está sozinho num `q … cm /Nome Do Q`: a faixa do bloco e a matriz antes dele —
    /// editar regrava o bloco em vez de aninhar mais um `q`.
    block: Option<(std::ops::Range<usize>, Matrix)>,
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
    let mut draws: Vec<Draw> = Vec::new();
    // `q` sem operandos (início e matriz de antes) e se já veio o `cm` logo depois dele
    let mut open: Option<(usize, Matrix, bool)> = None;
    while let Some((start, tok)) = lx.next() {
        let Tok::Op(op) = tok else {
            operands.push((start, tok));
            continue;
        };
        let prev = open.take();
        match op {
            b"q" => {
                if operands.is_empty() {
                    open = Some((start, ctm.clone(), false));
                }
                stack.push(ctm.clone())
            }
            b"Q" => {
                ctm = stack.pop().unwrap_or(Matrix::IDENTITY);
                // fecha `q cm Do` (marcado no Do como bloco em aberto)
                if let (Some((q, before, true)), true, Some(d)) =
                    (prev, operands.is_empty(), draws.last_mut())
                {
                    if d.range.end <= start && d.range.start > q && d.block.is_none() {
                        d.block = Some((q..lx.i, before));
                    }
                }
            }
            b"cm" => {
                if let Some((q, before, false)) = prev {
                    if operands.len() == 6 {
                        open = Some((q, before, true));
                    }
                }
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
                        block: None,
                    });
                    // ainda pode fechar um bloco: o Q seguinte decide
                    if operands.len() == 1 {
                        open = prev.filter(|p| p.2);
                    }
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

/// Acrescenta a imagem do arquivo aos recursos da página com um nome livre; devolve o nome.
fn add_xobject(
    doc: &mut PdfDocument,
    page_obj: &mut PdfObject,
    up: &UprightImage,
) -> Result<String> {
    let image = doc.add_image(&up.img)?;
    let mut xobjects = own_resource(doc, page_obj, "XObject")?;
    let name = free_name(&xobjects, "GeraIm")?;
    xobjects.dict_put(name.as_str(), image)?;
    Ok(name)
}

/// Com espaço dos dois lados: o vizinho pode estar colado (`cm/Im0 Do`, válido no PDF).
fn draw_ops(x: &Matrix, name: &str) -> String {
    format!(" q {} cm {} Do Q ", matrix_ops(x), encode_name(name))
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
        // num bloco `q cm Do Q` próprio, ele todo é regravado; senão, o Do vira um
        let (range, base) = draw
            .block
            .clone()
            .unwrap_or((draw.range.clone(), draw.ctm.clone()));
        let ops = match new_screen {
            None => String::new(),
            // a matriz nova, relativa à que já vale onde o bloco (ou o Do) está
            Some(s) => {
                let x = mul(&mul(&s, &invert(&to_screen)?), &invert(&base)?);
                draw_ops(&x, &new_name)
            }
        };
        let mut out = content[..range.start].to_vec();
        out.extend(ops.as_bytes());
        out.extend(&content[range.end..]);
        let stream = self
            .inner
            .add_stream(&Buffer::from_bytes(&out)?, None, false)?;
        page_obj.dict_put("Contents", stream)?;
        if new_name != name {
            // excluída ou substituída: a imagem antiga sai dos recursos (e do arquivo, ao salvar)
            own_resource(&self.inner, &mut page_obj, "XObject")?.dict_delete(name)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_content::num;

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
    fn own_q_cm_do_q_blocks_are_found() {
        let content =
            b"q 1 0 0 1 5 5 cm q 2 0 0 2 0 0 cm /A Do Q q 2 0 0 2 0 0 cm 0 g /B Do Q /C Do";
        let draws = scan(content).draws;
        let (range, before) = draws[0].block.clone().expect("bloco de A");
        assert_eq!(&content[range], b"q 2 0 0 2 0 0 cm /A Do Q");
        assert_eq!(before.e, 5.0);
        assert!(draws[1].block.is_none() && draws[2].block.is_none());
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
