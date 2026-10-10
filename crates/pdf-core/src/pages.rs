//! Organizar páginas: girar, excluir, mover, inserir, substituir e recortar no documento em
//! memória, e extrair para outros arquivos. Cada operação guarda antes um instantâneo do
//! documento (desfazer/refazer troca o documento pelo instantâneo).
use crate::annot_edit::SNAPSHOT_BUDGET;
use crate::doc::{open_authenticated, purge, reopen, write_atomic, PdfDoc};
use crate::error::{PdfError, Result};
use crate::types::PageOp;
use mupdf::pdf::{Encryption, PdfDocument, PdfGraftMap, PdfObject, PdfWriteOptions};
use mupdf::Size;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Chaves que fazem a página (o resto do dicionário é da árvore de páginas).
const PAGE_KEYS: [&str; 10] = [
    "Contents",
    "Resources",
    "MediaBox",
    "CropBox",
    "BleedBox",
    "TrimBox",
    "ArtBox",
    "Rotate",
    "UserUnit",
    "Group",
];
/// Destas, as que a página pode herdar da árvore.
const INHERITED: [&str; 4] = ["Resources", "MediaBox", "CropBox", "Rotate"];

fn invalid(msg: &str) -> PdfError {
    PdfError::InvalidPages(msg.into())
}

/// Índices válidos, sem repetição e em ordem.
fn checked(pages: &[i32], count: i32) -> Result<Vec<i32>> {
    let mut v: Vec<i32> = pages.to_vec();
    v.sort_unstable();
    v.dedup();
    if v.is_empty() {
        return Err(invalid("nenhuma página selecionada"));
    }
    if let Some(&bad) = v.iter().find(|&&p| p < 0 || p >= count) {
        return Err(PdfError::PageOutOfRange(bad));
    }
    Ok(v)
}

/// Ordem nova das páginas ao levar `moving` (em ordem) para antes de `to`.
fn moved_order(count: i32, moving: &[i32], to: i32) -> Vec<i32> {
    let rest: Vec<i32> = (0..count).filter(|p| !moving.contains(p)).collect();
    let at = rest.iter().filter(|&&p| p < to).count();
    let mut order = rest;
    order.splice(at..at, moving.iter().copied());
    order
}

/// Nome do tipo de um objeto (/Subtype) ou vazio.
fn name_of(obj: &PdfObject, key: &str) -> Result<String> {
    Ok(match obj.get_dict(key)? {
        Some(o) if o.is_name()? => String::from_utf8_lossy(&o.as_name()?).into_owned(),
        _ => String::new(),
    })
}

/// Link para uma página do próprio documento (/Dest ou /A GoTo): ao copiar a página, o destino
/// ficaria apontando para a árvore de outro documento.
fn is_internal_link(annot: &PdfObject) -> Result<bool> {
    if name_of(annot, "Subtype")? != "Link" {
        return Ok(false);
    }
    if annot.get_dict("Dest")?.is_some() {
        return Ok(true);
    }
    Ok(match annot.get_dict("A")? {
        Some(a) => name_of(&a, "S")? == "GoTo",
        None => false,
    })
}

/// Cópias das anotações de `src_page` para a página `page_obj` de outro documento, com o mesmo
/// mapa de graft (recursos compartilhados uma vez só). Ficam de fora os campos de formulário
/// (dependem do /AcroForm do documento de origem) e os links internos (apontariam para páginas
/// que não vieram). Tira o /P da origem antes do graft — **muda `src_page`**.
fn copied_annots(
    map: &mut PdfGraftMap,
    src_page: &PdfObject,
    page_obj: &PdfObject,
) -> Result<Vec<PdfObject>> {
    let Some(annots) = src_page.get_dict("Annots")? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for i in 0..annots.len()? {
        let Some(mut a) = annots.get_array(i as i32)? else {
            continue;
        };
        if name_of(&a, "Subtype")? == "Widget" || is_internal_link(&a)? {
            continue;
        }
        a.dict_delete("P")?;
        let mut popup = a.get_dict("Popup")?;
        if let Some(p) = popup.as_mut() {
            p.dict_delete("P")?;
        }
        let mut copy = map.graft_object(&a)?;
        copy.dict_put("P", page_obj.clone())?;
        if let Some(mut p) = copy.get_dict("Popup")? {
            p.dict_put("P", page_obj.clone())?;
        }
        out.push(copy);
    }
    Ok(out)
}

