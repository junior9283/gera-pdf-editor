//! Comprimir PDF (como no Acrobat): grava uma cópia menor — estrutura enxuta (objetos repetidos
//! unidos, fluxos comprimidos) e, nos níveis Média e Alta, fotos reduzidas e recodificadas em JPEG.
use crate::doc::{write_bytes_atomic, PdfDoc};
use crate::error::{PdfError, Result};
use mupdf::pdf::{Encryption, PdfDocument, PdfObject, PdfWriteOptions};
use mupdf::Buffer;
use std::collections::HashSet;
use std::path::Path;

/// Nível de compactação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressLevel {
    /// Só estrutura (sem perda).
    Low,
    /// Fotos acima de 225 ppp reduzidas a 150 ppp, JPEG qualidade 75.
    Medium,
    /// Fotos acima de 150 ppp reduzidas a 100 ppp, JPEG qualidade 60.
    High,
}

/// Política das imagens: (acima de quantos ppp reduz, para quantos, qualidade JPEG).
struct ImagePolicy {
    threshold: f32,
    target: f32,
    quality: u8,
}

impl CompressLevel {
    fn images(self) -> Option<ImagePolicy> {
        match self {
            CompressLevel::Low => None,
            CompressLevel::Medium => Some(ImagePolicy {
                threshold: 225.0,
                target: 150.0,
                quality: 75,
            }),
            CompressLevel::High => Some(ImagePolicy {
                threshold: 150.0,
                target: 100.0,
                quality: 60,
            }),
        }
    }
}

/// Resultado: tamanhos antes e depois (bytes) e quantas imagens foram reduzidas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressReport {
    pub before: u64,
    pub after: u64,
    pub images: usize,
}

/// Imagem grande demais para decodificar na memória (fica como está): 30 Mpx RGB = 90 MB.
const MAX_PIXELS: u64 = 30_000_000;
/// Profundidade máxima de formulários aninhados percorridos.
const MAX_DEPTH: usize = 8;

/// Redução por média de área (sem serrilhado), `n` componentes por pixel.
fn downscale(src: &[u8], w: usize, h: usize, n: usize, nw: usize, nh: usize) -> Vec<u8> {
    let mut out = vec![0u8; nw * nh * n];
    for y in 0..nh {
        let y0 = y * h / nh;
        let y1 = ((y + 1) * h / nh).max(y0 + 1);
        for x in 0..nw {
            let x0 = x * w / nw;
            let x1 = ((x + 1) * w / nw).max(x0 + 1);
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            for c in 0..n {
                let mut sum = 0u32;
                for yy in y0..y1 {
                    let row = yy * w * n;
                    for xx in x0..x1 {
                        sum += u32::from(src[row + xx * n + c]);
                    }
                }
                out[(y * nw + x) * n + c] = (sum / count) as u8;
            }
        }
    }
    out
}

/// Componentes de cor de uma imagem que sabemos recodificar (cinza ou RGB), ou `None`.
fn components(img: &PdfObject) -> Result<Option<usize>> {
    let Some(cs) = img.get_dict("ColorSpace")? else {
        return Ok(None);
    };
    if cs.is_name()? {
        return Ok(match cs.as_name()?.as_slice() {
            b"DeviceRGB" => Some(3),
            b"DeviceGray" => Some(1),
            _ => None,
        });
    }
    // [/ICCBased fluxo] com /N 1 ou 3
    if cs.is_array()? && cs.len()? == 2 {
        let kind = cs.get_array(0)?;
        let is_icc = kind
            .map(|k| k.as_name())
            .transpose()?
            .is_some_and(|n| n == b"ICCBased");
        if is_icc {
            let n = cs
                .get_array(1)?
                .and_then(|s| s.get_dict("N").ok().flatten())
                .map(|n| n.as_int())
                .transpose()?;
            return Ok(match n {
                Some(3) => Some(3),
                Some(1) => Some(1),
                _ => None,
            });
        }
    }
    Ok(None)
}

