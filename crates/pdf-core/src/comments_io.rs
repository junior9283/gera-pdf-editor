//! Exportar e importar comentários, como o Acrobat: arquivo de dados FDF ou XFDF, ou os
//! comentários de outro PDF. O FDF é escrito e lido pelo MuPDF (é sintaxe de PDF com outro
//! cabeçalho); as aparências vão junto.
use crate::doc::{with_group_and_replies, PdfDoc};
use crate::error::{refused, Result};
use crate::pdf_obj::{get, name_of};
use crate::types::AnnotInfo;
use crate::xfdf::{self, XfdfAnnot};
use mupdf::pdf::{PdfDocument, PdfGraftMap, PdfObject, PdfWriteOptions};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

/// Formato do arquivo de dados de comentários.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentFormat {
    Fdf,
    Xfdf,
}

impl CommentFormat {
    /// Pela extensão (`.fdf` ou `.xfdf`, sem diferença de maiúsculas).
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "fdf" => Some(Self::Fdf),
            "xfdf" => Some(Self::Xfdf),
            _ => None,
        }
    }
}

/// Resultado de importar: comentários acrescentados e ignorados (página inexistente, tipo
/// desconhecido ou acima do teto).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
    /// Páginas que ganharam comentários (em ordem).
    pub pages: Vec<i32>,
}

/// Põe dicionários de anotação nas páginas (cada página carregada uma vez) e anota quais mudaram.
#[derive(Default)]
struct Placer {
    page_objs: HashMap<i32, PdfObject>,
    pages: BTreeSet<i32>,
}

impl Placer {
    /// Acrescenta `dict` à página `page`; devolve a referência da anotação nova.
    fn put(&mut self, doc: &mut PdfDocument, page: i32, mut dict: PdfObject) -> Result<PdfObject> {
        let page_obj = match self.page_objs.get(&page) {
            Some(p) => p.clone(),
            None => {
                let p = doc.load_pdf_page(page)?.object();
                self.page_objs.insert(page, p.clone());
                p
            }
        };
        dict.dict_put("P", page_obj.clone())?;
        let r = doc.add_object(&dict)?;
        push_annot(doc, page_obj, r.clone())?;
        self.pages.insert(page);
        Ok(r)
    }

    fn report(self, added: usize, skipped: usize) -> ImportReport {
        ImportReport {
            added,
            skipped,
            pages: self.pages.into_iter().collect(),
        }
    }
}

/// Liga a resposta (ou o membro do grupo) ao pai; sem pai, fica solta (sem grupo).
fn link_parent(mut child: PdfObject, parent: Option<&PdfObject>) -> Result<()> {
    match parent {
        Some(p) => child.dict_put("IRT", p.clone())?,
        None => child.dict_delete("RT")?,
    }
    Ok(())
}

