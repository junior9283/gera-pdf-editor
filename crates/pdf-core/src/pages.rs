//! Organizar páginas: girar, excluir, mover, inserir, substituir e recortar no documento em
//! memória, e extrair para outros arquivos. Cada operação guarda antes um instantâneo do
//! documento (desfazer/refazer troca o documento pelo instantâneo).
use crate::annot_edit::SNAPSHOT_BUDGET;
use crate::doc::{open_authenticated, PdfDoc};
use crate::error::{PdfError, Result};
use crate::types::PageOp;
use mupdf::pdf::{
    Encryption, InsertPdfOptions, InsertPosition, PageSelection, PdfDocument, PdfWriteOptions,
};
use mupdf::{Rect, Size};
use std::path::{Path, PathBuf};

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

fn as_usize(v: &[i32]) -> Vec<usize> {
    v.iter().map(|&p| p as usize).collect()
}

impl PdfDoc {
    /// Aplica a operação; em erro o documento volta ao que era e o histórico não muda.
    pub fn apply_pages(&mut self, op: PageOp) -> Result<()> {
        let before = self.snapshot()?;
        // as apagadas saem de verdade antes (os índices das páginas vão mudar); o instantâneo
        // as guarda para o desfazer
        let result = self.purge_hidden().and_then(|()| {
            self.history.hidden.clear();
            self.run_page_op(op)
        });
        match result {
            Ok(()) => {
                self.history.record_snapshot(before, SNAPSHOT_BUDGET);
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
                if pages.len() as i32 >= count {
                    return Err(invalid("o documento precisa ficar com ao menos uma página"));
                }
                self.inner
                    .delete_pages(PageSelection::Pages(as_usize(&pages)))?;
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
                let page = self.inner.new_page_at(
                    at,
                    Size {
                        width: size.0,
                        height: size.1,
                    },
                )?;
                if self.page_count()? == count {
                    // a página foi criada mas não entrou na árvore: insere
                    self.inner.insert_page(at, &page.object())?;
                }
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
                let src_count = src.page_count()?;
                let wanted: Vec<i32> = match range {
                    None => (0..src_count).collect(),
                    Some((a, b)) => checked(&(a.min(b)..=a.max(b)).collect::<Vec<_>>(), src_count)?,
                };
                self.insert_pages(&src, &wanted, at)?;
            }
            PageOp::Replace {
                pages,
                path,
                password,
                from,
            } => {
                let pages = checked(&pages, count)?;
                let src = open_authenticated(&path, password.as_deref())?;
                let wanted: Vec<i32> = (from..from + pages.len() as i32).collect();
                checked(&wanted, src.page_count()?)?;
                for (p, s) in pages.iter().zip(wanted) {
                    self.insert_pages(&src, &[s], *p)?;
                    self.inner.delete_page(p + 1)?;
                }
            }
            PageOp::Crop { pages, margins } => {
                let [top, bottom, left, right] = margins.map(|m| m.max(0.0));
                for p in checked(&pages, count)? {
                    let mut page = self.inner.load_pdf_page(p)?;
                    let b = page.crop_box()?;
                    let r = Rect::new(b.x0 + left, b.y0 + top, b.x1 - right, b.y1 - bottom);
                    if r.x1 - r.x0 < 10.0 || r.y1 - r.y0 < 10.0 {
                        return Err(invalid("as margens não deixam sobrar página"));
                    }
                    page.set_crop_box(r)?;
                }
            }
        }
        Ok(())
    }

    /// Copia as páginas `pages` de `src` para antes de `at`, com as anotações.
    fn insert_pages(&mut self, src: &PdfDocument, pages: &[i32], at: i32) -> Result<()> {
        copy_pages(&mut self.inner, src, pages, at)
    }

