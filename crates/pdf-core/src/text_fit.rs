//! Medida de texto com as fontes base do PDF, para a caixa de texto crescer como no Acrobat: as
//! mesmas regras que o MuPDF usa ao desenhar a caixa (quebra por palavras, linha = 1,2 × tamanho,
//! margem interna = 2 × borda).
use crate::error::Result;
use crate::types::PdfRect;
use mupdf::Font;

/// Altura de uma linha em relação ao tamanho da fonte (a mesma do MuPDF).
pub(crate) const LINE_HEIGHT: f32 = 1.2;

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
    // caixa estreitíssima (ou largura inválida, de outro programa): pelo menos um "eme" por linha,
    // senão a quebra de palavra viraria milhões de linhas
    let maxw = if maxw.is_finite() {
        maxw.max(size)
    } else {
        size
    }
    .max(1.0);
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
            if used > maxw {
                // palavra maior que a linha: quebra no meio, quantas vezes for preciso
                let extra = (used / maxw).ceil() - 1.0;
                lines += extra as usize;
                used -= extra * maxw;
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

/// Caixa justa do texto solto (Typewriter): largura da linha mais longa, uma altura de linha por
/// parágrafo, a partir do canto `at`.
pub(crate) fn snug(at: (f32, f32), text: &str, size: f32) -> Result<PdfRect> {
    let font = Font::new("Helvetica")?;
    let mut w: f32 = size; // nunca menor que um "eme" (texto vazio continua clicável)
    let mut lines = 0;
    for line in text.split('\n') {
        w = w.max(text_width(&font, line, size)?);
        lines += 1;
    }
    // folga para a métrica do MuPDF ao desenhar (não quebrar a última palavra)
    let w = w + 0.5 * size;
    let h = lines as f32 * LINE_HEIGHT * size + 0.2 * size;
    Ok(PdfRect::new(at.0, at.1, at.0 + w, at.1 + h))
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

    #[test]
    fn degenerate_widths_finish_fast() {
        let font = Font::new("Helvetica").unwrap();
        let long = "x".repeat(5000);
        let t = std::time::Instant::now();
        for w in [1e-6, 0.0, -50.0, f32::NAN] {
            let n = line_count(&font, &long, 12.0, w).unwrap();
            assert!(n >= 1);
        }
        let tiny = PdfRect::new(0.0, 0.0, 2.0, 10.0);
        assert!(fitted(tiny, &long, 12.0, 1.0).unwrap().height().is_finite());
        assert!(t.elapsed().as_millis() < 500, "{:?}", t.elapsed());
    }
}