/// Reduz e recodifica a imagem se for uma foto acima do limite; devolve se trocou.
fn recompress(img: &mut PdfObject, page_in: (f32, f32), policy: &ImagePolicy) -> Result<bool> {
    let int =
        |k: &str| -> Result<Option<i32>> { Ok(img.get_dict(k)?.map(|v| v.as_int()).transpose()?) };
    let is_image = img
        .get_dict("Subtype")?
        .map(|s| s.as_name())
        .transpose()?
        .is_some_and(|n| n == b"Image");
    let mask = img
        .get_dict("ImageMask")?
        .map(|m| m.as_bool())
        .transpose()?
        .unwrap_or(false);
    if !is_image || mask || img.get_dict("Decode")?.is_some() || int("BitsPerComponent")? != Some(8)
    {
        return Ok(false);
    }
    // máscara de cor-chave (o JPEG mudaria as cores e abriria furos), SMask pré-multiplicada
    // (/Matte exige as mesmas dimensões) e JPX (o MuPDF devolve o fluxo cru)
    let color_key = img
        .get_dict("Mask")?
        .is_some_and(|m| m.is_array().unwrap_or(false));
    let matte = img
        .get_dict("SMask")?
        .is_some_and(|m| m.get_dict("Matte").ok().flatten().is_some());
    let jpx = img
        .get_dict("Filter")?
        .is_some_and(|f| f.to_string().contains("JPXDecode"));
    if color_key || matte || jpx {
        return Ok(false);
    }
    let (Some(w), Some(h), Some(n)) = (int("Width")?, int("Height")?, components(img)?) else {
        return Ok(false);
    };
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_PIXELS {
        return Ok(false);
    }
    // resolução efetiva na página (a imagem ocupando a página: o caso das digitalizações e fotos)
    let dpi = (w as f32 / page_in.0.max(0.1)).max(h as f32 / page_in.1.max(0.1));
    if dpi <= policy.threshold {
        return Ok(false);
    }
    let scale = policy.target / dpi;
    let (nw, nh) = (
        ((w as f32 * scale).round() as usize).max(1),
        ((h as f32 * scale).round() as usize).max(1),
    );
    if nw > u16::MAX as usize || nh > u16::MAX as usize {
        return Ok(false);
    }
    // amostras decodificadas (filtros aplicados); formatos que o MuPDF não lê ficam como estão
    let Ok(samples) = img.read_stream() else {
        return Ok(false);
    };
    if samples.len() < w * h * n {
        return Ok(false);
    }
    let small = downscale(&samples, w, h, n, nw, nh);
    let mut jpeg = Vec::new();
    let color = if n == 3 {
        jpeg_encoder::ColorType::Rgb
    } else {
        jpeg_encoder::ColorType::Luma
    };
    if jpeg_encoder::Encoder::new(&mut jpeg, policy.quality)
        .encode(&small, nw as u16, nh as u16, color)
        .is_err()
    {
        return Ok(false);
    }
    // só troca se ficar menor que o fluxo atual (nunca piorar)
    let current = img.read_raw_stream().map(|r| r.len()).unwrap_or(usize::MAX);
    if jpeg.len() >= current {
        return Ok(false);
    }
    img.write_raw_stream_buffer(&Buffer::from_bytes(&jpeg)?)?;
    img.dict_put("Width", PdfObject::new_int(nw as i32)?)?;
    img.dict_put("Height", PdfObject::new_int(nh as i32)?)?;
    img.dict_put("Filter", PdfObject::new_name("DCTDecode")?)?;
    // o /ColorSpace fica (um perfil ICC continua valendo com JPEG)
    img.dict_put("BitsPerComponent", PdfObject::new_int(8)?)?;
    img.dict_delete("DecodeParms")?;
    Ok(true)
}

/// Percorre os XObjects dos recursos (e dos formulários aninhados), recomprimindo cada imagem
/// uma vez só (a mesma imagem pode estar em várias páginas).
fn walk(
    resources: Option<PdfObject>,
    page_in: (f32, f32),
    policy: &ImagePolicy,
    seen: &mut HashSet<i32>,
    depth: usize,
) -> Result<usize> {
    let Some(xobjects) = resources.and_then(|r| r.get_dict("XObject").ok().flatten()) else {
        return Ok(0);
    };
    let mut changed = 0;
    for i in 0..xobjects.dict_len()? as i32 {
        let Some(mut x) = xobjects.get_dict_val(i)? else {
            continue;
        };
        let num = x.as_indirect()?;
        if num != 0 && !seen.insert(num) {
            continue;
        }
        let form = x
            .get_dict("Subtype")?
            .map(|s| s.as_name())
            .transpose()?
            .is_some_and(|n| n == b"Form");
        if form {
            if depth < MAX_DEPTH {
                changed += walk(x.get_dict("Resources")?, page_in, policy, seen, depth + 1)?;
            }
        } else if recompress(&mut x, page_in, policy)? {
            changed += 1;
        }
    }
    Ok(changed)
}

impl PdfDoc {
    /// Grava em `dest` uma cópia comprimida do documento (o aberto não muda).
    pub fn compress_to(&self, dest: &Path, level: CompressLevel) -> Result<CompressReport> {
        if self.signed_on_disk || self.has_signatures()? {
            return Err(PdfError::Refused(
                "o documento está assinado: comprimir invalidaria as assinaturas".into(),
            ));
        }
        let before = std::fs::metadata(&self.path)?.len();
        let mut keep = PdfWriteOptions::default();
        keep.set_encryption(Encryption::Keep);
        let bytes = self.with_file_layers(|d| {
            let mut bytes = Vec::new();
            d.inner.write_to_with_options(&mut bytes, keep)?;
            Ok(bytes)
        })?;
        let mut copy = PdfDocument::from_bytes(&bytes)?;
        if let Some(p) = self.password.as_deref() {
            copy.authenticate(p)?;
        }
        let mut images = 0;
        if let Some(policy) = level.images() {
            let mut seen = HashSet::new();
            for p in 0..copy.page_count()? {
                let page = copy.load_pdf_page(p)?;
                let b = page.bounds()?;
                let page_in = ((b.x1 - b.x0) / 72.0, (b.y1 - b.y0) / 72.0);
                let resources = copy.find_page(p)?.get_dict_inheritable("Resources")?;
                images += walk(resources, page_in, &policy, &mut seen, 0)?;
            }
        }
        let mut opts = PdfWriteOptions::default();
        opts.set_encryption(Encryption::Keep)
            .set_garbage_level(4)
            .set_compress(true)
            .set_compress_images(true)
            .set_compress_fonts(true);
        let mut out = Vec::new();
        copy.write_to_with_options(&mut out, opts)?;
        // nunca entregar um arquivo maior (ou igual): o Acrobat também avisa e não grava
        if out.len() as u64 >= before {
            return Err(PdfError::Refused(
                "o arquivo já está otimizado: a cópia comprimida não ficaria menor".into(),
            ));
        }
        write_bytes_atomic(dest, &out)?;
        Ok(CompressReport {
            before,
            after: out.len() as u64,
            images,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscaling_averages_areas() {
        // 4×2 cinza → 2×1: média de cada bloco 2×2
        let src = [0, 100, 200, 255, 0, 100, 200, 255];
        assert_eq!(downscale(&src, 4, 2, 1, 2, 1), vec![50, 227]);
    }
}
