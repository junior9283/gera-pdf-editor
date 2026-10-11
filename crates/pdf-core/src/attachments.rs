//! Anexos (painel Anexos do Acrobat): os arquivos anexados ao documento (`/EmbeddedFiles`) e os de
//! comentário (`FileAttachment`). Adicionar, remover e descrever são etapas do desfazer.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::types::{AttachmentInfo, AttachmentOrigin};
use mupdf::pdf::{EmbeddedFileOptions, PdfAnnotationType, PdfObject};
use std::path::Path;

/// Maior anexo que o app adiciona.
pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// Maior anexo que o app lê (abrir ou salvar).
const MAX_READ_BYTES: usize = 512 * 1024 * 1024;
/// Teto de anexos listados.
const MAX_LISTED: usize = 10_000;

fn get(obj: &PdfObject, key: &str) -> Option<PdfObject> {
    obj.get_dict(key).ok().flatten()
}

fn text(obj: &PdfObject, key: &str) -> String {
    get(obj, key)
        .and_then(|v| v.as_string().ok())
        .unwrap_or_default()
}

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
    if !taken.iter().any(|t| t == wanted) {
        return wanted.to_owned();
    }
    let path = Path::new(wanted);
    let stem = path
        .file_stem()
        .map_or(wanted.into(), |s| s.to_string_lossy());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| !taken.iter().any(|t| t == c))
        .unwrap_or_else(|| wanted.to_owned())
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
        let mut out: Vec<AttachmentInfo> = self
            .document_files()
            .into_iter()
            .map(|(name, fs)| {
                let size = embedded_stream(&fs).map_or(0, |s| declared_size(&s));
                AttachmentInfo {
                    filename: file_name(&fs),
                    description: text(&fs, "Desc"),
                    size,
                    name,
                    origin: AttachmentOrigin::Document,
                }
            })
            .collect();
        for page in 0..self.page_count().unwrap_or(0) {
            let Ok(pdf_page) = self.inner.load_pdf_page(page) else {
                continue;
            };
            for annot in pdf_page.annotations() {
                if out.len() >= MAX_LISTED {
                    return out;
                }
                if annot.r#type().ok() != Some(PdfAnnotationType::FileAttachment) {
                    continue;
                }
                let obj = annot.object();
                let Some(fs) = get(&obj, "FS") else { continue };
                let filename = file_name(&fs);
                let Ok(id) = obj.as_indirect() else { continue };
                out.push(AttachmentInfo {
                    name: filename.clone(),
                    filename,
                    description: text(&obj, "Contents"),
                    size: embedded_stream(&fs).map_or(0, |s| declared_size(&s)),
                    origin: AttachmentOrigin::Comment { page, id },
                });
            }
        }
        out
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
        let bytes = stream.read_stream()?;
        if bytes.len() > MAX_READ_BYTES {
            return Err(refused("o anexo é grande demais"));
        }
        Ok(bytes)
    }

    /// Anexa o arquivo ao documento; devolve o nome (único) que ele recebeu.
    pub fn add_attachment(&mut self, path: &Path, description: &str) -> Result<String> {
        let len = std::fs::metadata(path)?.len();
        if len > MAX_ATTACHMENT_BYTES {
            return Err(refused("o arquivo passa de 100 MB"));
        }
        let wanted = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| refused("arquivo sem nome"))?;
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

    #[test]
    fn unique_names() {
        let taken = vec!["a.txt".to_string(), "a (2).txt".to_string()];
        assert_eq!(unique_name("a.txt", &taken), "a (3).txt");
        assert_eq!(unique_name("b.txt", &taken), "b.txt");
        assert_eq!(unique_name("leia", &["leia".to_string()]), "leia (2)");
    }
}
