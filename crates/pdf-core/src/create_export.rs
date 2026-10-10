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

/// Orientação EXIF (tag 0x0112) de um JPEG, de 1 a 8; 1 quando não há.
fn exif_orientation(bytes: &[u8]) -> u16 {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return 1;
    }
    // segmentos JPEG até achar o APP1 "Exif"
    let mut i = 2;
    while i + 4 <= bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if marker == 0xDA || len < 2 {
            break; // começo da imagem: sem EXIF antes
        }
        let seg = bytes.get(i + 4..i + 2 + len).unwrap_or_default();
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return tiff_orientation(&seg[6..]).unwrap_or(1);
        }
        i += 2 + len;
    }
    1
}

fn tiff_orientation(t: &[u8]) -> Option<u16> {
    let le = match t.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = [*t.get(o)?, *t.get(o + 1)?];
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = [*t.get(o)?, *t.get(o + 1)?, *t.get(o + 2)?, *t.get(o + 3)?];
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let ifd = u32_at(4)? as usize;
    let n = usize::from(u16_at(ifd)?);
    (0..n.min(256))
        .find_map(|k| {
            let e = ifd + 2 + k * 12;
            (u16_at(e)? == 0x0112).then(|| u16_at(e + 8)).flatten()
        })
        .filter(|o| (1..=8).contains(o))
}

/// Matriz (a b c d e f) que desenha a imagem crua (quadrado unitário) numa página `w`×`h` já em
/// pé, para cada orientação EXIF.
fn oriented(o: u16, w: f32, h: f32) -> [f32; 6] {
    match o {
        2 => [-w, 0.0, 0.0, h, w, 0.0],
        3 => [-w, 0.0, 0.0, -h, w, h],
        4 => [w, 0.0, 0.0, -h, 0.0, h],
        5 => [0.0, -h, -w, 0.0, w, h],
        6 => [0.0, -h, w, 0.0, 0.0, h],
        7 => [0.0, h, w, 0.0, 0.0, 0.0],
        8 => [0.0, h, -w, 0.0, w, 0.0],
        _ => [w, 0.0, 0.0, h, 0.0, 0.0],
    }
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
        // fotos de celular: a orientação EXIF diz como a imagem fica em pé (como o Acrobat faz)
        let orientation = std::fs::File::open(path)
            .and_then(|f| {
                use std::io::Read;
                let mut head = Vec::new();
                f.take(256 * 1024).read_to_end(&mut head)?;
                Ok(exif_orientation(&head))
            })
            .unwrap_or(1);
        let (xr, yr) = img.resolution();
        let dpi = |r: i32| if r > 0 { r as f32 } else { 72.0 };
        let mut w = img.width() as f32 * 72.0 / dpi(xr);
        let mut h = img.height() as f32 * 72.0 / dpi(yr);
        // o PDF não aceita páginas maiores que 200 pol: reduz mantendo a proporção
        let fit = (MAX_PAGE_PT / w.max(h)).min(1.0);
        w *= fit;
        h *= fit;
        if (5..=8).contains(&orientation) {
            std::mem::swap(&mut w, &mut h);
        }
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
        let [a, b, c, d, e, f] = oriented(orientation, w, h);
        let ops = format!("q {a} {b} {c} {d} {e} {f} cm /Im0 Do Q");
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
                self.export_word(&dest)?;
                Ok(vec![dest])
            }
            ExportFormat::Png { dpi } | ExportFormat::Jpeg { dpi } => {
                let scale = dpi.clamp(36, 600) as f32 / 72.0;
                // todas as páginas conferidas antes: nada de pasta pela metade
                for p in 0..self.page_count()? {
                    let b = self.load(p)?.bounds()?;
                    let px = f64::from((b.x1 - b.x0) * scale) * f64::from((b.y1 - b.y0) * scale);
                    if px > MAX_PIXELS as f64 {
                        return Err(refused(format!(
                            "a página {} ficaria grande demais nessa resolução: escolha uma menor",
                            p + 1
                        )));
                    }
                }
                let dir = free_dir(&self.path, &stem);
                std::fs::create_dir_all(&dir)?;
                let written = self.export_images(&dir, format, scale);
                if written.is_err() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                written
            }
        }
    }

    /// Word pelo conversor do MuPDF, reempacotado num zip limpo (o do MuPDF marca "data
    /// descriptor" só no cabeçalho local, e o LibreOffice recusa).
    fn export_word(&self, dest: &Path) -> Result<()> {
        // o fechamento do conversor não tem proteção no binding: sem espaço, o MuPDF encerraria o
        // app. Confere antes que cabe com folga (o .docx raramente passa do tamanho do PDF).
        let need = std::fs::metadata(&self.path)?.len().saturating_mul(3) + 50 * 1024 * 1024;
        if free_space(dest.parent().unwrap_or(Path::new("."))).is_some_and(|f| f < need) {
            return Err(refused(
                "não há espaço livre suficiente no disco para exportar",
            ));
        }
        let tmp = dest.with_extension("docx.tmp");
        let utf8 = tmp
            .to_str()
            .ok_or_else(|| refused("caminho não é UTF-8 válido"))?;
        let converted = (|| -> Result<()> {
            let mut writer = DocumentWriter::new(utf8, "docx", "")?;
            for p in 0..self.page_count()? {
                let page = self.load(p)?;
                let dev = writer.begin_page(page.bounds()?)?;
                page.run(&dev, &Matrix::IDENTITY)?;
                writer.end_page(dev)?;
            }
            drop(writer); // fecha e grava
            let clean = repack_zip(&std::fs::read(&tmp)?)?;
            crate::doc::write_bytes_atomic(dest, &clean)
        })();
        let _ = std::fs::remove_file(&tmp);
        converted
    }

    /// Uma imagem por página em `dir` (PNG ou JPEG).
    fn export_images(&self, dir: &Path, format: ExportFormat, scale: f32) -> Result<Vec<PathBuf>> {
        {
            {
                let mut out = Vec::new();
                for p in 0..self.page_count()? {
                    let page = self.load(p)?;
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

/// Espaço livre (bytes) no sistema de arquivos de `dir`.
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: caminho terminado em NUL e estrutura de saída válida
    (unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0).then(|| st.f_bavail * st.f_frsize)
}

/// Regrava um zip entrada por entrada (comprimido, sem "data descriptor").
fn repack_zip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::{Read, Write};
    let bad = |e: zip::result::ZipError| refused(format!("arquivo do Word inválido: {e}"));
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(bad)?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for i in 0..src.len() {
        let mut entry = src.by_index(i).map_err(bad)?;
        let name = entry.name().to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        out.start_file(name, opts).map_err(bad)?;
        out.write_all(&data)?;
    }
    Ok(out.finish().map_err(bad)?.into_inner())
}
