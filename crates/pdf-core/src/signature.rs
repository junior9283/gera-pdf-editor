//! Assinatura e iniciais do Preencher e assinar: carimbo com aparência vetorial própria (nítida em
//! qualquer zoom e na impressão) ou com a imagem escolhida.
use crate::error::{PdfError, Result};
use crate::types::{PathCmd, PdfRect, SignatureArt, VectorPath};
use mupdf::pdf::{PdfAnnotation, PdfDocument, PdfObject, PdfPage};
use mupdf::{Buffer, Font, Image, Matrix, PathWalker, Rect};
use std::path::Path;

/// Fonte manuscrita da assinatura digitada (Great Vibes, SIL OFL 1.1: ver `fonts/OFL.txt`).
const SCRIPT_FONT: &[u8] = include_bytes!("../fonts/GreatVibes-Regular.ttf");

/// Nome que marca os carimbos de assinatura criados aqui.
pub const STAMP_NAME: &str = "GeraSignature";

/// Imagem de assinatura maior que isto é recusada (fotos enormes travariam a gravação).
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// Junta os comandos de contorno de um glifo, já deslocado e em y para baixo.
struct Collect {
    cmds: Vec<PathCmd>,
    dx: f32,
}

impl PathWalker for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cmds.push(PathCmd::Move(x + self.dx, -y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cmds.push(PathCmd::Line(x + self.dx, -y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let dx = self.dx;
        self.cmds
            .push(PathCmd::Curve(x1 + dx, -y1, x2 + dx, -y2, x + dx, -y));
    }
    fn close(&mut self) {
        self.cmds.push(PathCmd::Close);
    }
}

fn points_mut(c: &mut PathCmd) -> Vec<(&mut f32, &mut f32)> {
    match c {
        PathCmd::Move(x, y) | PathCmd::Line(x, y) => vec![(x, y)],
        PathCmd::Curve(a, b, c, d, e, f) => vec![(a, b), (c, d), (e, f)],
        PathCmd::Close => vec![],
    }
}

/// Assinatura digitada: contornos (preenchidos) do texto na fonte manuscrita, normalizados para
/// altura 1 (y para baixo), e a proporção largura/altura. Caracteres sem glifo são ignorados.
pub fn typed(text: &str) -> Result<(Vec<VectorPath>, f32)> {
    let font = Font::from_bytes("GreatVibes", SCRIPT_FONT)?;
    let mut paths = Vec::new();
    let mut pen = 0.0;
    for ch in text.chars() {
        let gid = font.encode_character(ch as i32)?;
        if gid == 0 {
            continue; // sem glifo (emoji, CJK…)
        }
        if let Some(outline) = font.outline_glyph(gid)? {
            let mut c = Collect {
                cmds: Vec::new(),
                dx: pen,
            };
            outline.walk(&mut c)?;
            if !c.cmds.is_empty() {
                paths.push(VectorPath {
                    cmds: c.cmds,
                    fill: true,
                    width: 0.0,
                });
            }
        }
        pen += font.advance_glyph(gid)?;
    }
    // caixa da tinta, com uma pequena margem; depois tudo vai para 0..aspect × 0..1
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in &mut paths {
        for c in &mut p.cmds {
            for (x, y) in points_mut(c) {
                b = [b[0].min(*x), b[1].min(*y), b[2].max(*x), b[3].max(*y)];
            }
        }
    }
    if paths.is_empty() {
        return Ok((paths, 1.0));
    }
    let pad = (b[3] - b[1]) * 0.08;
    let (x0, y0) = (b[0] - pad, b[1] - pad);
    let h = b[3] - b[1] + 2.0 * pad;
    for p in &mut paths {
        for c in &mut p.cmds {
            for (x, y) in points_mut(c) {
                *x = (*x - x0) / h;
                *y = (*y - y0) / h;
            }
        }
    }
    Ok((paths, (b[2] - b[0] + 2.0 * pad) / h))
}

fn checked_image(path: &Path) -> Result<Image> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_IMAGE_BYTES {
        return Err(PdfError::InvalidAnnotation(
            "imagem grande demais para assinatura (máximo 20 MB)".into(),
        ));
    }
    let name = path
        .to_str()
        .ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))?;
    Image::from_file(name)
        .map_err(|_| PdfError::InvalidAnnotation("não foi possível ler a imagem".into()))
}

