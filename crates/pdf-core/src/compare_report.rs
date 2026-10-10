//! O PDF do resultado da comparação, como o relatório do Acrobat: uma página de resumo e as páginas
//! lado a lado (antiga à esquerda, nova à direita) com as alterações marcadas. Cada alteração é um
//! destaque travado (aparece no painel Comentários); do outro lado, a marca vai no conteúdo.
use crate::compare::{compare, Change, ChangeKind, CompareOptions, CompareStage, Comparison};
use crate::doc::{open_authenticated, rgb, write_atomic};
use crate::error::{refused, Result};
use crate::page_content::{form_xobject, matrix_ops, mul, num, page_parts, rgb_ops};
use crate::pages::same_file;
use crate::stamp::literal;
use crate::text_fit::text_width;
use crate::{PdfError, PdfRect};
use mupdf::document::Location;
use mupdf::link::LinkDestination;
use mupdf::pdf::{AnnotationFlags, Encryption, PdfDocument, PdfObject, PdfWriteOptions};
use mupdf::{Buffer, Font, Matrix, Outline, Rect, Size};
use std::path::Path;

/// Margem em volta das páginas, espaço entre elas e faixa do título no alto.
const MARGIN: f32 = 18.0;
const GAP: f32 = 18.0;
const BAND: f32 = 30.0;
/// Autor das anotações do resultado (a linha Comparação navega por elas).
pub const COMPARE_AUTHOR: &str = "Comparação";
/// Largura do acento onde o texto entrou ou saiu.
const CARET: f32 = 1.5;
/// Resumo em A4.
const SUMMARY: (f32, f32) = (595.0, 842.0);
/// Nomes de arquivo mais longos que isto são cortados nos títulos.
const NAME_CHARS: usize = 60;

/// Compara e grava o resultado em `dest`.
pub fn compare_files(
    opts: &CompareOptions,
    dest: &Path,
    progress: &mut dyn FnMut(CompareStage) -> bool,
) -> Result<Comparison> {
    refuse_inputs_as_dest(opts, dest)?;
    let cmp = compare(opts, progress)?;
    write_comparison(opts, &cmp, dest, progress)?;
    Ok(cmp)
}