/// Uma página de `src` como página nova de `dest` (ainda fora da árvore), com as anotações. Um
/// só mapa para todas as páginas de uma operação: fontes e imagens compartilhadas vêm uma vez.
fn graft_page(
    dest: &mut PdfDocument,
    src: &PdfDocument,
    map: &mut PdfGraftMap,
    s: i32,
) -> Result<PdfObject> {
    let src_page = src.load_pdf_page(s)?.object();
    let mut page = dest.new_dict()?;
    page.dict_put("Type", PdfObject::new_name("Page")?)?;
    for key in PAGE_KEYS {
        let value = if INHERITED.contains(&key) {
            src_page.get_dict_inheritable(key)?
        } else {
            src_page.get_dict(key)?
        };
        if let Some(v) = value {
            page.dict_put(key, map.graft_object(&v)?)?;
        }
    }
    let page_ref = dest.add_object(&page)?;
    let annots = copied_annots(map, &src_page, &page_ref)?;
    if !annots.is_empty() {
        let mut arr = dest.new_array()?;
        for a in annots {
            arr.array_push(a)?;
        }
        page_ref.clone().dict_put("Annots", arr)?;
    }
    Ok(page_ref)
}

/// Copia as páginas `pages` de `src` para antes de `at` em `dest`. **Muda `src`** (ver
/// `copied_annots`): use um documento que pode ser alterado.
pub(crate) fn copy_pages(
    dest: &mut PdfDocument,
    src: &PdfDocument,
    pages: &[i32],
    at: i32,
) -> Result<()> {
    let mut map = dest.new_graft_map()?;
    for (k, &s) in pages.iter().enumerate() {
        let page = graft_page(dest, src, &mut map, s)?;
        dest.insert_page(at + k as i32, &page)?;
    }
    Ok(())
}

/// Tira de `obj` o destino (/Dest ou /A /D) que aponta para uma das páginas `gone`.
fn drop_dest_to(obj: &mut PdfObject, gone: &HashSet<i32>) -> Result<()> {
    let points_to_gone = |d: Option<PdfObject>| -> Result<bool> {
        Ok(match d {
            Some(arr) if arr.is_array()? && arr.len()? > 0 => match arr.get_array(0)? {
                Some(p) if p.is_indirect()? => gone.contains(&p.as_indirect()?),
                _ => false,
            },
            _ => false,
        })
    };
    if points_to_gone(obj.get_dict("Dest")?)? {
        obj.dict_delete("Dest")?;
    }
    if let Some(a) = obj.get_dict("A")? {
        if points_to_gone(a.get_dict("D")?)? {
            obj.dict_delete("A")?;
        }
    }
    Ok(())
}

/// Grava um PDF novo (parte, extração, combinação): sem senha, como o Acrobat faz.
pub(crate) fn write_new(doc: &PdfDocument, path: &Path) -> Result<()> {
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::None);
    write_atomic(doc, path, opts)
}

impl PdfDoc {
    /// Cópia em memória do documento como está (sem as anotações apagadas), para copiar
    /// páginas dela: copiar mexe na origem (ver `copied_annots`).
    pub(crate) fn replica(&self) -> Result<PdfDocument> {
        let snap = self.snapshot()?;
        let source = reopen(&snap.bytes, self.password.as_deref())?;
        purge(&source, &snap.hidden)?;
        Ok(source)
    }