/// Proporção largura/altura da imagem (para a interface montar o retângulo).
pub fn image_aspect(path: &Path) -> Result<f32> {
    let img = checked_image(path)?;
    Ok(img.width() as f32 / img.height().max(1) as f32)
}

fn num(v: f32) -> String {
    format!("{:.3}", v)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// Operadores PDF dos caminhos (arte em 0..aspect × 0..1, y para baixo) numa caixa w × h.
fn vector_ops(paths: &[VectorPath], aspect: f32, w: f32, h: f32) -> String {
    let s = (w / aspect.max(0.01)).min(h);
    let mut ops = String::from("q 0 0 0 rg 0 0 0 RG 1 J 1 j\n");
    let pt = |x: f32, y: f32| format!("{} {}", num(x * s), num(h - y * s));
    for p in paths {
        for c in &p.cmds {
            match *c {
                PathCmd::Move(x, y) => ops.push_str(&format!("{} m\n", pt(x, y))),
                PathCmd::Line(x, y) => ops.push_str(&format!("{} l\n", pt(x, y))),
                PathCmd::Curve(a, b, c2, d, e, f) => {
                    ops.push_str(&format!("{} {} {} c\n", pt(a, b), pt(c2, d), pt(e, f)))
                }
                PathCmd::Close => ops.push_str("h\n"),
            }
        }
        if p.fill {
            ops.push_str("f\n");
        } else {
            ops.push_str(&format!("{} w S\n", num(p.width * s)));
        }
    }
    ops.push('Q');
    ops
}

/// Carimbo de assinatura com a aparência montada aqui. A arte é desenhada no espaço da tela
/// (largura × altura do retângulo como aparece) e a /Matrix contra-gira a rotação da página, para
/// ela ficar em pé; o leitor ajusta a caixa girada ao /Rect.
pub(crate) fn add(
    doc: &mut PdfDocument,
    page: &mut PdfPage,
    rect: PdfRect,
    art: &SignatureArt,
    initials: bool,
) -> Result<PdfAnnotation> {
    let rotation = page.rotation()?.rem_euclid(360);
    let (w, h) = (rect.width(), rect.height());
    let mut resources = doc.new_dict()?;
    let ops = match art {
        SignatureArt::Vector { paths, aspect } => vector_ops(paths, *aspect, w, h),
        SignatureArt::Image { path } => {
            let img = checked_image(path)?;
            let mut xobjects = doc.new_dict()?;
            xobjects.dict_put("I", doc.add_image(&img)?)?;
            resources.dict_put("XObject", xobjects)?;
            format!("q {} 0 0 {} 0 0 cm /I Do Q", num(w), num(h))
        }
    };
    let title = if initials { "Iniciais" } else { "Assinatura" };
    let annot = page.add_stamp_annotation(Rect::from(rect), title)?;
    let mut form = doc.new_dict()?;
    form.dict_put("Type", PdfObject::new_name("XObject")?)?;
    form.dict_put("Subtype", PdfObject::new_name("Form")?)?;
    form.dict_put(
        "BBox",
        doc.new_object_from_str(&format!("[0 0 {} {}]", num(w), num(h)))?,
    )?;
    // em pé na tela: gira a arte no sentido anti-horário pelo /Rotate da página
    let m = Matrix::new_rotate(rotation as f32);
    form.dict_put(
        "Matrix",
        doc.new_object_from_str(&format!(
            "[{} {} {} {} 0 0]",
            num(m.a),
            num(m.b),
            num(m.c),
            num(m.d)
        ))?,
    )?;
    form.dict_put("Resources", resources)?;
    let stream = doc.add_stream(&Buffer::from_bytes(ops.as_bytes())?, Some(&form), false)?;
    let mut ap = doc.new_dict()?;
    ap.dict_put("N", stream)?;
    let mut obj = annot.object();
    obj.dict_put("AP", ap)?;
    obj.dict_put("Name", PdfObject::new_name(STAMP_NAME)?)?;
    obj.dict_put("Subj", PdfObject::new_string(title)?)?;
    Ok(annot)
}
