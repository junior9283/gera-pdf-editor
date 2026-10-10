//! Criar PDF a partir de imagens e Exportar PDF (Word, texto, imagens), como as ferramentas do
//! Acrobat.
use crate::doc::{write_atomic, PdfDoc};
use crate::error::{PdfError, Result};
use mupdf::pdf::{Encryption, PdfDocument, PdfWriteOptions};
use mupdf::{Buffer, Colorspace, DocumentWriter, Image, ImageFormat, Matrix, Size};
use std::path::{Path, PathBuf};

/// Imagem maior que isto (arquivo) não entra (fotos de câmera ficam bem abaixo).
const MAX_IMAGE_BYTES: u64 = 200 * 1024 * 1024;
/// Mais pixels que isto não são decodificados (imagem) nem renderizados (exportar): 100 Mpx RGB
/// já são 300 MB. Arquivo pequeno pode declarar dimensões enormes (bomba de descompressão).
const MAX_PIXELS: u64 = 100_000_000;
/// Maior lado de página que o PDF aceita (200 polegadas).
const MAX_PAGE_PT: f32 = 14_400.0;

fn refused(msg: impl Into<String>) -> PdfError {
    PdfError::Refused(msg.into())
}

fn open_image(path: &Path) -> Result<Image> {
    let meta = std::fs::metadata(path)?;
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return Err(refused(format!(
            "{name}: não é uma imagem que dê para usar"
        )));
    }
    let utf8 = path
        .to_str()
        .ok_or_else(|| refused("caminho não é UTF-8 válido"))?;
    let img = Image::from_file(utf8)
        .map_err(|_| refused(format!("{name}: não foi possível ler a imagem")))?;
    // só o cabeçalho foi lido até aqui: as dimensões são conferidas antes de decodificar
    if u64::from(img.width()) * u64::from(img.height()) > MAX_PIXELS {
        return Err(refused(format!(
            "{name}: imagem grande demais (mais de 100 megapixels)"
        )));
    }
    Ok(img)
}

/// Cria um PDF com uma página por imagem, do tamanho da imagem pela resolução dela (sem
/// resolução: 72 ppp, como o Acrobat); JPEG entra sem recompressão.
pub fn create_pdf_from_images(images: &[PathBuf], dest: &Path) -> Result<()> {
    if images.is_empty() {
        return Err(refused("escolha ao menos uma imagem"));
    }
    let mut doc = PdfDocument::new();
    for path in images {
        let img = open_image(path)?;
        let (xr, yr) = img.resolution();
        let dpi = |r: i32| if r > 0 { r as f32 } else { 72.0 };
        let mut w = img.width() as f32 * 72.0 / dpi(xr);
        let mut h = img.height() as f32 * 72.0 / dpi(yr);
        // o PDF não aceita páginas maiores que 200 pol: reduz mantendo a proporção
        let fit = (MAX_PAGE_PT / w.max(h)).min(1.0);
        w *= fit;
        h *= fit;
        let image = doc.add_image(&img)?;
        let page = doc.new_page(Size {
            width: w,
            height: h,
        })?;
        let mut obj = page.object();
        let mut xobjects = doc.new_dict()?;
        xobjects.dict_put("Im0", image)?;
        let mut resources = doc.new_dict()?;
        resources.dict_put("XObject", xobjects)?;
        obj.dict_put("Resources", resources)?;
        let ops = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q");
        let contents = doc.add_stream(&Buffer::from_bytes(ops.as_bytes())?, None, false)?;
        obj.dict_put("Contents", contents)?;
    }
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::None).set_compress(true);
    write_atomic(&doc, dest, opts)
}

/// Formato de Exportar PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Microsoft Word (.docx), pelo conversor do MuPDF.
    Word,
    /// Texto simples (.txt), página após página.
    Text,
    /// Uma imagem PNG por página.
    Png { dpi: u32 },
    /// Uma imagem JPEG por página (qualidade 90).
    Jpeg { dpi: u32 },
}