    /// Aplica a operação; em erro o documento volta ao que era e o histórico não muda.
    pub fn apply_pages(&mut self, op: PageOp) -> Result<()> {
        // o que dá para recusar sem tocar no documento (serializar um PDF grande custa)
        if let PageOp::Delete { pages } = &op {
            if checked(pages, self.page_count()?)?.len() as i32 >= self.page_count()? {
                return Err(invalid("o documento precisa ficar com ao menos uma página"));
            }
        }
        let before = self.snapshot()?;
        // as apagadas saem de verdade antes (os índices das páginas vão mudar); o instantâneo
        // as guarda para o desfazer
        let result = self.purge_hidden().and_then(|()| {
            self.history.hidden.clear();
            self.run_page_op(op)
        });
        match result {
            Ok(()) => {
                if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
                    // o desfazer não alcança mais o arquivo salvo: continua havendo o que salvar
                    self.mark_unsaved();
                }
                Ok(())
            }
            Err(e) => {
                self.restore(&before)?;
                Err(e)
            }
        }
    }

    fn run_page_op(&mut self, op: PageOp) -> Result<()> {
        let count = self.page_count()?;
        match op {
            PageOp::Rotate {
                pages,
                quarter_turns,
            } => {
                for p in checked(&pages, count)? {
                    let mut page = self.inner.load_pdf_page(p)?;
                    let r = (page.rotation()? + 90 * quarter_turns).rem_euclid(360);
                    page.set_rotation(r)?;
                }
            }
            PageOp::Delete { pages } => {
                let pages = checked(&pages, count)?;
                let gone: HashSet<i32> = pages
                    .iter()
                    .map(|&p| self.inner.load_pdf_page(p)?.object().as_indirect())
                    .collect::<std::result::Result<_, _>>()?;
                for &p in pages.iter().rev() {
                    self.inner.delete_page(p)?;
                }
                self.drop_dests_to(&gone)?;
            }
            PageOp::Move { pages, to } => {
                let pages = checked(&pages, count)?;
                if !(0..=count).contains(&to) {
                    return Err(PdfError::PageOutOfRange(to));
                }
                // aplica a ordem nova movendo uma página por vez (remover + inserir)
                let order = moved_order(count, &pages, to);
                let mut current: Vec<i32> = (0..count).collect();
                for (k, want) in order.iter().enumerate() {
                    let at = current.iter().position(|p| p == want).unwrap_or(k);
                    if at != k {
                        self.inner.move_page(at, k)?;
                        let p = current.remove(at);
                        current.insert(k, p);
                    }
                }
            }
            PageOp::InsertBlank { at, size } => {
                if !(0..=count).contains(&at) {
                    return Err(PdfError::PageOutOfRange(at));
                }
                self.inner.new_page_at(
                    at,
                    Size {
                        width: size.0,
                        height: size.1,
                    },
                )?;
            }
            PageOp::InsertFrom {
                at,
                path,
                password,
                range,
            } => {
                if !(0..=count).contains(&at) {
                    return Err(PdfError::PageOutOfRange(at));
                }
                let src = open_authenticated(&path, password.as_deref())?;
                if !crate::security::pages_can_be_copied(&src, password.as_deref())? {
                    return Err(crate::security::restricted_source(&path));
                }
                let src_count = src.page_count()?;
                let wanted: Vec<i32> = match range {
                    None => (0..src_count).collect(),
                    Some((a, b)) => checked(&(a.min(b)..=a.max(b)).collect::<Vec<_>>(), src_count)?,
                };
                copy_pages(&mut self.inner, &src, &wanted, at)?;
            }
            PageOp::Replace {
                pages,
                path,
                password,
                from,
            } => {
                // como no Acrobat: troca só o conteúdo; a página (com os comentários, links e
                // marcadores que levam a ela) continua a mesma
                let pages = checked(&pages, count)?;
                let src = open_authenticated(&path, password.as_deref())?;
                if !crate::security::pages_can_be_copied(&src, password.as_deref())? {
                    return Err(crate::security::restricted_source(&path));
                }
                let wanted: Vec<i32> = (from..from + pages.len() as i32).collect();
                checked(&wanted, src.page_count()?)?;
                let mut map = self.inner.new_graft_map()?;
                for (&p, s) in pages.iter().zip(wanted) {
                    let src_page = src.load_pdf_page(s)?.object();
                    let mut page = self.inner.load_pdf_page(p)?.object();
                    for key in PAGE_KEYS {
                        let value = if INHERITED.contains(&key) {
                            src_page.get_dict_inheritable(key)?
                        } else {
                            src_page.get_dict(key)?
                        };
                        match value {
                            Some(v) => page.dict_put(key, map.graft_object(&v)?)?,
                            None => page.dict_delete(key)?,
                        }
                    }
                    let extra = copied_annots(&mut map, &src_page, &page)?;
                    if !extra.is_empty() {
                        let mut arr = match page.get_dict("Annots")? {
                            Some(a) => a,
                            None => {
                                let a = self.inner.new_array()?;
                                page.dict_put("Annots", a)?;
                                page.get_dict("Annots")?.expect("acabou de ser criado")
                            }
                        };
                        for a in extra {
                            arr.array_push(a)?;
                        }
                    }
                }
            }
            PageOp::Crop { pages, margins } => {
                for p in checked(&pages, count)? {
                    self.crop(p, margins)?;
                }
            }
        }
        Ok(())
    }

    /// Recorta a página `p` pelo que se vê: as margens (cima, baixo, esquerda, direita da página
    /// na tela) viram os lados certos do /CropBox, que fica em coordenadas do PDF (sem giro).
    fn crop(&mut self, p: i32, margins: [f32; 4]) -> Result<()> {
        let page = self.inner.load_pdf_page(p)?;
        let rotation = page.rotation()?.rem_euclid(360);
        let mut obj = page.object();
        let read = |key: &str| -> Result<Option<[f32; 4]>> {
            let Some(arr) = obj.get_dict_inheritable(key)? else {
                return Ok(None);
            };
            let mut v = [0.0; 4];
            for (i, slot) in v.iter_mut().enumerate() {
                *slot = match arr.get_array(i as i32)? {
                    Some(n) => n.as_float()?,
                    None => return Ok(None),
                };
            }
            Ok(Some([
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ]))
        };
        let media = read("MediaBox")?.unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = read("CropBox")?.unwrap_or(media);
        let [top, bottom, left, right] = margins.map(|m| m.max(0.0));
        // lados do PDF sem giro (x0, y0 = baixo, x1, y1 = cima) para cada lado da tela
        let [dx0, dy0, dx1, dy1] = match rotation {
            90 => [top, left, bottom, right],
            180 => [right, top, left, bottom],
            270 => [bottom, right, top, left],
            _ => [left, bottom, right, top],
        };
        let r = [
            (crop[0] + dx0).max(media[0]),
            (crop[1] + dy0).max(media[1]),
            (crop[2] - dx1).min(media[2]),
            (crop[3] - dy1).min(media[3]),
        ];
        if r[2] - r[0] < 10.0 || r[3] - r[1] < 10.0 {
            return Err(invalid("as margens não deixam sobrar página"));
        }
        let mut arr = self.inner.new_array()?;
        for v in r {
            arr.array_push(PdfObject::new_real(v)?)?;
        }
        obj.dict_put("CropBox", arr)?;
        Ok(())
    }

    /// Marcadores e links que levavam às páginas excluídas ficam sem destino (senão o MuPDF os
    /// leva a outra página e o objeto da página continua no arquivo).
    fn drop_dests_to(&mut self, gone: &HashSet<i32>) -> Result<()> {
        // marcadores (árvore /Outlines: /First, /Next)
        let mut stack: Vec<PdfObject> = self
            .inner
            .catalog()?
            .get_dict("Outlines")?
            .and_then(|o| o.get_dict("First").ok().flatten())
            .into_iter()
            .collect();
        let mut seen = HashSet::new();
        while let Some(mut item) = stack.pop() {
            if item.is_indirect()? && !seen.insert(item.as_indirect()?) {
                continue; // árvore com ciclo
            }
            drop_dest_to(&mut item, gone)?;
            stack.extend(item.get_dict("Next")?);
            stack.extend(item.get_dict("First")?);
        }
        // links nas páginas que ficaram
        for p in 0..self.page_count()? {
            let Some(annots) = self.inner.load_pdf_page(p)?.object().get_dict("Annots")? else {
                continue;
            };
            for i in 0..annots.len()? {
                if let Some(mut a) = annots.get_array(i as i32)? {
                    drop_dest_to(&mut a, gone)?;
                }
            }
        }
        Ok(())
    }

    /// Grava as páginas em `dest` (ou, com `separate`, uma por arquivo: `nome_N.pdf`, N = número
    /// da página; um nome que já existe ganha " (2)", " (3)"…). As cópias saem sem senha (como o
    /// Acrobat ao extrair), com os comentários. O documento não muda.
    pub fn extract(&self, pages: &[i32], dest: &Path, separate: bool) -> Result<Vec<PathBuf>> {
        let pages = checked(pages, self.page_count()?)?;
        if same_file(dest, &self.path) {
            return Err(invalid("escolha outro arquivo: este é o documento aberto"));
        }
        let groups: Vec<(Vec<i32>, PathBuf)> = if separate {
            let stem = dest
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "páginas".into());
            pages
                .iter()
                .map(|&p| (vec![p], free_name(dest, &format!("{stem}_{}", p + 1))))
                .collect()
        } else {
            vec![(pages, dest.to_path_buf())]
        };
        let source = self.replica()?;
        let mut written = Vec::new();
        for (group, path) in groups {
            let mut out = PdfDocument::new();
            copy_pages(&mut out, &source, &group, 0)?;
            write_new(&out, &path)?;
            written.push(path);
        }
        Ok(written)
    }
}

/// Os dois caminhos são o mesmo arquivo?
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `dir/stem.pdf`, ou `dir/stem (2).pdf`… se já existir.
pub(crate) fn free_name(dest: &Path, stem: &str) -> PathBuf {
    let first = dest.with_file_name(format!("{stem}.pdf"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dest.with_file_name(format!("{stem} ({n}).pdf")))
        .find(|p| !p.exists())
        .expect("algum nome livre")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_order_keeps_relative_order() {
        assert_eq!(moved_order(5, &[0, 2], 5), [1, 3, 4, 0, 2]);
        assert_eq!(moved_order(5, &[3, 4], 1), [0, 3, 4, 1, 2]);
        assert_eq!(moved_order(3, &[1], 0), [1, 0, 2]);
    }
}