fn refuse_inputs_as_dest(opts: &CompareOptions, dest: &Path) -> Result<()> {
    if same_file(dest, &opts.old.path) || same_file(dest, &opts.new.path) {
        return Err(refused(
            "o resultado não pode ser gravado por cima de um dos arquivos comparados",
        ));
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    match name.char_indices().nth(NAME_CHARS) {
        Some((cut, _)) => format!("{}…", &name[..cut]),
        None => name,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Texto numa posição (Helvetica `/F1` ou negrito `/F2`), na cor `gray`.
fn text_at(font: &str, size: f32, gray: f32, x: f32, y: f32, s: &str) -> String {
    format!(
        "q BT /{font} {} Tf {} g {} {} Td ({}) Tj ET Q\n",
        num(size),
        num(gray),
        num(x),
        num(y),
        literal(s)
    )
}

fn rect_ops(x: f32, y: f32, w: f32, h: f32) -> String {
    format!("{} {} {} {} re", num(x), num(y), num(w), num(h))
}

/// Retângulo preenchido com `color` (cada um define a própria cor: nada vaza).
fn fill_rect(color: [f32; 3], x: f32, y: f32, w: f32, h: f32) -> String {
    format!(
        "{} {} f\n",
        rgb_ops(color.map(|c| (c * 255.0).round() as u8)),
        rect_ops(x, y, w, h)
    )
}

/// Contorno de retângulo com `color` e espessura `width`.
fn stroke_rect(color: [f32; 3], width: f32, x: f32, y: f32, w: f32, h: f32) -> String {
    let [r, g, b] = color.map(num);
    format!(
        "q {r} {g} {b} RG {} w {} S Q\n",
        num(width),
        rect_ops(x, y, w, h)
    )
}

/// Página nova com os recursos e o conteúdo.
fn content_page(
    out: &mut PdfDocument,
    size: Size,
    resources: PdfObject,
    ops: &str,
) -> Result<mupdf::pdf::PdfPage> {
    let page = out.new_page(size)?;
    let mut obj = page.object();
    obj.dict_put("Resources", resources)?;
    let contents = out.add_stream(&Buffer::from_bytes(ops.as_bytes())?, None, false)?;
    obj.dict_put("Contents", contents)?;
    Ok(page)
}

/// Um lado do par: a página de origem, o tamanho na tela e onde fica no resultado.
struct Placed {
    /// Canto superior esquerdo no resultado (coordenadas da tela, origem no alto).
    ox: f32,
    oy: f32,
    /// Origem da caixa da página de origem na tela.
    bx: f32,
    by: f32,
    w: f32,
    h: f32,
}

impl Placed {
    /// Da tela da página de origem para a tela do resultado.
    fn rect(&self, r: &PdfRect) -> PdfRect {
        r.translated(self.ox - self.bx, self.oy - self.by)
    }
}

/// Retângulo da tela do resultado (origem no alto) em coordenadas do PDF (origem embaixo).
fn pdf_box(r: &PdfRect, height: f32) -> (f32, f32, f32, f32) {
    (r.x0, height - r.y1, r.x1 - r.x0, r.y1 - r.y0)
}

/// Grava o PDF do resultado. Cancelar (`progress` falso) não deixa arquivo.
pub fn write_comparison(
    opts: &CompareOptions,
    cmp: &Comparison,
    dest: &Path,
    progress: &mut dyn FnMut(CompareStage) -> bool,
) -> Result<()> {
    refuse_inputs_as_dest(opts, dest)?;
    let old = open_authenticated(&opts.old.path, opts.old.password.as_deref())?;
    let new = open_authenticated(&opts.new.path, opts.new.password.as_deref())?;
    let (old_name, new_name) = (file_name(&opts.old.path), file_name(&opts.new.path));
    let mut out = PdfDocument::new();
    let helv = out.add_object(&out.new_object_from_str(
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>",
    )?)?;
    let bold = out.add_object(&out.new_object_from_str(
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica-Bold/Encoding/WinAnsiEncoding>>",
    )?)?;
    let multiply = out.add_object(&out.new_object_from_str("<</Type/ExtGState/BM/Multiply>>")?)?;
    let metrics = Font::new("Helvetica")?;
    let mut old_map = out.new_graft_map()?;
    let mut new_map = out.new_graft_map()?;

    let mut by_pair: Vec<Vec<&Change>> = vec![Vec::new(); cmp.pairs.len()];
    for c in &cmp.changes {
        by_pair[c.pair].push(c);
    }

    let link = summary_page(&mut out, cmp, &old_name, &new_name, &helv, &bold, &metrics)?;
    let mut outline = vec![outline_to(0, "Resumo")];
    let total = cmp.pairs.len();
    for (k, &(op, np)) in cmp.pairs.iter().enumerate() {
        if !progress(CompareStage::Writing(k + 1, total)) {
            return Err(PdfError::Cancelled);
        }
        // tamanho de cada lado na tela; o lado vazio fica do tamanho do outro
        let size = |doc: &PdfDocument, p: Option<i32>| -> Result<Option<Rect>> {
            p.map(|p| Ok(doc.load_page(p)?.bounds()?)).transpose()
        };
        let (ob, nb) = (size(&old, op)?, size(&new, np)?);
        let (lw, lh, rw, rh) = {
            let l = ob.or(nb).expect("um par tem ao menos uma página");
            let r = nb.or(ob).expect("um par tem ao menos uma página");
            (l.width(), l.height(), r.width(), r.height())
        };
        let width = MARGIN + lw + GAP + rw + MARGIN;
        let height = BAND + lh.max(rh) + MARGIN;
        let side = |ox: f32, b: Option<Rect>, w: f32, h: f32| Placed {
            ox,
            oy: BAND,
            bx: b.map_or(0.0, |b| b.x0),
            by: b.map_or(0.0, |b| b.y0),
            w,
            h,
        };
        let left = side(MARGIN, ob, lw, lh);
        let right = side(MARGIN + lw + GAP, nb, rw, rh);

        let mut ops = String::new();
        let mut xobjects = out.new_dict()?;
        for (name, doc, map, page, at, label, file, missing) in [
            (
                "P0",
                &old,
                &mut old_map,
                op,
                &left,
                "Antigo",
                &old_name,
                "Página inserida",
            ),
            (
                "P1",
                &new,
                &mut new_map,
                np,
                &right,
                "Novo",
                &new_name,
                "Página excluída",
            ),
        ] {
            let (x, y, w, h) = (at.ox, height - at.oy - at.h, at.w, at.h);
            match page {
                Some(p) => {
                    let src = doc.find_page(p)?;
                    let (content, res, bbox) = page_parts(map, &out, &src)?;
                    xobjects.dict_put(name, form_xobject(&mut out, &content, res, bbox)?)?;
                    // da página de origem (espaço do PDF) para a tela dela, e daí para o resultado
                    let to_out =
                        Matrix::new(1.0, 0.0, 0.0, -1.0, at.ox - at.bx, height - at.oy + at.by);
                    let m = mul(&src.page_ctm()?, &to_out);
                    ops.push_str(&format!(
                        "q {} {} {} {} re W n {} cm /{name} Do Q\n",
                        num(x),
                        num(y),
                        num(w),
                        num(h),
                        matrix_ops(&m)
                    ));
                    ops.push_str(&text_at(
                        "F1",
                        9.0,
                        0.3,
                        x,
                        height - 20.0,
                        &format!("{label} — página {} — {file}", p + 1),
                    ));
                }
                None => {
                    ops.push_str(&fill_rect([0.95; 3], x, y, w, h));
                    let tw = text_width(&metrics, missing, 14.0)?;
                    ops.push_str(&text_at(
                        "F1",
                        14.0,
                        0.45,
                        x + (w - tw) / 2.0,
                        y + h / 2.0,
                        missing,
                    ));
                    ops.push_str(&text_at("F1", 9.0, 0.3, x, height - 20.0, missing));
                }
            }
            ops.push_str(&stroke_rect([0.75; 3], 0.5, x, y, w, h));
        }

        // as marcas do lado sem anotação, por cima do conteúdo
        let mut annots: Vec<(&Change, Vec<PdfRect>)> = Vec::new();
        for c in &by_pair[k] {
            let (color, tint) = (c.kind.color(), c.kind.tint());
            let (old_r, new_r) = (
                c.old_rects.iter().map(|r| left.rect(r)).collect::<Vec<_>>(),
                c.new_rects
                    .iter()
                    .map(|r| right.rect(r))
                    .collect::<Vec<_>>(),
            );
            let (marked, other) = if new_r.is_empty() {
                (old_r, new_r)
            } else {
                (new_r, old_r)
            };
            ops.push_str("q /GM gs\n");
            for r in &other {
                let (x, y, w, h) = pdf_box(r, height);
                ops.push_str(&fill_rect(tint, x, y, w, h));
            }
            ops.push_str("Q\n");
            if c.kind == ChangeKind::Image {
                for r in marked.iter().chain(&other) {
                    let (x, y, w, h) = pdf_box(r, height);
                    ops.push_str(&stroke_rect(color, 2.0, x, y, w, h));
                }
            }
            if let Some(a) = &c.anchor {
                // o acento fica no lado sem palavras
                let at = if c.new_rects.is_empty() {
                    &right
                } else {
                    &left
                };
                let (x, y, _, h) = pdf_box(&at.rect(a), height);
                ops.push_str(&fill_rect(color, x - CARET / 2.0, y, CARET, h));
            }
            annots.push((c, marked));
        }

        let mut fonts = out.new_dict()?;
        fonts.dict_put("F1", helv.clone())?;
        let mut gs = out.new_dict()?;
        gs.dict_put("GM", multiply.clone())?;
        let mut res = out.new_dict()?;
        res.dict_put("Font", fonts)?;
        res.dict_put("XObject", xobjects)?;
        res.dict_put("ExtGState", gs)?;
        let mut page = content_page(&mut out, Size { width, height }, res, &ops)?;
        for (c, rects) in annots {
            let quads = rects
                .iter()
                .map(|r| Rect::from(*r).quad())
                .collect::<Vec<_>>();
            let mut a = page.add_highlight_annotation(quads)?;
            a.set_color(rgb(c.kind.tint()))?;
            a.set_author(COMPARE_AUTHOR)?;
            a.set_contents(&c.description)?;
            a.set_flags(AnnotationFlags::IS_PRINT | AnnotationFlags::IS_LOCKED)?;
            a.object()
                .dict_put("Subj", PdfObject::new_string(c.kind.label())?)?;
            a.update()?;
        }
        let title = match (op, np) {
            (Some(o), Some(n)) => format!("Antigo p. {} ↔ Novo p. {}", o + 1, n + 1),
            (Some(o), None) => format!("Página excluída (antigo p. {})", o + 1),
            (None, Some(n)) => format!("Página inserida (novo p. {})", n + 1),
            (None, None) => unreachable!("um par tem ao menos uma página"),
        };
        outline.push(outline_to(k as u32 + 1, &title));
    }
    out.set_outlines(&outline)?;
    if let Some((rect, target)) = link {
        link_summary(&mut out, rect, target)?;
    }

    let mut info = out.new_dict()?;
    info.dict_put(
        "Title",
        PdfObject::new_string(&format!("Comparação de {old_name} e {new_name}"))?,
    )?;
    info.dict_put("Creator", PdfObject::new_string("Gera PDF Editor")?)?;
    info.dict_put("GeraCompare", PdfObject::new_bool(true))?;
    let info = out.add_object(&info)?;
    out.trailer()?.dict_put("Info", info)?;
    // comprimido: o conteúdo das páginas copiadas entra descomprimido no formulário
    let mut w = PdfWriteOptions::default();
    w.set_encryption(Encryption::None).set_compress(true);
    write_atomic(&out, dest, w)
}

fn outline_to(page: u32, title: &str) -> Outline {
    Outline {
        title: title.into(),
        uri: None,
        dest: Some(LinkDestination {
            loc: Location {
                chapter: 0,
                page_in_chapter: page,
                page_number: page,
            },
            kind: mupdf::DestinationKind::Fit,
        }),
        down: Vec::new(),
    }
}

/// A página 1: os arquivos, a contagem por tipo com a legenda e o link para a primeira alteração.
fn summary_page(
    out: &mut PdfDocument,
    cmp: &Comparison,
    old_name: &str,
    new_name: &str,
    helv: &PdfObject,
    bold: &PdfObject,
    metrics: &Font,
) -> Result<Option<([f32; 4], i32)>> {
    let (w, h) = SUMMARY;
    let x = 56.0;
    let mut y = h - 72.0;
    let mut ops = text_at("F2", 20.0, 0.0, x, y, "Resultados da comparação");
    y -= 36.0;
    let pages = |n: i32| plural(n as usize, "página comparada", "páginas comparadas");
    for line in [
        format!("Arquivo antigo: {old_name} ({})", pages(cmp.old_pages)),
        format!("Arquivo novo: {new_name} ({})", pages(cmp.new_pages)),
        format!(
            "Comparado em: {}",
            chrono::Local::now().format("%d/%m/%Y %H:%M")
        ),
    ] {
        ops.push_str(&text_at("F1", 11.0, 0.0, x, y, &line));
        y -= 18.0;
    }
    y -= 24.0;
    let total = cmp.changes.len();
    let mut link = None;
    if total == 0 {
        ops.push_str(&text_at(
            "F2",
            16.0,
            0.0,
            x,
            y,
            "Não foram encontradas diferenças.",
        ));
    } else {
        ops.push_str(&text_at(
            "F2",
            16.0,
            0.0,
            x,
            y,
            &plural(total, "alteração", "alterações"),
        ));
        y -= 30.0;
        for kind in ChangeKind::ALL {
            let n = cmp.changes.iter().filter(|c| c.kind == kind).count();
            ops.push_str(&fill_rect(kind.tint(), x, y - 2.0, 12.0, 12.0));
            ops.push_str(&text_at(
                "F1",
                11.0,
                0.0,
                x + 20.0,
                y,
                &format!("{}: {n}", kind.label()),
            ));
            y -= 20.0;
        }
        let inserted = cmp.pairs.iter().filter(|p| p.0.is_none()).count();
        let deleted = cmp.pairs.iter().filter(|p| p.1.is_none()).count();
        y -= 8.0;
        ops.push_str(&text_at(
            "F1",
            11.0,
            0.0,
            x,
            y,
            &format!("Páginas inseridas: {inserted}    Páginas excluídas: {deleted}"),
        ));
        y -= 32.0;
        let label = "Ir para a primeira alteração";
        let lw = text_width(metrics, label, 11.0)?;
        ops.push_str(&format!(
            "BT /F1 11 Tf 0.1 0.35 0.8 rg {} {} Td ({}) Tj ET q 0.1 0.35 0.8 RG 0.75 w {} {} m {} {} l S Q\n",
            num(x),
            num(y),
            literal(label),
            num(x),
            num(y - 2.0),
            num(x + lw),
            num(y - 2.0)
        ));
        let first = cmp.changes[0].pair as i32 + 1;
        link = Some(([x, y - 4.0, x + lw, y + 12.0], first));
    }
    ops.push_str(&text_at(
        "F1",
        9.0,
        0.35,
        x,
        56.0,
        "Antigo à esquerda, novo à direita. A lista das alterações está no painel Comentários.",
    ));
    let mut fonts = out.new_dict()?;
    fonts.dict_put("F1", helv.clone())?;
    fonts.dict_put("F2", bold.clone())?;
    let mut res = out.new_dict()?;
    res.dict_put("Font", fonts)?;
    content_page(
        out,
        Size {
            width: w,
            height: h,
        },
        res,
        &ops,
    )?;
    Ok(link)
}

/// Liga a área `rect` do resumo à página `target` (que só existe depois das páginas lado a lado).
fn link_summary(out: &mut PdfDocument, rect: [f32; 4], target: i32) -> Result<()> {
    let [x0, y0, x1, y1] = rect.map(num);
    let mut annot = out.new_object_from_str(&format!(
        "<</Type/Annot/Subtype/Link/Rect[{x0} {y0} {x1} {y1}]/Border[0 0 0]>>"
    ))?;
    let mut dest = out.new_array()?;
    dest.array_push(out.find_page(target)?)?;
    dest.array_push(PdfObject::new_name("Fit")?)?;
    annot.dict_put("Dest", dest)?;
    let annot = out.add_object(&annot)?;
    let mut list = out.new_array()?;
    list.array_push(annot)?;
    out.find_page(0)?.dict_put("Annots", list)?;
    Ok(())
}
