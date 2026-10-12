//! Botões com ícone (como no Acrobat): o campo Imagem e a aba Opções dos botões (layout,
//! comportamento e ícone). O MuPDF não desenha /MK /I: a aparência desses botões é gerada aqui,
//! com o ícone ajustado proporcionalmente e o rótulo em Helvetica.
use crate::create_export::{oriented, UprightImage};
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::page_content::form_xobject;
use crate::stamp::literal;
use crate::text_fit::text_width;
use crate::types::PdfRect;
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Font;
use std::path::Path;

/// Layouts do Acrobat (/MK /TP).
pub const LAYOUTS: [&str; 7] = [
    "Somente rótulo",
    "Somente ícone",
    "Ícone em cima, rótulo embaixo",
    "Rótulo em cima, ícone embaixo",
    "Ícone à esquerda, rótulo à direita",
    "Rótulo à esquerda, ícone à direita",
    "Rótulo sobre o ícone",
];

/// Comportamento ao clicar (/H): (valor, nome).
pub const BEHAVIORS: [(&str, &str); 4] = [
    ("N", "Nenhum"),
    ("I", "Inverter"),
    ("O", "Contorno"),
    ("P", "Pressionar"),
];

/// Ícone (Form XObject) a partir de um arquivo de imagem, em pé (orientação EXIF).
pub(crate) fn icon_from_file(doc: &mut PdfDocument, path: &Path) -> Result<PdfObject> {
    let up = UprightImage::open(path)?;
    let image = doc.add_image(&up.img)?;
    let (w, h) = (up.width.max(1.0), up.height.max(1.0));
    let [a, b, c, d, e, f] = oriented(up.orientation, w, h);
    let ops = format!("q {a} {b} {c} {d} {e} {f} cm /Img Do Q");
    let mut res = doc.new_dict()?;
    let mut xo = doc.new_dict()?;
    xo.dict_put("Img", image)?;
    res.dict_put("XObject", xo)?;
    form_xobject(doc, ops.as_bytes(), res, [0.0, 0.0, w, h])
}

/// Marca dos botões cuja aparência o app desenhou (chave própria, ignorada pelos leitores).
const APP_MARK: &str = "GeraAP";

thread_local! {
    /// Helvetica do MuPDF, para medir os rótulos (carregada uma vez por thread).
    static HELVETICA: Option<Font> = Font::new("Helvetica").ok();
}

/// Operadores de cor de um array /MK (/BG ou /BC): cinza, RGB ou CMYK.
fn color_ops(arr: Option<PdfObject>, stroke: bool) -> Result<Option<String>> {
    let Some(arr) = arr.filter(|a| a.is_array().unwrap_or(false)) else {
        return Ok(None);
    };
    let n = arr.len()?;
    let vals: Vec<String> = (0..n as i32)
        .filter_map(|k| arr.get_array(k).ok().flatten())
        .map(|v| format!("{}", v.as_float().unwrap_or(0.0)))
        .collect();
    let op = match (n, stroke) {
        (1, false) => "g",
        (1, true) => "G",
        (3, false) => "rg",
        (3, true) => "RG",
        (4, false) => "k",
        (4, true) => "K",
        _ => return Ok(None),
    };
    Ok(Some(format!("{} {op}", vals.join(" "))))
}

/// Onde vão o ícone e o rótulo no botão `w`×`h`, pelo layout (com 2 pt de margem).
fn split(
    layout: i32,
    w: f32,
    h: f32,
    label_w: f32,
    label_h: f32,
) -> (Option<PdfRect>, Option<PdfRect>) {
    let a = PdfRect::new(2.0, 2.0, (w - 2.0).max(2.0), (h - 2.0).max(2.0));
    let lh = label_h.min(a.height());
    let lw = (label_w + 4.0).min(a.width() / 2.0);
    match layout {
        1 => (Some(a), None),
        2 => (
            Some(PdfRect::new(a.x0, a.y0 + lh, a.x1, a.y1)),
            Some(PdfRect::new(a.x0, a.y0, a.x1, a.y0 + lh)),
        ),
        3 => (
            Some(PdfRect::new(a.x0, a.y0, a.x1, a.y1 - lh)),
            Some(PdfRect::new(a.x0, a.y1 - lh, a.x1, a.y1)),
        ),
        4 => (
            Some(PdfRect::new(a.x0, a.y0, a.x1 - lw, a.y1)),
            Some(PdfRect::new(a.x1 - lw, a.y0, a.x1, a.y1)),
        ),
        5 => (
            Some(PdfRect::new(a.x0 + lw, a.y0, a.x1, a.y1)),
            Some(PdfRect::new(a.x0, a.y0, a.x0 + lw, a.y1)),
        ),
        6 => (Some(a), Some(a)),
        _ => (None, Some(a)),
    }
}

impl PdfDoc {
    /// A Helvetica dos campos (/AcroForm /DR /Font /Helv); sem ela, uma nova.
    pub(crate) fn helvetica(&mut self) -> Result<PdfObject> {
        use crate::pdf_obj::get;
        let found = get(&self.inner.catalog()?, "AcroForm")
            .and_then(|a| get(&a, "DR"))
            .and_then(|dr| get(&dr, "Font"))
            .and_then(|f| get(&f, "Helv"));
        if let Some(helv) = found {
            return Ok(helv);
        }
        let helv = self.inner.new_object_from_str(
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>",
        )?;
        Ok(self.inner.add_object(&helv)?)
    }

    /// Botão que o app desenha (com ícone ou layout diferente de "Somente rótulo")?
    /// Só os botões de ação que o app já desenhou (marca /GeraAP): os de outros programas
    /// guardam a aparência deles (estados, pressionado) até serem editados aqui.
    pub(crate) fn drawn_by_app(obj: &PdfObject) -> bool {
        crate::pdf_obj::get(obj, APP_MARK).is_some_and(|m| m.as_bool().unwrap_or(false))
    }

