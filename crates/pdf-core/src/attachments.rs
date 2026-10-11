//! Anexos (painel Anexos do Acrobat): os arquivos anexados ao documento (`/EmbeddedFiles`) e os de
//! comentário (`FileAttachment`). Adicionar, remover e descrever são etapas do desfazer.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::pdf_obj::{get, name_of, text};
use crate::types::{AttachmentInfo, AttachmentOrigin};
use mupdf::pdf::{EmbeddedFileOptions, PdfAnnotationType, PdfObject, PdfPage};
use std::path::Path;

/// Maior anexo que o app adiciona.
pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// Maior anexo que o app lê (abrir ou salvar).
const MAX_READ_BYTES: usize = 512 * 1024 * 1024;
/// Teto de anexos listados.
const MAX_LISTED: usize = 10_000;

/// Nome do arquivo no filespec (`/UF`, Unicode, ou `/F`).
fn file_name(fs: &PdfObject) -> String {
    Some(text(fs, "UF"))
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| text(fs, "F"))
}

/// O stream do arquivo dentro do filespec (`/EF /UF` ou `/EF /F`).
fn embedded_stream(fs: &PdfObject) -> Option<PdfObject> {
    let ef = get(fs, "EF")?;
    get(&ef, "UF").or_else(|| get(&ef, "F"))
}

/// Tamanho declarado do arquivo (`/Params /Size`), ou o comprimento do stream.
fn declared_size(stream: &PdfObject) -> usize {
    get(stream, "Params")
        .and_then(|p| get(&p, "Size"))
        .or_else(|| get(stream, "Length"))
        .and_then(|v| v.as_int().ok())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

/// Nome único entre `taken`: "nota.txt", "nota (2).txt", "nota (3).txt"…
fn unique_name(wanted: &str, taken: &[String]) -> String {
    let path = Path::new(wanted);
    let stem = path
        .file_stem()
        .map_or(wanted.into(), |s| s.to_string_lossy());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    numbered(wanted, &stem, &ext, taken)
}

/// `wanted` se estiver livre; senão "stem (2)ext", "stem (3)ext"… (anexos e destinos).
pub(crate) fn numbered(wanted: &str, stem: &str, ext: &str, taken: &[String]) -> String {
    let free = |c: &str| !taken.iter().any(|t| t == c);
    if free(wanted) {
        return wanted.to_owned();
    }
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| free(c))
        .unwrap_or_else(|| wanted.to_owned())
}

/// Nomes dos filtros do stream (`/Filter` nome ou lista). `None` quando a lista não é só de
/// nomes ou é longa demais: o app não decide sobre o que o MuPDF leria diferente.
fn filters(stream: &PdfObject) -> Option<Vec<String>> {
    let Some(f) = get(stream, "Filter") else {
        return Some(Vec::new());
    };
    if f.is_array().unwrap_or(false) {
        let len = f.len().ok()?;
        if len > 16 {
            return None;
        }
        (0..len as i32)
            .map(|k| name_of(&f.get_array(k).ok()??))
            .collect()
    } else {
        Some(vec![name_of(&f)?])
    }
}

/// Descomprime o Flate `raw` com teto (memória limitada a `cap`): zlib, ou deflate puro quando
/// falta o cabeçalho. Dado corrompido para no erro, como no MuPDF. `None` se passar do teto.
fn inflate_capped(raw: &[u8], cap: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    fn read(r: impl Read, cap: usize) -> (Vec<u8>, bool) {
        let (mut out, mut buf) = (Vec::new(), [0u8; 64 * 1024]);
        let mut r = r.take(cap as u64 + 1);
        let failed = loop {
            match r.read(&mut buf) {
                Ok(0) => break false,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => break true,
            }
        };
        (out, failed)
    }
    let (mut out, failed) = read(flate2::read::ZlibDecoder::new(raw), cap);
    if failed && out.is_empty() {
        out = read(flate2::read::DeflateDecoder::new(raw), cap).0;
    }
    (out.len() <= cap).then_some(out)
}

