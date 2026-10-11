//! Carimbo desenhado pelo app: um PDF de 190×60 pt em memória, rasterizado a 300 dpi com fundo
//! transparente. O MuPDF só desenha carimbos de uma linha e corta nomes longos; a imagem dá a
//! moldura arredondada e as duas linhas do carimbo dinâmico, e o MuPDF a reajusta ao mover e
//! redimensionar.
use crate::error::{refused, Result};
use crate::text_fit::text_width;
use mupdf::{Colorspace, Document, Font, Image, ImageFormat, Matrix};
use std::path::Path;

/// Tamanho do desenho do carimbo, em pontos: também o tamanho padrão do carimbo na página
/// (exportado como `STAMP_WIDTH`/`STAMP_HEIGHT`).
pub const WIDTH: f32 = 190.0;
pub const HEIGHT: f32 = 60.0;
/// Margem lateral do texto dentro da moldura.
const PAD: f32 = 14.0;
/// Resolução da imagem (pontos → pixels).
const DPI: f32 = 300.0;

/// Carimbo personalizado: maior lado da imagem guardada (pixels), maior lado do carimbo na página
/// (pontos) e maior arquivo aceito como fonte ou como imagem guardada.
const CUSTOM_MAX_PX: f32 = 1500.0;
pub const CUSTOM_MAX_PT: f32 = 300.0;
const CUSTOM_MAX_BYTES: u64 = 100 * 1024 * 1024;

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| refused("o caminho do arquivo não é UTF-8"))
}

/// Tamanho natural do carimbo na página: o do original, com o maior lado até 300 pt.
pub fn natural_size(w: f32, h: f32) -> (f32, f32) {
    let k = (CUSTOM_MAX_PT / w.max(h).max(0.01)).min(1.0);
    (w * k, h * k)
}

/// Faz a imagem do carimbo personalizado (PNG com fundo transparente, 300 dpi, no máximo 1500
/// pixels no lado maior) a partir da página `page` de um PDF ou de uma imagem. Devolve o tamanho
/// natural do carimbo, em pontos.
pub fn stamp_image_from_file(src: &Path, page: i32, dest: &Path) -> Result<(f32, f32)> {
    if std::fs::metadata(src)?.len() > CUSTOM_MAX_BYTES {
        return Err(refused("o arquivo é grande demais para virar carimbo"));
    }
    let doc = open_source(src)?;
    if doc.needs_password()? {
        return Err(refused("o PDF está protegido por senha"));
    }
    if !(0..doc.page_count()?).contains(&page) {
        return Err(refused("a página não existe no arquivo"));
    }
    let p = doc.load_page(page)?;
    let b = p.bounds()?;
    let (w, h) = (b.width(), b.height());
    if !(w > 0.0 && h > 0.0) {
        return Err(refused("a página está vazia"));
    }
    let scale = (DPI / 72.0).min(CUSTOM_MAX_PX / w.max(h));
    let pix = p.to_pixmap(
        &Matrix::new_scale(scale, scale),
        &Colorspace::device_rgb(),
        true,
        false,
    )?;
    let mut png = Vec::new();
    pix.write_to(&mut png, ImageFormat::PNG)?;
    crate::doc::write_bytes_atomic(dest, &png)?;
    Ok(natural_size(w, h))
}

/// A imagem numa página de um PDF em memória (o MuPDF daqui não abre imagens como documento).
fn image_document(path: &Path) -> Result<Document> {
    let mut doc = mupdf::pdf::PdfDocument::new();
    crate::create_export::add_image_page(&mut doc, path)?;
    let mut bytes = Vec::new();
    doc.write_to(&mut bytes)?;
    Ok(Document::from_bytes(&bytes, "pdf")?)
}

/// O PDF, ou a imagem numa página.
fn open_source(src: &Path) -> Result<Document> {
    let is_pdf = std::fs::File::open(src)
        .and_then(|f| {
            use std::io::Read;
            let mut head = Vec::new();
            f.take(1024).read_to_end(&mut head)?;
            Ok(head.windows(5).any(|w| w == b"%PDF-"))
        })
        .unwrap_or(false);
    let opened = if is_pdf {
        Document::open(utf8(src)?).map_err(crate::PdfError::from)
    } else {
        image_document(src)
    };
    opened.map_err(|_| refused("o arquivo não é um PDF ou uma imagem que dê para abrir"))
}

/// Quantas páginas tem a fonte de um carimbo (imagem: 1).
pub fn stamp_source_pages(src: &Path) -> Result<i32> {
    let doc = open_source(src)?;
    if doc.needs_password()? {
        return Err(refused("o PDF está protegido por senha"));
    }
    Ok(doc.page_count()?)
}

/// Tamanho natural do carimbo guardado em `png` (a imagem tem 300 dpi).
pub fn stamp_image_size(png: &Path) -> Option<(f32, f32)> {
    let img = Image::from_file(png.to_str()?).ok()?;
    let k = 72.0 / DPI;
    Some(natural_size(
        img.width() as f32 * k,
        img.height() as f32 * k,
    ))
}

