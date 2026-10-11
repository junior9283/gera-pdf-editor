//! Recortar, Copiar e Colar de comentários, texto e imagens (como no Acrobat).

use pdf_core::PdfRect;

/// Texto colado vira caixa de texto com no máximo isso.
pub const MAX_PASTED_CHARS: usize = 10_000;
/// Imagem colada: teto de pixels (40 Mpx).
pub const MAX_PASTED_PIXELS: usize = 40_000_000;

/// O que Colar faz.
#[derive(Debug, Clone, PartialEq)]
pub enum Paste {
    /// O comentário copiado no app: cópia em `to_page`, deslocada.
    Annot {
        page: i32,
        id: i32,
        to_page: i32,
        dx: f32,
        dy: f32,
    },
    /// Texto de fora: uma caixa de texto.
    Text(String),
    /// Sem texto: ler a área de transferência do sistema (pode ter imagem).
    FromSystem,
}

/// Comentário copiado ou recortado no app.
#[derive(Debug, Default)]
pub struct AnnotClipboard {
    copied: Option<Copied>,
}

#[derive(Debug)]
struct Copied {
    page: i32,
    id: i32,
    /// O que foi para a área de transferência do sistema: se ainda estiver lá, Colar traz o
    /// comentário; se outro programa copiou depois, vale o texto novo.
    marker: String,
    /// Colagens na mesma página (cada uma desce mais 10 pt).
    pasted: u32,
}

impl AnnotClipboard {
    /// Copia o comentário; devolve o texto para a área de transferência do sistema.
    pub fn copy(&mut self, page: i32, id: i32, contents: &str) -> String {
        let marker = if contents.trim().is_empty() {
            format!("[Comentário {id}]")
        } else {
            contents.to_owned()
        };
        self.copied = Some(Copied {
            page,
            id,
            marker: marker.clone(),
            pasted: 0,
        });
        marker
    }

    /// Decide o que colar com `system_text` (o texto da área do sistema) na página `to_page`.
    pub fn paste(&mut self, system_text: Option<&str>, to_page: i32) -> Paste {
        if let Some(c) = self
            .copied
            .as_mut()
            .filter(|c| system_text == Some(c.marker.as_str()))
        {
            let step = if c.page == to_page {
                c.pasted += 1;
                10.0 * c.pasted as f32
            } else {
                0.0
            };
            return Paste::Annot {
                page: c.page,
                id: c.id,
                to_page,
                dx: step,
                dy: step,
            };
        }
        match system_text.filter(|t| !t.trim().is_empty()) {
            Some(t) => Paste::Text(t.chars().take(MAX_PASTED_CHARS).collect()),
            None => Paste::FromSystem,
        }
    }
}

/// Imagem da área de transferência utilizável: tamanho positivo, dentro do teto e com os bytes
/// RGBA completos.
pub fn image_fits(width: usize, height: usize, bytes: usize) -> bool {
    let pixels = width.checked_mul(height);
    pixels.is_some_and(|p| p > 0 && p <= MAX_PASTED_PIXELS && bytes >= p * 4)
}

/// Retângulo de `size` centrado em `at`, dentro da página `page` (pontos do MuPDF).
pub fn centered(at: (f32, f32), size: (f32, f32), page: (f32, f32)) -> PdfRect {
    let x0 = (at.0 - size.0 / 2.0).clamp(0.0, (page.0 - size.0).max(0.0));
    let y0 = (at.1 - size.1 / 2.0).clamp(0.0, (page.1 - size.1).max(0.0));
    PdfRect::new(x0, y0, x0 + size.0, y0 + size.1)
}

/// O que havia na área de transferência do sistema.
#[derive(Debug)]
pub enum SystemClip {
    Text(String),
    /// Imagem gravada em PNG em `path` (pasta temporária, só o dono lê).
    Image {
        path: std::path::PathBuf,
        width: u32,
        height: u32,
    },
    Refused(String),
    Empty,
}