/// Conteúdo do stream com teto de `cap` bytes, sem descomprimir uma bomba: o tamanho declarado
/// vem do arquivo e pode mentir. O Flate (o filtro dos anexos) é descomprimido aqui, em blocos e
/// com teto; os demais filtros que expandem só valem para streams pequenos.
pub(crate) fn read_capped(stream: &PdfObject, cap: usize) -> Result<Vec<u8>> {
    let too_big = || refused("o anexo é grande demais");
    let unsafe_filter = || refused("o anexo usa uma compressão que o app não abre com segurança");
    let filters = filters(stream).ok_or_else(unsafe_filter)?;
    // ASCIIHex e ASCII85 só encolhem; os demais podem expandir
    let expanding = filters
        .iter()
        .filter(|f| {
            !matches!(
                f.as_str(),
                "ASCIIHexDecode" | "AHx" | "ASCII85Decode" | "A85"
            )
        })
        .count();
    if expanding > 0 {
        let raw = stream.read_raw_stream()?;
        let plain_flate = matches!(filters.as_slice(), [f] if f == "FlateDecode" || f == "Fl")
            && get(stream, "DecodeParms").is_none();
        if plain_flate {
            return inflate_capped(&raw, cap).ok_or_else(too_big);
        }
        if expanding > 1 || raw.len() > cap / 4096 {
            // LZW e RunLength expandem até ~4096×: só os pequenos são seguros
            return Err(unsafe_filter());
        }
    }
    let bytes = stream.read_stream()?;
    if bytes.len() > cap {
        return Err(too_big());
    }
    Ok(bytes)
}

/// Anexos de comentário (`FileAttachment`) da página já carregada, até o teto da lista.
pub(crate) fn comment_attachments(pdf_page: &PdfPage, page: i32, out: &mut Vec<AttachmentInfo>) {
    for annot in pdf_page.annotations() {
        if out.len() >= MAX_LISTED {
            return;
        }
        if annot.r#type().ok() != Some(PdfAnnotationType::FileAttachment) {
            continue;
        }
        let obj = annot.object();
        let Some(fs) = get(&obj, "FS") else { continue };
        let Ok(id) = obj.as_indirect() else { continue };
        out.push(AttachmentInfo {
            name: file_name(&fs),
            description: text(&obj, "Contents"),
            size: embedded_stream(&fs).map_or(0, |s| declared_size(&s)),
            origin: AttachmentOrigin::Comment { page, id },
        });
    }
}

/// Nome e tamanho do arquivo a anexar (até 100 MB; arquivo sem nome é recusado).
pub(crate) fn attachment_file(path: &Path) -> Result<(String, u64)> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_ATTACHMENT_BYTES {
        return Err(refused("o arquivo passa de 100 MB"));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| refused("arquivo sem nome"))?;
    Ok((name, len))
}

impl PdfDoc {
    /// Os pares (nome, filespec) da árvore `/EmbeddedFiles` (lista simples, como o MuPDF lê).
    fn document_files(&self) -> Vec<(String, PdfObject)> {
        let names = self
            .inner
            .catalog()
            .ok()
            .and_then(|c| get(&c, "Names"))
            .and_then(|n| get(&n, "EmbeddedFiles"))
            .and_then(|e| get(&e, "Names"));
        let Some(names) = names.filter(|n| n.is_array().unwrap_or(false)) else {
            return Vec::new();
        };
        let len = names.len().unwrap_or(0).min(2 * MAX_LISTED) as i32;
        (0..len / 2)
            .filter_map(|k| {
                let key = names.get_array(2 * k).ok()??.as_string().ok()?;
                let fs = names.get_array(2 * k + 1).ok()??;
                Some((key, fs))
            })
            .collect()
    }

    /// Anexos do documento e dos comentários.
    pub fn attachments(&self) -> Vec<AttachmentInfo> {
        let mut out = self.document_attachments();
        for page in 0..self.page_count().unwrap_or(0) {
            if let Ok(pdf_page) = self.inner.load_pdf_page(page) {
                comment_attachments(&pdf_page, page, &mut out);
            }
        }
        out
    }

