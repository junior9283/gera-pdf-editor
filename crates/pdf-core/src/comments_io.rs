//! Exportar e importar comentários, como o Acrobat: arquivo de dados FDF ou XFDF, ou os
//! comentários de outro PDF. O FDF é escrito e lido pelo MuPDF (é sintaxe de PDF com outro
//! cabeçalho); as aparências vão junto.
use crate::doc::{deleted, PdfDoc};
use crate::error::{refused, Result};
use crate::pdf_obj::{get, name_of};
use crate::types::AnnotInfo;
use crate::xfdf::{self, XfdfAnnot};
use mupdf::pdf::{PdfDocument, PdfGraftMap, PdfObject, PdfWriteOptions};
use std::collections::HashMap;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
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
        let bytes = match format {
            CommentFormat::Fdf => self.fdf_bytes(&picked, path)?,
            CommentFormat::Xfdf => {
                let name = path.file_name().map_or_else(String::new, |n| {
                    n.to_string_lossy().replace(".xfdf", ".pdf")
                });
                crate::xfdf::write(&self.inner, &picked, &name)?.into_bytes()
            }
        };
        write_bytes_atomic(path, &bytes)?;
        Ok(picked.len())
    }

    /// Comentários a exportar, na ordem das páginas.
    fn comments_to_export(&self, only: Option<&[(i32, i32)]>) -> Result<Vec<AnnotInfo>> {
        let mut out = Vec::new();
        for page in 0..self.page_count()? {
            let annots = self.annotations(page)?;
            match only {
                None => out.extend(annots),
                Some(sel) => {
                    let ids: Vec<i32> = sel
                        .iter()
                        .filter(|(p, _)| *p == page)
                        .map(|&(_, id)| id)
                        .collect();
                    let keep = deleted(&annots, &ids);
                    out.extend(annots.into_iter().filter(|a| keep.contains(&a.id)));
                }
            }
        }
        Ok(out)
    }

    fn fdf_bytes(&self, picked: &[AnnotInfo], path: &Path) -> Result<Vec<u8>> {
        let mut out = PdfDocument::new();
        let mut map = out.new_graft_map()?;
        let mut refs: HashMap<i32, PdfObject> = HashMap::new();
        let mut list = out.new_array()?;
        for page in 0..self.page_count()? {
            let on_page: Vec<&AnnotInfo> = picked.iter().filter(|a| a.page == page).collect();
            if on_page.is_empty() {
                continue;
            }
            let page_ref = self.inner.load_pdf_page(page)?;
            for annot in page_ref.annotations() {
                let id = annot.object().as_indirect()?;
                if !on_page.iter().any(|a| a.id == id) {
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
        let name = path.file_name().map_or_else(String::new, |n| {
            n.to_string_lossy()
                .replace(".fdf", ".pdf")
                .replace(".FDF", ".pdf")
        });
        fdf.dict_put("F", PdfObject::new_string(&name)?)?;
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
        let (items, mut skipped) = incoming(&src)?;
        if items.is_empty() {
            return Err(refused("o arquivo não tem comentários"));
        }
        let mut report = self.with_snapshot(|d| d.add_incoming(&src, items))?;
        report.skipped += std::mem::take(&mut skipped);
        Ok(report)
    }

    fn add_incoming(&mut self, src: &PdfDocument, items: Vec<Incoming>) -> Result<ImportReport> {
        let pages = self.page_count()?;
        let start = self.inner.count_objects()?;
        let mut map = self.inner.new_graft_map()?;
        let mut added: HashMap<i32, PdfObject> = HashMap::new();
        let mut pending = Vec::new();
        let mut report = ImportReport::default();
        let mut page_objs: HashMap<i32, PdfObject> = HashMap::new();
        for item in items {
            if !(0..pages).contains(&item.page) {
                report.skipped += 1;
                continue;
            }
            let mut copy = graft(&mut map, src, &item.dict)?;
            if self.inner.count_objects()?.saturating_sub(start) > MAX_IMPORTED_OBJECTS {
                return Err(refused("os comentários do arquivo são complexos demais"));
            }
            let page_obj = match page_objs.get(&item.page) {
                Some(p) => p.clone(),
                None => {
                    let p = self.inner.load_pdf_page(item.page)?.object();
                    page_objs.insert(item.page, p.clone());
                    p
                }
            };
            copy.dict_put("P", page_obj.clone())?;
            let r = self.inner.add_object(&copy)?;
            push_annot(&self.inner, page_obj, r.clone())?;
            if let (Some(num), Some(parent)) = (item.num, item.parent) {
                pending.push((r.clone(), parent));
                added.insert(num, r);
            } else if let Some(num) = item.num {
                added.insert(num, r);
            }
            report.added += 1;
        }
        for (child, parent) in pending {
            let mut child = child;
            match added.get(&parent) {
                Some(p) => child.dict_put("IRT", p.clone())?,
                // o pai não veio: o comentário fica solto (sem grupo)
                None => child.dict_delete("RT")?,
            }
        }
        Ok(report)
    }
}

impl PdfDoc {
    fn add_xfdf(&mut self, items: Vec<XfdfAnnot>) -> Result<ImportReport> {
        let pages = self.page_count()?;
        let mut report = ImportReport::default();
        let mut by_name: HashMap<String, PdfObject> = HashMap::new();
        let mut parents: HashMap<String, String> = HashMap::new();
        let mut pending = Vec::new();
        let mut fresh: Vec<(i32, i32)> = Vec::new();
        let mut page_objs: HashMap<i32, PdfObject> = HashMap::new();
        for item in items {
            let rect = item
                .attr("rect")
                .and_then(xfdf::numbers)
                .filter(|r| r.len() == 4);
            let (Some(rect), true) = (rect, (0..pages).contains(&item.page)) else {
                report.skipped += 1;
                continue;
            };
            let mut dict = xfdf_dict(&self.inner, &item, &rect)?;
            let page_obj = match page_objs.get(&item.page) {
                Some(p) => p.clone(),
                None => {
                    let p = self.inner.load_pdf_page(item.page)?.object();
                    page_objs.insert(item.page, p.clone());
                    p
                }
            };
            dict.dict_put("P", page_obj.clone())?;
            let r = self.inner.add_object(&dict)?;
            push_annot(&self.inner, page_obj, r.clone())?;
            fresh.push((item.page, r.as_indirect()?));
            if let Some(name) = item.attr("name").filter(|n| !n.is_empty()) {
                by_name.insert(name.to_owned(), r.clone());
                if let Some(parent) = item.attr("inreplyto").filter(|p| !p.is_empty()) {
                    parents.insert(name.to_owned(), parent.to_owned());
                    pending.push((r, name.to_owned(), parent.to_owned()));
                }
            }
            report.added += 1;
        }
        for (mut child, name, parent) in pending {
            // pai que não veio, ou em ciclo (A responde a B que responde a A): fica solto
            match by_name.get(&parent) {
                Some(p) if !in_cycle(&parents, &name) => child.dict_put("IRT", p.clone())?,
                _ => child.dict_delete("RT")?,
            }
        }
        // o XFDF não traz aparência: o MuPDF a faz
        for page in fresh
            .iter()
            .map(|(p, _)| *p)
            .collect::<std::collections::BTreeSet<_>>()
        {
            let page_ref = self.inner.load_pdf_page(page)?;
            for mut a in page_ref.annotations() {
                if fresh.contains(&(page, a.object().as_indirect()?)) {
                    a.update()?;
                }
            }
        }
        Ok(report)
    }
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

/// Dicionário da anotação a partir do elemento do XFDF (`rect` já conferido).
fn xfdf_dict(doc: &PdfDocument, item: &XfdfAnnot, rect: &[f32]) -> Result<PdfObject> {
    let mut d = doc.new_dict()?;
    let reals = |v: &[f32]| -> Result<PdfObject> {
        let mut a = doc.new_array()?;
        for &x in v {
            a.array_push(PdfObject::new_real(x)?)?;
        }
        Ok(a)
    };
    let name = |n: &str| PdfObject::new_name(n);
    d.dict_put("Type", name("Annot")?)?;
    d.dict_put("Subtype", name(item.subtype)?)?;
    d.dict_put("Rect", reals(rect)?)?;
    for (attr, key) in [("color", "C"), ("interior-color", "IC")] {
        if let Some(c) = item.attr(attr).and_then(xfdf::parse_color) {
            d.dict_put(key, reals(&c)?)?;
        }
    }
    if let Some(o) = item
        .attr("opacity")
        .and_then(|o| o.trim().parse::<f32>().ok())
    {
        if o.is_finite() {
            d.dict_put("CA", PdfObject::new_real(o.clamp(0.0, 1.0))?)?;
        }
    }
    for (attr, key) in [
        ("title", "T"),
        ("date", "M"),
        ("creationdate", "CreationDate"),
        ("subject", "Subj"),
        ("name", "NM"),
    ] {
        if let Some(v) = item.attr(attr) {
            d.dict_put(key, PdfObject::new_string(v)?)?;
        }
    }
    if let Some(words) = item.attr("flags") {
        let f = xfdf::flag_bits(words);
        if f != 0 {
            d.dict_put("F", PdfObject::new_int(f)?)?;
        }
    }
    if item.attr("replyType") == Some("group") {
        d.dict_put("RT", name("Group")?)?;
    }
    for (attr, key) in [("icon", "Name"), ("intent", "IT")] {
        if let Some(v) = item.attr(attr).filter(|v| xfdf::plain_name(v)) {
            d.dict_put(key, name(v)?)?;
        }
    }
    if let Some(w) = item
        .attr("width")
        .and_then(|w| w.trim().parse::<f32>().ok())
    {
        if w.is_finite() {
            let mut bs = doc.new_dict()?;
            bs.dict_put("W", PdfObject::new_real(w.clamp(0.0, 100.0))?)?;
            d.dict_put("BS", bs)?;
        }
    }
    let list = |attr: &str, step: usize| {
        item.attr(attr)
            .and_then(xfdf::numbers)
            .filter(|v| !v.is_empty() && v.len() % step == 0)
    };
    if let Some(q) = list("coords", 8) {
        d.dict_put("QuadPoints", reals(&q)?)?;
    }
    if let Some(v) = list("vertices", 2) {
        d.dict_put("Vertices", reals(&v)?)?;
    }
    if let Some(cl) = list("callout", 2).filter(|c| c.len() <= 6) {
        d.dict_put("CL", reals(&cl)?)?;
    }
    if let (Some(a), Some(b)) = (list("start", 2), list("end", 2)) {
        if a.len() == 2 && b.len() == 2 {
            d.dict_put("L", reals(&[a[0], a[1], b[0], b[1]])?)?;
        }
    }
    if let (Some(h), Some(t)) = (item.attr("head"), item.attr("tail")) {
        if xfdf::plain_name(h) && xfdf::plain_name(t) {
            let mut le = doc.new_array()?;
            le.array_push(name(h)?)?;
            le.array_push(name(t)?)?;
            d.dict_put("LE", le)?;
        }
    }
    if !item.gestures.is_empty() {
        let mut ink = doc.new_array()?;
        for g in &item.gestures {
            if let Some(v) = xfdf::numbers(g).filter(|v| v.len() >= 2 && v.len() % 2 == 0) {
                ink.array_push(reals(&v)?)?;
            }
        }
        d.dict_put("InkList", ink)?;
    }
    if let Some(text) = &item.contents {
        d.dict_put("Contents", PdfObject::new_string(text)?)?;
    }
    if let Some(da) = &item.da {
        d.dict_put("DA", PdfObject::new_string(da)?)?;
    }
    Ok(d)
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

/// Grava os bytes num temporário ao lado e troca de nome (nunca deixa o arquivo pela metade).
fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("gera-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}