/// Lê a área de transferência (imagem primeiro, depois texto). Roda fora da interface.
pub fn read_system(dir: &std::path::Path) -> SystemClip {
    let Ok(mut board) = arboard::Clipboard::new() else {
        return SystemClip::Refused("Não foi possível ler a área de transferência.".into());
    };
    if let Ok(img) = board.get_image() {
        if !image_fits(img.width, img.height, img.bytes.len()) {
            return SystemClip::Refused(
                "A imagem da área de transferência é grande demais para colar.".into(),
            );
        }
        return match write_png(dir, img.width as u32, img.height as u32, &img.bytes) {
            Ok(path) => SystemClip::Image {
                path,
                width: img.width as u32,
                height: img.height as u32,
            },
            Err(_) => SystemClip::Refused("Não foi possível colar a imagem.".into()),
        };
    }
    match board.get_text() {
        Ok(t) if !t.trim().is_empty() => SystemClip::Text(t),
        _ => SystemClip::Empty,
    }
}

/// Grava os pixels RGBA num PNG novo da pasta `dir`, legível só pelo dono.
fn write_png(
    dir: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("gera-colar-{}-{n}.png", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let file = opts.open(&path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let len = width as usize * height as usize * 4;
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgba[..len]))
        .map_err(std::io::Error::other)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_comment_pastes_with_offset_on_the_same_page() {
        let mut c = AnnotClipboard::default();
        let marker = c.copy(2, 7, "nota");
        assert_eq!(
            c.paste(Some(&marker), 2),
            Paste::Annot {
                page: 2,
                id: 7,
                to_page: 2,
                dx: 10.0,
                dy: 10.0
            }
        );
        // colar de novo: mais um degrau, para não ficar uma em cima da outra
        assert_eq!(
            c.paste(Some(&marker), 2),
            Paste::Annot {
                page: 2,
                id: 7,
                to_page: 2,
                dx: 20.0,
                dy: 20.0
            }
        );
        // em outra página: no mesmo lugar
        assert_eq!(
            c.paste(Some(&marker), 5),
            Paste::Annot {
                page: 2,
                id: 7,
                to_page: 5,
                dx: 0.0,
                dy: 0.0
            }
        );
    }

    #[test]
    fn a_comment_without_text_still_pastes() {
        let mut c = AnnotClipboard::default();
        let marker = c.copy(0, 3, "");
        assert!(
            !marker.is_empty(),
            "a área de transferência recebe um rótulo"
        );
        assert!(matches!(c.paste(Some(&marker), 0), Paste::Annot { .. }));
    }

    #[test]
    fn newer_outside_text_wins_over_the_copied_comment() {
        let mut c = AnnotClipboard::default();
        c.copy(0, 3, "nota");
        assert_eq!(
            c.paste(Some("de outro programa"), 0),
            Paste::Text("de outro programa".into())
        );
        assert_eq!(
            c.paste(None, 0),
            Paste::FromSystem,
            "sem texto: lê a área (imagem)"
        );
    }

    #[test]
    fn pasted_text_becomes_a_text_box_and_is_capped() {
        let mut c = AnnotClipboard::default();
        let long = "a".repeat(MAX_PASTED_CHARS + 50);
        match c.paste(Some(&long), 0) {
            Paste::Text(t) => assert_eq!(t.chars().count(), MAX_PASTED_CHARS),
            p => panic!("{p:?}"),
        }
        assert_eq!(
            c.paste(Some("   "), 0),
            Paste::FromSystem,
            "texto vazio não cria caixa"
        );
    }

    #[test]
    fn huge_or_broken_images_are_refused() {
        assert!(image_fits(1000, 800, 1000 * 800 * 4));
        assert!(
            !image_fits(10_000, 10_000, 10_000 * 10_000 * 4),
            "acima de 40 Mpx"
        );
        assert!(!image_fits(100, 100, 10), "bytes faltando");
        assert!(!image_fits(0, 100, 0));
    }

    #[test]
    fn pasted_image_is_written_as_a_private_png() {
        let dir = tempfile::tempdir().unwrap();
        let rgba = [255u8, 0, 0, 255].repeat(4);
        let path = write_png(dir.path(), 2, 2, &rgba).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn pasted_items_go_in_the_middle_of_the_view() {
        // caixa de texto de 200 × 40 centrada no ponto visível
        let r = centered((300.0, 400.0), (200.0, 40.0), (612.0, 792.0));
        assert_eq!(r, pdf_core::PdfRect::new(200.0, 380.0, 400.0, 420.0));
        // perto da borda: fica dentro da página
        let r = centered((10.0, 10.0), (200.0, 40.0), (612.0, 792.0));
        assert_eq!((r.x0, r.y0), (0.0, 0.0));
    }
}
