//! Carimbo desenhado pelo app: um PDF de 190×60 pt em memória, rasterizado a 300 dpi com fundo
//! transparente. O MuPDF só desenha carimbos de uma linha e corta nomes longos; a imagem dá a
//! moldura arredondada e as duas linhas do carimbo dinâmico, e o MuPDF a reajusta ao mover e
//! redimensionar.
use crate::error::Result;
use mupdf::{Colorspace, Document, Font, Image, Matrix};

/// Tamanho do desenho do carimbo, em pontos (a proporção padrão do carimbo na página).
pub const WIDTH: f32 = 190.0;
pub const HEIGHT: f32 = 60.0;
/// Margem lateral do texto dentro da moldura.
const PAD: f32 = 14.0;
/// Resolução da imagem (pontos → pixels).
const DPI: f32 = 300.0;

/// Texto num literal PDF em WinAnsi (os acentos do português cabem nele).
fn literal(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '(' | ')' | '\\' => format!("\\{c}"),
            c if (c as u32) < 128 => c.to_string(),
            c if (c as u32) < 256 => format!("\\{:03o}", c as u32),
            _ => "?".into(),
        })
        .collect()
}

/// Largura do texto em pontos, na fonte e no tamanho dados.
fn text_width(font: &Font, s: &str, size: f32) -> Result<f32> {
    let mut w = 0.0;
    for c in s.chars() {
        w += font.advance_glyph(font.encode_character(c as i32)?)?;
    }
    Ok(w * size)
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
pub(crate) fn image(title: &str, subtitle: Option<&str>, [r, g, b]: [f32; 3]) -> Result<Image> {
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
    let pix = page.to_pixmap(
        &Matrix::new_scale(scale, scale),
        &Colorspace::device_rgb(),
        true,
        false,
    )?;
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
    fn long_text_is_shrunk_to_fit() {
        let font = Font::new("Times-Bold").unwrap();
        let long = "Maria da Silva Pereira de Albuquerque, 09/10/2026 14:30";
        let size = 10.5f32.min((WIDTH - 2.0 * PAD) / text_width(&font, long, 1.0).unwrap());
        assert!(text_width(&font, long, size).unwrap() <= WIDTH - 2.0 * PAD + 0.01);
    }
}
