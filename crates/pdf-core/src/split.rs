//! Dividir PDF e Combinar arquivos (como no Acrobat). As partes e o arquivo combinado levam as
//! páginas com os comentários e os marcadores que caem nelas; nada é gravado pela metade.
use crate::doc::{open_authenticated, PdfDoc};
use crate::error::{PdfError, Result};
use crate::pages::{copy_pages, free_name, write_new};
use mupdf::document::Location;
use mupdf::link::LinkDestination;
use mupdf::pdf::{PdfDocument, PdfWriteOptions};
use mupdf::Outline;
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

/// Bytes máximos de um título usado em nome de arquivo (o sistema aceita 255 no nome inteiro).
const MAX_NAME_BYTES: usize = 150;

/// Texto que serve de nome de arquivo: sem `/ \ : * ? " < > |` nem caracteres de controle, sem
/// ponto ou espaço no fim e curto o bastante para o sistema aceitar.
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
    let mut name = cleaned.trim().to_owned();
    if name.len() > MAX_NAME_BYTES {
        let cut = (0..=MAX_NAME_BYTES)
            .rev()
            .find(|&i| name.is_char_boundary(i))
            .unwrap_or(0);
        name.truncate(cut);
    }
    let name = name.trim_end_matches(['.', ' ']).trim();
    if name.is_empty() {
        "Parte".into()
    } else {
        name.to_owned()
    }
}

/// Nome do arquivo sem a extensão (ou `default`).
pub(crate) fn stem_of(path: &Path, default: &str) -> String {
    path.file_stem()
        .map_or_else(|| default.to_owned(), |s| s.to_string_lossy().into_owned())
}

/// Os marcadores que caem nas páginas `first..first + len`, levados para começar em `to`, com o
/// destino original (posição e zoom). Um marcador de fora com filhos de dentro some, e os
/// filhos sobem um nível. Com `keep_links`, marcadores que abrem endereços externos ficam.
fn shifted(items: &[Outline], first: u32, len: u32, to: u32, keep_links: bool) -> Vec<Outline> {
    let mut out = Vec::new();
    for it in items {
        let down = shifted(&it.down, first, len, to, keep_links);
        match (&it.dest, &it.uri) {
            (Some(d), _) if (first..first + len).contains(&d.loc.page_number) => {
                let page = d.loc.page_number - first + to;
                out.push(Outline {
                    title: it.title.clone(),
                    uri: None,
                    dest: Some(LinkDestination {
                        loc: Location {
                            chapter: 0,
                            page_in_chapter: page,
                            page_number: page,
                        },
                        kind: d.kind,
                    }),
                    down,
                });
            }
            (None, Some(uri)) if keep_links => out.push(Outline {
                title: it.title.clone(),
                uri: Some(uri.clone()),
                dest: None,
                down,
            }),
            _ => out.extend(down),
        }
    }
    out
}

/// As páginas `pages` (contíguas) de `source` num documento novo, com os marcadores delas.
fn build_part(source: &PdfDocument, pages: &[i32]) -> Result<PdfDocument> {
    let mut out = PdfDocument::new();
    copy_pages(&mut out, source, pages, 0)?;
    let first = pages.first().copied().unwrap_or(0) as u32;
    out.set_outlines(&shifted(
        &source.outlines()?,
        first,
        pages.len() as u32,
        0,
        false,
    ))?;
    Ok(out)
}

/// Tamanho exato que a parte teria gravada (com os marcadores e sem os objetos sem uso).
fn part_size(source: &PdfDocument, pages: &[i32]) -> Result<u64> {
    let mut opts = PdfWriteOptions::default();
    opts.set_garbage_level(1);
    let mut bytes = Vec::new();
    build_part(source, pages)?.write_to_with_options(&mut bytes, opts)?;
    Ok(bytes.len() as u64)
}

/// Até onde a parte que começa em `start` cabe no limite: busca exponencial e depois binária
/// (O(log n) medidas por parte, não uma por página). Ao menos uma página.
fn fit_end(source: &PdfDocument, start: i32, count: i32, max: u64) -> Result<i32> {
    let fits = |end: i32| -> Result<bool> {
        Ok(part_size(source, &(start..end).collect::<Vec<_>>())? <= max)
    };
    let (mut good, mut step) = (start + 1, 1);
    let mut bad = loop {
        let probe = (good + step).min(count);
        if probe == good {
            return Ok(good); // chegou ao fim do documento
        }
        if fits(probe)? {
            good = probe;
            step *= 2;
        } else {
            break probe;
        }
    };
    while bad - good > 1 {
        let mid = good + (bad - good) / 2;
        if fits(mid)? {
            good = mid;
        } else {
            bad = mid;
        }
    }
    Ok(good)
}

impl PdfDoc {
    /// Divide o documento em arquivos novos na pasta `dir` (`<stem>_Parte1.pdf`… ou, por
    /// marcadores, `<stem>_<Título>.pdf`). Se uma parte falha, as já gravadas são apagadas. O
    /// documento não muda.
    pub fn split(&self, rule: SplitRule, dir: &Path, stem: &str) -> Result<Vec<PathBuf>> {
        let count = self.page_count()?;
        let source = self.replica()?;
        let numbered = |k: usize| format!("{stem}_Parte{k}");
        let mut parts: Vec<(Vec<i32>, String)> = Vec::new();
        match rule {
            SplitRule::Pages(n) => {
                for (k, start) in (0..count).step_by(n.max(1) as usize).enumerate() {
                    let end = (start + n.max(1) as i32).min(count);
                    parts.push(((start..end).collect(), numbered(k + 1)));
                }
            }
            SplitRule::MaxBytes(max) => {
                let mut start = 0;
                while start < count {
                    let end = fit_end(&source, start, count, max)?;
                    parts.push(((start..end).collect(), numbered(parts.len() + 1)));
                    start = end;
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
            if let Err(e) = build_part(&source, &pages).and_then(|doc| write_new(&doc, &path)) {
                // tudo ou nada: sem partes soltas na pasta
                for done in &written {
                    let _ = std::fs::remove_file(done);
                }
                return Err(e);
            }
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
        // arquivo restrito: copiar as páginas para um arquivo sem senha tiraria a proteção
        if !crate::security::pages_can_be_copied(&src, password.as_deref())? {
            return Err(crate::security::restricted_source(path));
        }
        let n = src.page_count()?;
        let at = out.page_count()? as u32;
        let outline = src.outlines()?;
        copy_pages(&mut out, &src, &(0..n).collect::<Vec<_>>(), at as i32)?;
        marks.push(Outline {
            title: stem_of(path, "Documento"),
            uri: None,
            dest: Some(LinkDestination {
                loc: Location {
                    chapter: 0,
                    page_in_chapter: at,
                    page_number: at,
                },
                kind: mupdf::DestinationKind::Fit,
            }),
            down: shifted(&outline, 0, n as u32, at, true),
        });
    }
    out.set_outlines(&marks)?;
    write_new(&out, dest)
}