/// A imagem guardada pronta para a página: contra-girada como a dos carimbos do catálogo.
pub(crate) fn custom_image(png: &Path, page_rotation: i32) -> Result<Image> {
    if std::fs::metadata(png)?.len() > CUSTOM_MAX_BYTES {
        return Err(refused("a imagem do carimbo é grande demais"));
    }
    let doc = image_document(png).map_err(|_| refused("a imagem do carimbo não abre"))?;
    let page = doc.load_page(0)?;
    let px = Image::from_file(utf8(png)?)?.width() as f32;
    let scale = px / page.bounds()?.width().max(0.01);
    let mut ctm = Matrix::new_scale(scale, scale);
    ctm.concat(Matrix::new_rotate(-(page_rotation as f32)));
    let pix = page.to_pixmap(&ctm, &Colorspace::device_rgb(), true, false)?;
    Ok(Image::from_pixmap(&pix)?)
}

/// Texto num literal PDF em WinAnsi (os acentos do português cabem nele).
pub(crate) fn literal(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '(' | ')' | '\\' => format!("\\{c}"),
            c if (c as u32) < 128 => c.to_string(),
            c if (c as u32) < 256 => format!("\\{:03o}", c as u32),
            c => winansi_extra(c).map_or_else(|| "?".into(), |b| format!("\\{b:03o}")),
        })
        .collect()
}

/// Código WinAnsi (0x80–0x9F) dos caracteres tipográficos fora do Latin-1.
fn winansi_extra(c: char) -> Option<u8> {
    Some(match c {
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '•' => 0x95,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        'Œ' => 0x8C,
        'œ' => 0x9C,
        'Š' => 0x8A,
        'š' => 0x9A,
        'Ž' => 0x8E,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        _ => return None,
    })
}

/// Retângulo de cantos arredondados (raio `r`) como operadores de caminho PDF.
fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> String {
    let k = r * 0.5523; // aproximação do quarto de círculo por Bézier
    format!(
        "{a} {y0} m {b} {y0} l {b2} {y0} {x1} {c2} {x1} {c} c {x1} {d} l {x1} {d2} {b2} {y1} {b} {y1} c \
         {a} {y1} l {a2} {y1} {x0} {d2} {x0} {d} c {x0} {c} l {x0} {c2} {a2} {y0} {a} {y0} c h",
        a = x0 + r,
        b = x1 - r,
        c = y0 + r,
        d = y1 - r,
        a2 = x0 + r - k,
        b2 = x1 - r + k,
        c2 = y0 + r - k,
        d2 = y1 - r + k,
    )
}

/// Desenha o carimbo: título grande e, nos dinâmicos, a linha de baixo (nome, data e hora).
/// `page_rotation` (/Rotate da página, em graus): a imagem vai para o espaço sem giro do /Rect, e
/// a página a gira na tela; por isso ela é desenhada contra-girada, para o carimbo ficar em pé.
pub(crate) fn image(
    title: &str,
    subtitle: Option<&str>,
    [r, g, b]: [f32; 3],
    page_rotation: i32,
) -> Result<Image> {
    let font = Font::new("Times-Bold")?;
    let mut ops = format!(
        "q {r} {g} {b} RG {r} {g} {b} rg 2.5 w {} S\n",
        rounded_rect(3.0, 3.0, WIDTH - 3.0, HEIGHT - 3.0, 9.0)
    );
    let room = WIDTH - 2.0 * PAD;
    let mut line = |text: &str, max: f32, baseline: f32| -> Result<()> {
        let size = max.min(room / text_width(&font, text, 1.0)?.max(0.01));
        let x = (WIDTH - text_width(&font, text, size)?) / 2.0;
        ops.push_str(&format!(
            "BT /F1 {size} Tf {x} {baseline} Td ({}) Tj ET\n",
            literal(text)
        ));
        Ok(())
    };
    match subtitle {
        Some(sub) => {
            line(title, 24.0, 29.0)?;
            line(sub, 10.5, 12.0)?;
        }
        None => line(title, 28.0, 21.0)?,
    }
    ops.push('Q');
    let pdf = format!(
        "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
         2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 {WIDTH} {HEIGHT}]\
         /Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n\
         4 0 obj<</Length {}>>stream\n{ops}\nendstream endobj\n\
         5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Times-Bold/Encoding/WinAnsiEncoding>>endobj\n\
         trailer<</Root 1 0 R>>\n",
        ops.len()
    );
    let page = Document::from_bytes(pdf.as_bytes(), "pdf")?.load_page(0)?;
    let scale = DPI / 72.0;
    let mut ctm = Matrix::new_scale(scale, scale);
    ctm.concat(Matrix::new_rotate(-(page_rotation as f32)));
    let pix = page.to_pixmap(&ctm, &Colorspace::device_rgb(), true, false)?;
    Ok(Image::from_pixmap(&pix)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_pdf_delimiters_are_escaped() {
        assert_eq!(literal("NÃO (1)"), "N\\303O \\(1\\)");
    }

    #[test]
    fn typographic_quotes_dashes_and_euro_use_winansi() {
        assert_eq!(
            literal("D’Ávila – “ok” …€"),
            "D\\222\\301vila \\226 \\223ok\\224 \\205\\200"
        );
    }

    #[test]
    fn long_text_is_shrunk_to_fit() {
        let font = Font::new("Times-Bold").unwrap();
        let long = "Maria da Silva Pereira de Albuquerque, 09/10/2026 14:30";
        let size = 10.5f32.min((WIDTH - 2.0 * PAD) / text_width(&font, long, 1.0).unwrap());
        assert!(text_width(&font, long, size).unwrap() <= WIDTH - 2.0 * PAD + 0.01);
    }
}