    /// Grava as páginas em `dest` (ou, com `separate`, uma por arquivo: `nome_N.pdf`, N = número
    /// da página). As cópias saem sem senha (como o Acrobat ao extrair). O documento não muda.
    pub fn extract(&self, pages: &[i32], dest: &Path, separate: bool) -> Result<Vec<PathBuf>> {
        let pages = checked(pages, self.page_count()?)?;
        let groups: Vec<(Vec<i32>, PathBuf)> = if separate {
            let stem = dest
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "páginas".into());
            pages
                .iter()
                .map(|&p| {
                    (
                        vec![p],
                        dest.with_file_name(format!("{stem}_{}.pdf", p + 1)),
                    )
                })
                .collect()
        } else {
            vec![(pages, dest.to_path_buf())]
        };
        let mut written = Vec::new();
        for (group, path) in groups {
            // a cópia sai de uma réplica em memória: copiar mexe na origem (ver copy_pages)
            let snap = self.snapshot()?;
            let mut source = PdfDocument::from_bytes(&snap.bytes)?;
            if let Some(p) = self.password.as_deref() {
                if source.needs_password()? {
                    source.authenticate(p)?;
                }
            }
            // as apagadas (ainda não salvas) não vão para o arquivo extraído
            for &(page, id) in &snap.hidden {
                if let Ok((mut page_ref, a)) = crate::annot_edit::find(&source, page, id) {
                    page_ref.delete_annotation(a)?;
                }
            }
            let mut out = PdfDocument::new();
            copy_pages(&mut out, &source, &group, 0)?;
            let mut opts = PdfWriteOptions::default();
            opts.set_encryption(Encryption::None);
            opts.set_garbage_level(1);
            let tmp = path.with_extension("pdf.gera-tmp");
            let tmp_str = tmp
                .to_str()
                .ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))?;
            let saved = out
                .save_with_options(tmp_str, opts)
                .map_err(PdfError::from)
                .and_then(|()| std::fs::rename(&tmp, &path).map_err(PdfError::from));
            if let Err(e) = saved {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            written.push(path);
        }
        Ok(written)
    }
}

/// Copia as páginas `pages` de `src` para antes de `at` em `dest`, com as anotações: o graft do
/// MuPDF copia a página sem o /Annots. **Muda `src`** (tira o /P das anotações, senão o graft
/// traria a página de origem inteira): use um documento que pode ser alterado.
fn copy_pages(dest: &mut PdfDocument, src: &PdfDocument, pages: &[i32], at: i32) -> Result<()> {
    let mut map = dest.new_graft_map()?;
    for (k, &s) in pages.iter().enumerate() {
        let to = at + k as i32;
        let target = if to == dest.page_count()? {
            InsertPosition::Append
        } else {
            InsertPosition::Before(to as usize)
        };
        dest.insert_pdf(
            src,
            InsertPdfOptions {
                source_pages: PageSelection::Pages(vec![s as usize]),
                target,
                ..Default::default()
            },
        )?;
        let Some(annots) = src.load_pdf_page(s)?.object().get_dict("Annots")? else {
            continue;
        };
        for_each_annot(&annots, |mut a| a.dict_delete("P").map_err(Into::into))?;
        let copied = map.graft_object(&annots)?;
        let page_obj = dest.load_pdf_page(to)?.object();
        for_each_annot(&copied, |mut a| {
            a.dict_put("P", page_obj.clone()).map_err(Into::into)
        })?;
        let mut page_obj = page_obj;
        page_obj.dict_put("Annots", copied)?;
    }
    Ok(())
}

/// Roda `f` em cada anotação do array e no popup dela.
fn for_each_annot(
    annots: &mupdf::pdf::PdfObject,
    mut f: impl FnMut(mupdf::pdf::PdfObject) -> Result<()>,
) -> Result<()> {
    for i in 0..annots.len()? {
        if let Some(a) = annots.get_array(i as i32)? {
            if let Some(popup) = a.get_dict("Popup")? {
                f(popup)?;
            }
            f(a)?;
        }
    }
    Ok(())
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