    /// Gera a aparência (/AP /N) do botão de ação: fundo, borda, ícone e rótulo, pelo layout.
    pub(crate) fn draw_button(&mut self, obj: &mut PdfObject) -> Result<()> {
        let mk = obj.get_dict("MK")?;
        let get = |k: &str| mk.as_ref().and_then(|m| m.get_dict(k).ok().flatten());
        // os lados do /Rect; com /MK /R 90 ou 270, o desenho em pé tem os lados trocados
        let (rw, rh) = crate::pdf_obj::rect_size(obj);
        let rotation = get("R")
            .and_then(|r| r.as_int().ok())
            .unwrap_or(0)
            .rem_euclid(360);
        let (w, h) = match rotation {
            90 | 270 => (rh, rw),
            _ => (rw, rh),
        };
        let layout = get("TP")
            .and_then(|t| t.as_int().ok())
            .unwrap_or(0)
            .clamp(0, 6);
        let label = get("CA")
            .and_then(|c| c.as_string().ok())
            .unwrap_or_default();
        let icon = get("I");
        let mut ops = String::from("q ");
        if let Some(bg) = color_ops(get("BG"), false)? {
            ops += &format!("{bg} 0 0 {w} {h} re f ");
        }
        if let Some(bc) = color_ops(get("BC"), true)? {
            ops += &format!("{bc} 1 w 0.5 0.5 {} {} re S ", w - 1.0, h - 1.0);
        }
        let size = match crate::prepare::da_size(obj).unwrap_or(0.0) {
            s if s > 0.0 => s,
            _ => (h * 0.5).clamp(4.0, 12.0),
        };
        let label_w = if label.is_empty() {
            0.0
        } else {
            HELVETICA
                .with(|f| f.as_ref().map(|f| text_width(f, &label, size)))
                .transpose()?
                .unwrap_or(label.chars().count() as f32 * size * 0.5)
        };
        let label_h = if label.is_empty() { 0.0 } else { size * 1.2 };
        let (icon_at, label_at) = split(layout, w, h, label_w, label_h);
        let mut res = self.inner.new_dict()?;
        if let (Some(icon), Some(r)) = (icon.as_ref(), icon_at) {
            let bbox = icon.get_dict("BBox")?;
            let num = |k: i32| {
                bbox.as_ref()
                    .and_then(|b| b.get_array(k).ok().flatten())
                    .and_then(|v| v.as_float().ok())
                    .unwrap_or(0.0)
            };
            let (iw, ih) = (
                (num(2) - num(0)).abs().max(1.0),
                (num(3) - num(1)).abs().max(1.0),
            );
            let s = (r.width() / iw).min(r.height() / ih);
            let x = r.x0 + (r.width() - iw * s) / 2.0;
            let y = r.y0 + (r.height() - ih * s) / 2.0;
            ops += &format!("q {s} 0 0 {s} {x} {y} cm /Icon Do Q ");
            let mut xo = self.inner.new_dict()?;
            xo.dict_put("Icon", icon.clone())?;
            res.dict_put("XObject", xo)?;
        }
        if let Some(r) = label_at.filter(|_| !label.is_empty()) {
            let x = r.x0 + (r.width() - label_w) / 2.0;
            let y = r.y0 + (r.height() - size) / 2.0 + size * 0.22;
            ops += &format!(
                "BT /Helv {size} Tf 0 g {x} {y} Td ({}) Tj ET ",
                literal(&label)
            );
            let mut fonts = self.inner.new_dict()?;
            fonts.dict_put("Helv", self.helvetica()?)?;
            res.dict_put("Font", fonts)?;
        }
        ops += "Q";
        let mut stream = form_xobject(&mut self.inner, ops.as_bytes(), res, [0.0, 0.0, w, h])?;
        let matrix = match rotation {
            90 => Some("[0 1 -1 0 0 0]"),
            180 => Some("[-1 0 0 -1 0 0]"),
            270 => Some("[0 -1 1 0 0 0]"),
            _ => None,
        };
        if let Some(m) = matrix {
            stream.dict_put("Matrix", self.inner.new_object_from_str(m)?)?;
        }
        let mut ap = self.inner.new_dict()?;
        ap.dict_put("N", stream)?;
        obj.dict_put("AP", ap)?;
        obj.dict_put(APP_MARK, PdfObject::new_bool(true))?;
        Ok(())
    }

    /// Campo Imagem (ou botão): a imagem do arquivo vira o ícone. Uma etapa do desfazer; um
    /// arquivo que não é imagem não muda nada.
    pub fn set_button_icon(&mut self, page: i32, id: i32, path: &Path) -> Result<()> {
        self.widget(page, id)?;
        // lida antes do instantâneo: imagem inválida não deixa etapa vazia no desfazer
        let icon = icon_from_file(&mut self.inner, path)?;
        self.with_snapshot(|d| {
            let w = d.widget(page, id)?;
            let mut obj = w.annotation().object();
            let mut mk = match obj.get_dict("MK")? {
                Some(mk) if mk.is_dict()? => mk,
                _ => {
                    obj.dict_put("MK", d.inner.new_dict()?)?;
                    obj.get_dict("MK")?.expect("acabou de ser posto")
                }
            };
            mk.dict_put("I", icon)?;
            let tp = mk
                .get_dict("TP")?
                .and_then(|t| t.as_int().ok())
                .unwrap_or(0);
            if tp == 0 {
                mk.dict_put("TP", PdfObject::new_int(1)?)?;
            }
            d.draw_button(&mut obj)
        })
    }
}