/// Pasta livre ao lado de `base`: "<stem> (imagens)", "<stem> (imagens) (2)"…
fn free_dir(base: &Path, stem: &str) -> PathBuf {
    let first = base.with_file_name(format!("{stem} (imagens)"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| base.with_file_name(format!("{stem} (imagens) ({n})")))
        .find(|p| !p.exists())
        .expect("algum nome livre")
}

/// Arquivo livre ao lado de `base` com a extensão `ext`.
fn free_file(base: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = base.with_file_name(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| base.with_file_name(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("algum nome livre")
}

impl PdfDoc {
    /// Exporta o documento ao lado do original (nomes livres, nunca sobrescreve); devolve os
    /// arquivos criados.
    pub fn export(&self, format: ExportFormat) -> Result<Vec<PathBuf>> {
        if !self.security()?.permissions.copy {
            return Err(refused(
                "o documento não permite copiar o conteúdo: exportar não é permitido",
            ));
        }
        let stem = crate::split::stem_of(&self.path, "documento");
        match format {
            ExportFormat::Text => {
                let dest = free_file(&self.path, &stem, "txt");
                crate::doc::write_bytes_atomic(&dest, self.document_text()?.as_bytes())?;
                Ok(vec![dest])
            }
            ExportFormat::Word => {
                let dest = free_file(&self.path, &stem, "docx");
                let utf8 = dest
                    .to_str()
                    .ok_or_else(|| refused("caminho não é UTF-8 válido"))?;
                let mut writer = DocumentWriter::new(utf8, "docx", "")?;
                for p in 0..self.page_count()? {
                    let page = self.load(p)?;
                    let dev = writer.begin_page(page.bounds()?)?;
                    page.run(&dev, &Matrix::IDENTITY)?;
                    writer.end_page(dev)?;
                }
                // fechar grava o arquivo
                drop(writer);
                Ok(vec![dest])
            }
            ExportFormat::Png { dpi } | ExportFormat::Jpeg { dpi } => {
                let dir = free_dir(&self.path, &stem);
                std::fs::create_dir_all(&dir)?;
                let scale = dpi.clamp(36, 600) as f32 / 72.0;
                let mut out = Vec::new();
                for p in 0..self.page_count()? {
                    let page = self.load(p)?;
                    let b = page.bounds()?;
                    let px = f64::from((b.x1 - b.x0) * scale) * f64::from((b.y1 - b.y0) * scale);
                    if px > MAX_PIXELS as f64 {
                        return Err(refused(format!(
                            "a página {} ficaria grande demais nessa resolução: escolha uma menor",
                            p + 1
                        )));
                    }
                    let pix = page.to_pixmap(
                        &Matrix::new_scale(scale, scale),
                        &Colorspace::device_rgb(),
                        false,
                        true,
                    )?;
                    let dest = if matches!(format, ExportFormat::Png { .. }) {
                        let dest = dir.join(format!("página {}.png", p + 1));
                        let utf8 = dest
                            .to_str()
                            .ok_or_else(|| refused("caminho não é UTF-8 válido"))?;
                        pix.save_as(utf8, ImageFormat::PNG)?;
                        dest
                    } else {
                        let dest = dir.join(format!("página {}.jpg", p + 1));
                        let (w, h) = (pix.width(), pix.height());
                        if w > u32::from(u16::MAX) || h > u32::from(u16::MAX) {
                            return Err(refused("página grande demais para JPEG nessa resolução"));
                        }
                        let mut jpeg = Vec::new();
                        jpeg_encoder::Encoder::new(&mut jpeg, 90)
                            .encode(
                                pix.samples(),
                                w as u16,
                                h as u16,
                                jpeg_encoder::ColorType::Rgb,
                            )
                            .map_err(|e| refused(e.to_string()))?;
                        crate::doc::write_bytes_atomic(&dest, &jpeg)?;
                        dest
                    };
                    out.push(dest);
                }
                Ok(out)
            }
        }
    }
}
