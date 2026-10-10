//! Dividir PDF e Combinar arquivos (como no Acrobat). As partes e o arquivo combinado levam as
//! páginas com os comentários e os marcadores que caem nelas; nada é gravado pela metade.
use crate::doc::{open_authenticated, purge, reopen, write_atomic, PdfDoc};
use crate::error::{PdfError, Result};
use crate::pages::{copy_pages, free_name};
use mupdf::document::Location;
use mupdf::link::LinkDestination;
use mupdf::pdf::{Encryption, PdfDocument, PdfWriteOptions};
use mupdf::{DestinationKind, Outline};
use std::path::{Path, PathBuf};

/// Como dividir o documento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitRule {
    /// N páginas por arquivo.
    Pages(u32),
    /// Arquivos de no máximo este tamanho (uma página maior que o limite fica sozinha).
    MaxBytes(u64),
    /// Um arquivo por marcador de primeiro nível.
    TopBookmarks,
}

/// Texto que serve de nome de arquivo: sem `/ \ : * ? " < > |` nem caracteres de controle.
pub fn safe_file_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Parte".into()
    } else {
        trimmed.to_owned()
    }
}

/// Destino de marcador para a página `page` (a partir de 0).
fn dest_to(page: u32) -> LinkDestination {
    LinkDestination {
        loc: Location {
            chapter: 0,
            page_in_chapter: page,
            page_number: page,
        },
        kind: DestinationKind::Fit,
    }
}

/// Os marcadores que caem nas páginas `first..first + len`, levados para começar em `to`. Um
/// marcador de fora com filhos de dentro some, e os filhos sobem um nível.
fn shifted(items: &[Outline], first: u32, len: u32, to: u32) -> Vec<Outline> {
    let mut out = Vec::new();
    for it in items {
        let down = shifted(&it.down, first, len, to);
        let page = it.dest.as_ref().map(|d| d.loc.page_number);
        match page {
            Some(p) if (first..first + len).contains(&p) => out.push(Outline {
                title: it.title.clone(),
                uri: None,
                dest: Some(dest_to(p - first + to)),
                down,
            }),
            _ => out.extend(down),
        }
    }
    out
}

/// Grava as páginas `pages` (contíguas) de `source` em `path`, com os marcadores delas.
fn write_part(source: &PdfDocument, pages: &[i32], path: &Path) -> Result<()> {
    let mut out = PdfDocument::new();
    copy_pages(&mut out, source, pages, 0)?;
    let first = pages.first().copied().unwrap_or(0) as u32;
    out.set_outlines(&shifted(&source.outlines()?, first, pages.len() as u32, 0))?;
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::None);
    write_atomic(&out, path, opts)
}

/// Tamanho que as páginas teriam num arquivo.
fn size_of(source: &PdfDocument, pages: &[i32]) -> Result<u64> {
    let mut out = PdfDocument::new();
    copy_pages(&mut out, source, pages, 0)?;
    let mut opts = PdfWriteOptions::default();
    opts.set_garbage_level(1);
    let mut bytes = Vec::new();
    out.write_to_with_options(&mut bytes, opts)?;
    Ok(bytes.len() as u64)
}

impl PdfDoc {
    /// Divide o documento em arquivos novos na pasta `dir` (`<stem>_Parte1.pdf`… ou, por
    /// marcadores, `<stem>_<Título>.pdf`). O documento não muda.
    pub fn split(&self, rule: SplitRule, dir: &Path, stem: &str) -> Result<Vec<PathBuf>> {
        let count = self.page_count()?;
        // as partes saem de uma réplica (copiar mexe na origem), sem as anotações apagadas
        let snap = self.snapshot()?;
        let source = reopen(&snap.bytes, self.password.as_deref())?;
        purge(&source, &snap.hidden)?;
        let mut parts: Vec<(Vec<i32>, String)> = Vec::new();
        let numbered = |k: usize| format!("{stem}_Parte{k}");
        match rule {
            SplitRule::Pages(n) => {
                let n = n.max(1) as usize;
                for (k, chunk) in (0..count).collect::<Vec<_>>().chunks(n).enumerate() {
                    parts.push((chunk.to_vec(), numbered(k + 1)));
                }
            }
            SplitRule::MaxBytes(max) => {
                let mut group: Vec<i32> = Vec::new();
                for p in 0..count {
                    let mut tentative = group.clone();
                    tentative.push(p);
                    if !group.is_empty() && size_of(&source, &tentative)? > max {
                        parts.push((std::mem::take(&mut group), numbered(parts.len() + 1)));
                        group.push(p);
                    } else {
                        group = tentative;
                    }
                }
                if !group.is_empty() {
                    parts.push((group, numbered(parts.len() + 1)));
                }
            }
            SplitRule::TopBookmarks => {
                let mut starts: Vec<(u32, String)> = source
                    .outlines()?
                    .iter()
                    .filter_map(|o| Some((o.dest.as_ref()?.loc.page_number, o.title.clone())))
                    .filter(|(p, _)| (*p as i32) < count)
                    .collect();
                // fora de ordem ou na mesma página: vale o primeiro título de cada página
                starts.sort_by_key(|(p, _)| *p);
                starts.dedup_by_key(|(p, _)| *p);
                if starts.is_empty() {
                    return Err(PdfError::InvalidPages(
                        "o documento não tem marcadores de primeiro nível".into(),
                    ));
                }
                if starts[0].0 != 0 {
                    starts.insert(0, (0, "Início".into()));
                }
                for (k, (start, title)) in starts.iter().enumerate() {
                    let end = starts.get(k + 1).map_or(count as u32, |s| s.0);
                    let pages = (*start as i32..end as i32).collect();
                    parts.push((pages, format!("{stem}_{}", safe_file_name(title))));
                }
            }
        }
        let mut written = Vec::new();
        for (pages, name) in parts {
            let path = free_name(&dir.join("x.pdf"), &name);
            write_part(&source, &pages, &path)?;
            written.push(path);
        }
        Ok(written)
    }
}

/// Junta os PDFs (na ordem, com a senha de cada um se precisar) num arquivo novo em `dest`: um
/// marcador por arquivo (o nome dele) com os marcadores originais dentro.
pub fn combine(inputs: &[(PathBuf, Option<String>)], dest: &Path) -> Result<()> {
    if inputs.is_empty() {
        return Err(PdfError::InvalidPages(
            "nenhum arquivo para combinar".into(),
        ));
    }
    let mut out = PdfDocument::new();
    let mut marks = Vec::new();
    for (path, password) in inputs {
        let src = open_authenticated(path, password.as_deref())?;
        let n = src.page_count()?;
        let at = out.page_count()?;
        let outline = src.outlines()?;
        copy_pages(&mut out, &src, &(0..n).collect::<Vec<_>>(), at)?;
        marks.push(Outline {
            title: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            uri: None,
            dest: Some(dest_to(at as u32)),
            down: shifted(&outline, 0, n as u32, at as u32),
        });
    }
    out.set_outlines(&marks)?;
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::None);
    write_atomic(&out, dest, opts)
}