    /// Anexos do documento (`/EmbeddedFiles`).
    pub(crate) fn document_attachments(&self) -> Vec<AttachmentInfo> {
        self.document_files()
            .into_iter()
            .map(|(name, fs)| {
                let size = embedded_stream(&fs).map_or(0, |s| declared_size(&s));
                AttachmentInfo {
                    description: text(&fs, "Desc"),
                    size,
                    name,
                    origin: AttachmentOrigin::Document,
                }
            })
            .collect()
    }

    /// Conteúdo do anexo (com teto).
    pub fn attachment_bytes(&self, origin: &AttachmentOrigin, name: &str) -> Result<Vec<u8>> {
        let fs = match origin {
            AttachmentOrigin::Document => self
                .document_files()
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, fs)| fs),
            AttachmentOrigin::Comment { page, id } => {
                let pdf_page = self.inner.load_pdf_page(*page)?;
                pdf_page
                    .annotations()
                    .find(|a| a.object().as_indirect().ok() == Some(*id))
                    .and_then(|a| get(&a.object(), "FS"))
            }
        };
        let stream = fs
            .as_ref()
            .and_then(embedded_stream)
            .ok_or_else(|| refused("anexo não encontrado"))?;
        if declared_size(&stream) > MAX_READ_BYTES {
            return Err(refused("o anexo é grande demais"));
        }
        read_capped(&stream, MAX_READ_BYTES)
    }

    /// Anexa o arquivo ao documento; devolve o nome (único) que ele recebeu.
    pub fn add_attachment(&mut self, path: &Path, description: &str) -> Result<String> {
        let (wanted, _) = attachment_file(path)?;
        self.require_modifiable("anexar arquivos")?;
        let bytes = std::fs::read(path)?;
        let taken: Vec<String> = self.document_files().into_iter().map(|(n, _)| n).collect();
        let name = unique_name(&wanted, &taken);
        self.with_snapshot(|d| {
            let info = d
                .inner
                .add_embedded_file(&name, &bytes, EmbeddedFileOptions::new(&name))?;
            if !description.trim().is_empty() {
                let mut fs = d.inner.new_indirect(info.xref, 0)?;
                fs.dict_put("Desc", PdfObject::new_string(description)?)?;
            }
            Ok(())
        })?;
        Ok(name)
    }

    pub fn remove_attachment(&mut self, name: &str) -> Result<()> {
        if !self.document_files().iter().any(|(n, _)| n == name) {
            return Err(refused("anexo não encontrado"));
        }
        self.require_modifiable("remover anexos")?;
        self.with_snapshot(|d| {
            d.inner.delete_embedded_file(name)?;
            Ok(())
        })
    }

    pub fn set_attachment_description(&mut self, name: &str, text: &str) -> Result<()> {
        let (_, mut fs) = self
            .document_files()
            .into_iter()
            .find(|(n, _)| n == name)
            .ok_or_else(|| refused("anexo não encontrado"))?;
        self.require_modifiable("alterar anexos")?;
        self.with_snapshot(|_| {
            if text.trim().is_empty() {
                fs.dict_delete("Desc")?;
            } else {
                fs.dict_put("Desc", PdfObject::new_string(text)?)?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stream `FlateDecode` com `len` zeros comprimidos (cabem em poucos KB).
    fn zeros_stream(doc: &mut mupdf::pdf::PdfDocument, len: usize) -> PdfObject {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&vec![0u8; len]).unwrap();
        let packed = z.finish().unwrap();
        let mut dict = doc.new_dict().unwrap();
        dict.dict_put("Filter", PdfObject::new_name("FlateDecode").unwrap())
            .unwrap();
        // o tamanho declarado mente: diz 1 byte
        let mut params = doc.new_dict().unwrap();
        params
            .dict_put("Size", PdfObject::new_int(1).unwrap())
            .unwrap();
        dict.dict_put("Params", params).unwrap();
        let buf = mupdf::Buffer::from_bytes(&packed).unwrap();
        doc.add_stream(&buf, Some(&dict), true).unwrap()
    }

    #[test]
    fn a_decompression_bomb_is_refused_before_inflating() {
        let mut doc = mupdf::pdf::PdfDocument::new();
        let bomb = zeros_stream(&mut doc, 4 << 20);
        assert!(read_capped(&bomb, 1 << 20).is_err(), "expande além do teto");
        let ok = read_capped(&bomb, 8 << 20).unwrap();
        assert_eq!(ok.len(), 4 << 20, "dentro do teto, lê tudo");
    }

    /// Stream com `packed` como conteúdo bruto e o dicionário dado.
    fn raw_stream(doc: &mut mupdf::pdf::PdfDocument, packed: &[u8], parms: bool) -> PdfObject {
        let mut dict = doc.new_dict().unwrap();
        dict.dict_put("Filter", PdfObject::new_name("FlateDecode").unwrap())
            .unwrap();
        if parms {
            let mut p = doc.new_dict().unwrap();
            p.dict_put("Predictor", PdfObject::new_int(12).unwrap())
                .unwrap();
            dict.dict_put("DecodeParms", p).unwrap();
        }
        let buf = mupdf::Buffer::from_bytes(packed).unwrap();
        doc.add_stream(&buf, Some(&dict), true).unwrap()
    }

    #[test]
    fn deflate_without_zlib_header_is_capped_too() {
        use std::io::Write;
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&vec![7u8; 4 << 20]).unwrap();
        let packed = z.finish().unwrap();
        let mut doc = mupdf::pdf::PdfDocument::new();
        let s = raw_stream(&mut doc, &packed, false);
        assert!(
            read_capped(&s, 1 << 20).is_err(),
            "sem cabeçalho também tem teto"
        );
        let all = read_capped(&s, 8 << 20).unwrap();
        assert_eq!(all, vec![7u8; 4 << 20]);
    }

    #[test]
    fn flate_with_parameters_and_a_big_body_is_refused() {
        let mut doc = mupdf::pdf::PdfDocument::new();
        let s = raw_stream(&mut doc, &vec![0x78; 1 << 20], true);
        assert!(
            read_capped(&s, 64 << 20).is_err(),
            "o app não mede o preditor"
        );
    }

    #[test]
    fn a_long_filter_chain_cannot_hide_the_flate() {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&vec![0u8; 4 << 20]).unwrap();
        let packed = z.finish().unwrap();
        let mut doc = mupdf::pdf::PdfDocument::new();
        for chain in [
            // o Flate depois de 16 filtros que só encolhem
            (0..17)
                .map(|k| if k == 16 { "FlateDecode" } else { "AHx" })
                .collect::<Vec<_>>(),
            vec!["AHx", "FlateDecode"],
        ] {
            let mut arr = doc.new_array().unwrap();
            for f in &chain {
                arr.array_push(PdfObject::new_name(f).unwrap()).unwrap();
            }
            let mut dict = doc.new_dict().unwrap();
            dict.dict_put("Filter", arr).unwrap();
            let buf = mupdf::Buffer::from_bytes(&packed).unwrap();
            let s = doc.add_stream(&buf, Some(&dict), true).unwrap();
            assert!(read_capped(&s, 1 << 20).is_err(), "{chain:?}");
        }
    }

    #[test]
    fn a_filter_that_is_not_a_name_is_refused() {
        let mut doc = mupdf::pdf::PdfDocument::new();
        let mut arr = doc.new_array().unwrap();
        arr.array_push(PdfObject::new_int(3).unwrap()).unwrap();
        arr.array_push(PdfObject::new_name("FlateDecode").unwrap())
            .unwrap();
        let mut dict = doc.new_dict().unwrap();
        dict.dict_put("Filter", arr).unwrap();
        let buf = mupdf::Buffer::from_bytes(&vec![1u8; 1 << 20]).unwrap();
        let s = doc.add_stream(&buf, Some(&dict), true).unwrap();
        assert!(read_capped(&s, 64 << 20).is_err());
    }

    #[test]
    fn unique_names() {
        let taken = vec!["a.txt".to_string(), "a (2).txt".to_string()];
        assert_eq!(unique_name("a.txt", &taken), "a (3).txt");
        assert_eq!(unique_name("b.txt", &taken), "b.txt");
        assert_eq!(unique_name("leia", &["leia".to_string()]), "leia (2)");
    }
}