/// O PDF de que os comentários são (/F do FDF, href do XFDF): o nome do arquivo com `.pdf`.
fn source_name(path: &Path) -> String {
    path.with_extension("pdf")
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Teto de comentários lidos de um arquivo.
const MAX_IMPORTED: usize = 5000;
/// Teto de objetos trazidos com os comentários (aparências e recursos).
const MAX_IMPORTED_OBJECTS: u32 = 200_000;
/// Maior arquivo de dados aceito para importar.
const MAX_IMPORT_BYTES: u64 = 200 * 1024 * 1024;
/// Chaves que não vêm junto: as que ligam a anotação ao documento de origem (página, popup,
/// pai, estrutura, nome) e as ações (/A, /AA: um comentário importado nunca roda nada).
const LINK_KEYS: [&str; 8] = [
    "P",
    "Popup",
    "IRT",
    "Parent",
    "StructParent",
    "NM",
    "A",
    "AA",
];
/// Tipos que não são comentários.
const NOT_COMMENTS: [&str; 3] = ["Popup", "Link", "Widget"];

/// Comentário lido de outro arquivo: página, dicionário, número do objeto e do pai (/IRT).
struct Incoming {
    page: i32,
    dict: PdfObject,
    num: Option<i32>,
    parent: Option<i32>,
}

impl PdfDoc {
    /// Grava os comentários (todos, ou só `only` com as respostas e o grupo de cada um) num
    /// arquivo de dados. Devolve quantos foram gravados.
    pub fn export_comments(
        &self,
        path: &Path,
        format: CommentFormat,
        only: Option<&[(i32, i32)]>,
    ) -> Result<usize> {
        let picked = self.comments_to_export(only)?;
        if picked.is_empty() {
            return Err(refused("não há comentários para exportar"));
        }
        let name = source_name(path);
        let bytes = match format {
            CommentFormat::Fdf => self.fdf_bytes(&picked, &name)?,
            CommentFormat::Xfdf => crate::xfdf::write(&self.inner, &picked, &name)?.into_bytes(),
        };
        crate::doc::write_bytes_atomic(path, &bytes)?;
        Ok(picked.len())
    }

    /// Comentários a exportar, na ordem das páginas.
    fn comments_to_export(&self, only: Option<&[(i32, i32)]>) -> Result<Vec<AnnotInfo>> {
        let mut out = Vec::new();
        let Some(sel) = only else {
            for page in 0..self.page_count()? {
                out.extend(self.annotations(page)?);
            }
            return Ok(out);
        };
        // só as páginas da seleção
        let mut by_page: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for &(p, id) in sel {
            by_page.entry(p).or_default().push(id);
        }
        for (page, ids) in by_page {
            let annots = self.annotations(page)?;
            let keep: HashSet<i32> = with_group_and_replies(&annots, &ids).into_iter().collect();
            out.extend(annots.into_iter().filter(|a| keep.contains(&a.id)));
        }
        Ok(out)
    }

    fn fdf_bytes(&self, picked: &[AnnotInfo], name: &str) -> Result<Vec<u8>> {
        let mut out = PdfDocument::new();
        let mut map = out.new_graft_map()?;
        let mut refs: HashMap<i32, PdfObject> = HashMap::new();
        let mut list = out.new_array()?;
        for (page, ids) in by_page(picked) {
            let page_ref = self.inner.load_pdf_page(page)?;
            for annot in page_ref.annotations() {
                let id = annot.object().as_indirect()?;
                if !ids.contains(&id) {
                    continue;
                }
                let Some(dict) = annot.object().resolve()? else {
                    continue;
                };
                let mut copy = map.graft_object(&unlinked(&self.inner, &dict)?)?;
                copy.dict_put("Page", PdfObject::new_int(page)?)?;
                let r = out.add_object(&copy)?;
                list.array_push(r.clone())?;
                refs.insert(id, r);
            }
        }
        // respostas e grupos apontam para o pai exportado
        for a in picked {
            let parent = a.reply_to.or(a.group);
            if let (Some(child), Some(p)) = (refs.get(&a.id), parent.and_then(|p| refs.get(&p))) {
                child.clone().dict_put("IRT", p.clone())?;
            }
        }
        let mut fdf = out.new_dict()?;
        fdf.dict_put("Annots", list)?;
        fdf.dict_put("F", PdfObject::new_string(name)?)?;
        let mut root = out.new_dict()?;
        root.dict_put("FDF", fdf)?;
        let root = out.add_object(&root)?;
        out.trailer()?.dict_put("Root", root)?;
        let mut opts = PdfWriteOptions::default();
        opts.set_garbage_level(1); // tira o catálogo vazio do documento novo
        let mut bytes = Vec::new();
        out.write_to_with_options(&mut bytes, opts)?;
        // o mesmo arquivo com o cabeçalho do FDF (mesmo tamanho: a tabela xref continua certa)
        if bytes.starts_with(b"%PDF-") && bytes.len() >= 8 {
            bytes[..8].copy_from_slice(b"%FDF-1.2");
        }
        Ok(bytes)
    }

    /// Acrescenta os comentários de um FDF, XFDF ou PDF às páginas de mesmo número, numa etapa
    /// do desfazer.
    pub fn import_comments(&mut self, path: &Path) -> Result<ImportReport> {
        if std::fs::metadata(path)?.len() > MAX_IMPORT_BYTES {
            return Err(refused("o arquivo é grande demais"));
        }
        let mut bytes = std::fs::read(path)?;
        if crate::xfdf::looks_like_xfdf(&bytes) {
            let (items, skipped) = crate::xfdf::read(&bytes)?;
            let mut report = self.with_snapshot(|d| d.add_xfdf(items))?;
            report.skipped += skipped;
            return Ok(report);
        }
        if bytes.starts_with(b"%FDF-") && bytes.len() >= 8 {
            bytes[..8].copy_from_slice(b"%PDF-1.7");
        }
        let src = PdfDocument::from_bytes(&bytes)
            .map_err(|_| refused("o arquivo não é um FDF, XFDF ou PDF válido"))?;
        let (items, skipped) = incoming(&src)?;
        if items.is_empty() {
            return Err(refused("o arquivo não tem comentários"));
        }
        let mut report = self.with_snapshot(|d| d.add_incoming(&src, items))?;
        report.skipped += skipped;
        Ok(report)
    }

    fn add_incoming(&mut self, src: &PdfDocument, items: Vec<Incoming>) -> Result<ImportReport> {
        let pages = self.page_count()?;
        let start = self.inner.count_objects()?;
        let mut map = self.inner.new_graft_map()?;
        let mut added: HashMap<i32, PdfObject> = HashMap::new();
        let mut pending = Vec::new();
        let (mut placer, mut count, mut skipped) = (Placer::default(), 0, 0);
        for item in items {
            if !(0..pages).contains(&item.page) {
                skipped += 1;
                continue;
            }
            let copy = graft(&mut map, src, &item.dict)?;
            if self.inner.count_objects()?.saturating_sub(start) > MAX_IMPORTED_OBJECTS {
                return Err(refused("os comentários do arquivo são complexos demais"));
            }
            let r = placer.put(&mut self.inner, item.page, copy)?;
            if let Some(num) = item.num {
                if let Some(parent) = item.parent {
                    pending.push((r.clone(), parent));
                }
                added.insert(num, r);
            }
            count += 1;
        }
        for (child, parent) in pending {
            link_parent(child, added.get(&parent))?;
        }
        Ok(placer.report(count, skipped))
    }
}

impl PdfDoc {
    fn add_xfdf(&mut self, items: Vec<XfdfAnnot>) -> Result<ImportReport> {
        let pages = self.page_count()?;
        let (mut placer, mut count, mut skipped) = (Placer::default(), 0, 0);
        let mut by_name: HashMap<String, PdfObject> = HashMap::new();
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut pending = Vec::new();
        let mut fresh: HashSet<(i32, i32)> = HashSet::new();
        for item in items {
            let rect = item
                .attr("rect")
                .and_then(xfdf::numbers)
                .filter(|r| r.len() == 4);
            let (Some(rect), true) = (rect, (0..pages).contains(&item.page)) else {
                skipped += 1;
                continue;
            };
            let dict = xfdf::annot_dict(&self.inner, &item, &rect)?;
            let r = placer.put(&mut self.inner, item.page, dict)?;
            fresh.insert((item.page, r.as_indirect()?));
            if let Some(name) = item.attr("name").filter(|n| !n.is_empty()) {
                by_name.insert(name.to_owned(), r.clone());
                if let Some(parent) = item.attr("inreplyto").filter(|p| !p.is_empty()) {
                    parents.insert(name.to_owned(), parent.to_owned());
                    pending.push((r, name.to_owned(), parent.to_owned()));
                }
            }
            count += 1;
        }
        for (child, name, parent) in pending {
            // pai que não veio, ou em ciclo (A responde a B que responde a A): fica solto
            let parent = by_name.get(&parent).filter(|_| !in_cycle(&parents, &name));
            link_parent(child, parent)?;
        }
        // o XFDF não traz aparência: o MuPDF a faz
        for &page in &placer.pages {
            let page_ref = self.inner.load_pdf_page(page)?;
            for mut a in page_ref.annotations() {
                if fresh.contains(&(page, a.object().as_indirect()?)) {
                    a.update()?;
                }
            }
        }
        Ok(placer.report(count, skipped))
    }
}

/// Os ids de `picked` por página.
fn by_page(picked: &[AnnotInfo]) -> BTreeMap<i32, HashSet<i32>> {
    let mut out: BTreeMap<i32, HashSet<i32>> = BTreeMap::new();
    for a in picked {
        out.entry(a.page).or_default().insert(a.id);
    }
    out
}

/// `name` está num ciclo de respostas?
fn in_cycle(parents: &HashMap<String, String>, name: &str) -> bool {
    let mut at = name;
    for _ in 0..=parents.len() {
        match parents.get(at) {
            Some(p) if p == name => return true,
            Some(p) => at = p,
            None => return false,
        }
    }
    true
}

/// Os comentários do FDF (/FDF /Annots com /Page) ou das páginas do PDF.
fn incoming(src: &PdfDocument) -> Result<(Vec<Incoming>, usize)> {
    let mut items = Vec::new();
    let mut skipped = 0;
    let mut take = |page: i32, r: PdfObject| -> Result<()> {
        let Some(dict) = r.resolve()?.filter(|d| d.is_dict().unwrap_or(false)) else {
            return Ok(());
        };
        let subtype = get(&dict, "Subtype").and_then(|s| name_of(&s));
        if subtype.is_none_or(|s| NOT_COMMENTS.contains(&s.as_str())) {
            return Ok(());
        }
        if items.len() >= MAX_IMPORTED {
            skipped += 1;
            return Ok(());
        }
        let num = r.is_indirect()?.then(|| r.as_indirect()).transpose()?;
        let parent = match dict.get_dict("IRT")? {
            Some(p) if p.is_indirect()? => Some(p.as_indirect()?),
            _ => None,
        };
        items.push(Incoming {
            page,
            dict,
            num,
            parent,
        });
        Ok(())
    };
    let root = src.trailer()?.get_dict("Root")?;
    if let Some(list) = root
        .as_ref()
        .and_then(|r| get(r, "FDF"))
        .and_then(|f| get(&f, "Annots"))
    {
        for k in 0..list.len()? as i32 {
            if let Some(r) = list.get_array(k)? {
                let page = r
                    .resolve()?
                    .and_then(|d| get(&d, "Page"))
                    .and_then(|p| p.as_int().ok())
                    .unwrap_or(-1);
                take(page, r)?;
            }
        }
    } else {
        for page in 0..src.page_count()? {
            let page_obj = src.load_pdf_page(page)?.object();
            if let Some(list) = get(&page_obj, "Annots").filter(|l| l.is_array().unwrap_or(false)) {
                for k in 0..list.len()? as i32 {
                    if let Some(r) = list.get_array(k)? {
                        take(page, r)?;
                    }
                }
            }
        }
    }
    Ok((items, skipped))
}

/// Cópia rasa do dicionário sem as ligações com o documento de origem.
fn unlinked(doc: &PdfDocument, dict: &PdfObject) -> Result<PdfObject> {
    let mut out = doc.new_dict()?;
    for i in 0..dict.dict_len()? as i32 {
        if let (Some(k), Some(v)) = (dict.get_dict_key(i)?, dict.get_dict_val(i)?) {
            let link = name_of(&k).is_some_and(|n| LINK_KEYS.contains(&n.as_str()));
            if !link {
                out.dict_put(k, v)?;
            }
        }
    }
    Ok(out)
}

/// O dicionário (sem ligações) enxertado neste documento, com o /Page do FDF fora.
fn graft(map: &mut PdfGraftMap, src: &PdfDocument, dict: &PdfObject) -> Result<PdfObject> {
    let mut copy = map.graft_object(&unlinked(src, dict)?)?;
    copy.dict_delete("Page")?;
    Ok(copy)
}

/// Acrescenta `annot` ao /Annots da página.
pub(crate) fn push_annot(
    doc: &PdfDocument,
    mut page_obj: PdfObject,
    annot: PdfObject,
) -> Result<()> {
    match page_obj.get_dict("Annots")? {
        Some(mut list) if list.is_array()? => list.array_push(annot)?,
        _ => {
            let mut list = doc.new_array()?;
            list.array_push(annot)?;
            page_obj.dict_put("Annots", list)?;
        }
    }
    Ok(())
}
