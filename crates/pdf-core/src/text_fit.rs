//! Medida de texto com as fontes base do PDF, para a caixa de texto crescer como no Acrobat: as
//! mesmas regras que o MuPDF usa ao desenhar a caixa (quebra por palavras, linha = 1,2 × tamanho,
//! margem interna = 2 × borda).
use crate::error::Result;
use crate::types::PdfRect;
use mupdf::Font;

/// Altura de uma linha em relação ao tamanho da fonte (a mesma do MuPDF).
const LINE_HEIGHT: f32 = 1.2;

/// Largura do texto em pontos, na fonte e no tamanho dados.
pub(crate) fn text_width(font: &Font, s: &str, size: f32) -> Result<f32> {
    let mut w = 0.0;
    for c in s.chars() {
        w += font.advance_glyph(font.encode_character(c as i32)?)?;
    }
    Ok(w * size)
}

/// Quantas linhas `text` ocupa com largura `maxw`: cada parágrafo quebra entre palavras; uma
/// palavra maior que a linha quebra no meio.
fn line_count(font: &Font, text: &str, size: f32, maxw: f32) -> Result<usize> {
    let mut lines = 0;
    for paragraph in text.split('\n') {
        lines += 1;
        let mut used = 0.0;
        for (n, word) in paragraph.split(' ').enumerate() {
            let piece = if n == 0 {
                word.to_owned()
            } else {
                format!(" {word}")
            };
            let w = text_width(font, &piece, size)?;
            if used > 0.0 && used + w > maxw {
                lines += 1;
                used = text_width(font, word, size)?;
            } else {
                used += w;
            }
            while used > maxw && maxw > 0.0 {
                lines += 1;
                used -= maxw;
            }
        }
    }
    Ok(lines)
}

/// Retângulo da caixa de texto crescido para baixo até caber o texto (nunca encolhe).
pub(crate) fn fitted(rect: PdfRect, text: &str, size: f32, border: f32) -> Result<PdfRect> {
    let font = Font::new("Helvetica")?;
    let pad = border * 2.0;
    let lines = line_count(&font, text, size, rect.width() - 2.0 * pad)?;
    let need = 2.0 * pad + lines as f32 * LINE_HEIGHT * size + 0.2 * size;
    Ok(if rect.height() + 0.01 < need {
        PdfRect::new(rect.x0, rect.y0, rect.x1, rect.y0 + need)
    } else {
        rect
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_paragraphs_and_wrapping() {
        let font = Font::new("Helvetica").unwrap();
        assert_eq!(line_count(&font, "a\nb\nc", 12.0, 200.0).unwrap(), 3);
        let long = "palavra ".repeat(20);
        assert!(line_count(&font, long.trim(), 12.0, 100.0).unwrap() > 3);
        assert_eq!(line_count(&font, "", 12.0, 100.0).unwrap(), 1);
    }

    #[test]
    fn fitted_never_shrinks() {
        let big = PdfRect::new(0.0, 0.0, 200.0, 300.0);
        assert_eq!(fitted(big, "oi", 12.0, 1.0).unwrap(), big);
    }
}
