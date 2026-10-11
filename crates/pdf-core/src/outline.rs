//! Marcadores editáveis (painel Marcadores do Acrobat): a árvore `/Outlines` é lida para a
//! memória (com proteção contra ciclos), a operação é aplicada nela e as ligações (`/First`,
//! `/Last`, `/Next`, `/Prev`, `/Parent`, `/Count`) são regravadas. Cada item mantém o próprio
//! dicionário (e chaves como `/A` e `/SE`).
use crate::doc::PdfDoc;
use crate::error::{refused, PdfError, Result};
use crate::pdf_obj::{get, name_of, text};
use crate::types::{OutlineNode, OutlineOp};
use mupdf::pdf::PdfObject;
use mupdf::Matrix;
use std::collections::HashSet;

/// Tetos da leitura (árvores enormes ou maliciosas).
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const MAX_TITLE_CHARS: usize = 512;

/// Página e ponto de um destino.
type Target = (Option<i32>, Option<(f32, f32)>);
/// Matriz e altura de cada página já carregada (destinos dos marcadores).
type PageCache = std::collections::HashMap<i32, (Matrix, f32)>;

/// Item da árvore em memória.
struct Item {
    obj: PdfObject,
    id: i32,
    open: bool,
    kids: Vec<Item>,
}

fn num(obj: Option<PdfObject>) -> Option<f32> {
    obj.and_then(|o| o.as_float().ok())
        .filter(|v| v.is_finite())
}

/// Irmãos a partir de `first` (e os filhos de cada um), uma vez cada objeto.
fn read_kids(
    first: Option<PdfObject>,
    depth: usize,
    visited: &mut HashSet<i32>,
    count: &mut usize,
) -> Vec<Item> {
    let mut out = Vec::new();
    let mut node = first;
    while let Some(n) = node {
        let Ok(id) = n.as_indirect() else { break };
        if id == 0 || *count >= MAX_NODES || !visited.insert(id) {
            break;
        }
        *count += 1;
        let kids = if depth < MAX_DEPTH {
            read_kids(get(&n, "First"), depth + 1, visited, count)
        } else {
            Vec::new()
        };
        let open = get(&n, "Count")
            .and_then(|c| c.as_int().ok())
            .is_some_and(|c| c > 0);
        node = get(&n, "Next");
        out.push(Item {
            obj: n,
            id,
            open,
            kids,
        });
    }
    out
}

/// Itens visíveis com tudo aberto conforme o estado de cada um.
fn visible(items: &[Item]) -> i32 {
    items
        .iter()
        .map(|i| 1 + if i.open { visible(&i.kids) } else { 0 })
        .sum()
}

fn find_mut(items: &mut [Item], id: i32) -> Option<&mut Item> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_mut(&mut item.kids, id) {
            return Some(found);
        }
    }
    None
}

fn contains(items: &[Item], id: i32) -> bool {
    items.iter().any(|i| i.id == id || contains(&i.kids, id))
}

/// Tira o item `id` (com os filhos) da árvore.
fn detach(items: &mut Vec<Item>, id: i32) -> Option<Item> {
    if let Some(pos) = items.iter().position(|i| i.id == id) {
        return Some(items.remove(pos));
    }
    items.iter_mut().find_map(|i| detach(&mut i.kids, id))
}

/// Põe `item` entre os filhos de `parent` (ou no topo), depois de `after` (ou no começo).
fn insert(
    items: &mut Vec<Item>,
    parent: Option<i32>,
    after: Option<i32>,
    item: Item,
) -> Result<()> {
    let siblings = match parent {
        None => items,
        Some(p) => {
            &mut find_mut(items, p)
                .ok_or_else(|| refused("marcador pai não encontrado"))?
                .kids
        }
    };
    let at = match after {
        None => 0,
        Some(a) => {
            siblings
                .iter()
                .position(|i| i.id == a)
                .ok_or_else(|| refused("marcador de referência não encontrado"))?
                + 1
        }
    };
    siblings.insert(at, item);
    Ok(())
}

/// Regrava as ligações dos irmãos `items`, filhos de `parent`.
fn write_kids(parent: &mut PdfObject, items: &[Item]) -> Result<()> {
    let Some((first, last)) = items.first().zip(items.last()) else {
        parent.dict_delete("First")?;
        parent.dict_delete("Last")?;
        return Ok(());
    };
    parent.dict_put("First", first.obj.clone())?;
    parent.dict_put("Last", last.obj.clone())?;
    for (k, item) in items.iter().enumerate() {
        let mut obj = item.obj.clone();
        obj.dict_put("Parent", parent.clone())?;
        match k.checked_sub(1).and_then(|p| items.get(p)) {
            Some(prev) => obj.dict_put("Prev", prev.obj.clone())?,
            None => obj.dict_delete("Prev")?,
        }
        match items.get(k + 1) {
            Some(next) => obj.dict_put("Next", next.obj.clone())?,
            None => obj.dict_delete("Next")?,
        }
        write_kids(&mut obj, &item.kids)?;
        if item.kids.is_empty() {
            obj.dict_delete("Count")?;
        } else {
            let n = visible(&item.kids);
            let count = if item.open { n } else { -n };
            obj.dict_put("Count", PdfObject::new_int(count)?)?;
        }
    }
    Ok(())
}

impl PdfDoc {
    /// Raiz `/Outlines` (se houver) e a árvore lida.
    fn read_outline(&self) -> Result<(Option<PdfObject>, Vec<Item>)> {
        let Some(root) = get(&self.inner.catalog()?, "Outlines") else {
            return Ok((None, Vec::new()));
        };
        let mut visited = HashSet::new();
        if let Ok(id) = root.as_indirect() {
            visited.insert(id);
        }
        let mut count = 0;
        let items = read_kids(get(&root, "First"), 0, &mut visited, &mut count);
        Ok((Some(root), items))
    }

    /// Marcadores em ordem de exibição, com destino e estilo.
    pub fn outline_tree(&self) -> Result<Vec<OutlineNode>> {
        let (_, items) = self.read_outline()?;
        let mut out = Vec::new();
        self.flatten_items(&items, None, 0, &mut out, &mut PageCache::new());
        Ok(out)
    }

    fn flatten_items(
        &self,
        items: &[Item],
        parent: Option<i32>,
        depth: usize,
        out: &mut Vec<OutlineNode>,
        pages: &mut PageCache,
    ) {
        for item in items {
            let title: String = text(&item.obj, "Title")
                .chars()
                .take(MAX_TITLE_CHARS)
                .collect();
            let color = get(&item.obj, "C")
                .filter(|c| c.len().ok() == Some(3))
                .and_then(|c| {
                    let v = |i| num(c.get_array(i).ok().flatten()).map(|x| x.clamp(0.0, 1.0));
                    Some([v(0)?, v(1)?, v(2)?])
                });
            let flags = get(&item.obj, "F")
                .and_then(|f| f.as_int().ok())
                .unwrap_or(0);
            let (page, at) = self.destination(&item.obj, pages).unwrap_or((None, None));
            out.push(OutlineNode {
                id: item.id,
                parent,
                depth,
                title,
                page,
                at,
                open: item.open,
                color,
                bold: flags & 2 != 0,
                italic: flags & 1 != 0,
            });
            self.flatten_items(&item.kids, Some(item.id), depth + 1, out, pages);
        }
    }

    /// Página e ponto (coordenadas do MuPDF) do destino do marcador: `/Dest` ou `/A` GoTo.
    fn destination(&self, item: &PdfObject, pages: &mut PageCache) -> Option<Target> {
        let dest = match get(item, "Dest") {
            Some(d) => d,
            None => {
                let action = get(item, "A")?;
                (get(&action, "S").and_then(|s| name_of(&s)).as_deref() == Some("GoTo"))
                    .then_some(())?;
                get(&action, "D")?
            }
        };
        let (page, array) = self.resolve_dest(&dest)?;
        let (Some(page), Some(dest)) = (page, array) else {
            return Some((page, None));
        };
        let kind = name_of(&dest.get_array(1).ok()??).unwrap_or_default();
        let arg = |i| num(dest.get_array(i).ok().flatten());
        let (x, y) = match kind.as_str() {
            "XYZ" => (arg(2), arg(3)),
            "FitH" | "FitBH" => (None, arg(2)),
            "FitR" => (arg(2), arg(5)),
            _ => (None, None),
        };
        if x.is_none() && y.is_none() {
            return Some((Some(page), None));
        }
        // cada página é carregada uma vez, mesmo com muitos marcadores apontando para ela
        let (ctm, page_h) = match pages.get(&page) {
            Some(known) => known.clone(),
            None => {
                let pdf_page = self.inner.load_pdf_page(page).ok()?;
                let ctm = pdf_page.ctm().ok()?;
                let page_h = pdf_page.bounds().map(|b| b.y1 - b.y0).unwrap_or(0.0);
                pages.entry(page).or_insert((ctm, page_h)).clone()
            }
        };
        let p = mupdf::Point::new(x.unwrap_or(0.0), y.unwrap_or(page_h)).transform(&ctm);
        Some((Some(page), Some((p.x, p.y))))
    }

    /// Aplica a operação nos marcadores (uma etapa do desfazer; abrir e fechar não entram). `Add`
    /// devolve o id do marcador novo.
    pub fn edit_outline(&mut self, op: OutlineOp) -> Result<Option<i32>> {
        self.require_modifiable("alterar os marcadores")?;
        if let OutlineOp::SetOpen { id, open } = op {
            let (root, mut items) = self.read_outline()?;
            let item = find_mut(&mut items, id).ok_or_else(not_found)?;
            item.open = open;
            self.write_outline(root, &items)?;
            return Ok(None);
        }
        self.with_snapshot(|d| d.apply_outline(op))
    }

    fn apply_outline(&mut self, op: OutlineOp) -> Result<Option<i32>> {
        let (root, mut items) = self.read_outline()?;
        let mut created = None;
        match op {
            OutlineOp::Add {
                parent,
                after,
                title,
                page,
                at,
            } => {
                let mut dict = self.inner.new_dict()?;
                dict.dict_put("Title", PdfObject::new_string(&title)?)?;
                let dest = self.dest_array(page, at)?;
                dict.dict_put("Dest", dest)?;
                let obj = self.inner.add_object(&dict)?;
                let id = obj.as_indirect()?;
                insert(
                    &mut items,
                    parent,
                    after,
                    Item {
                        obj,
                        id,
                        open: false,
                        kids: Vec::new(),
                    },
                )?;
                created = Some(id);
            }
            OutlineOp::Rename { id, title } => {
                let item = find_mut(&mut items, id).ok_or_else(not_found)?;
                item.obj.dict_put("Title", PdfObject::new_string(&title)?)?;
            }
            OutlineOp::Delete { id } => {
                detach(&mut items, id).ok_or_else(not_found)?;
            }
            OutlineOp::Move { id, parent, after } => {
                let moving = detach(&mut items, id).ok_or_else(not_found)?;
                if parent == Some(id) || parent.is_some_and(|p| contains(&moving.kids, p)) {
                    return Err(refused("um marcador não pode ir para dentro de si mesmo"));
                }
                insert(&mut items, parent, after, moving)?;
            }
            OutlineOp::Style {
                id,
                color,
                bold,
                italic,
            } => {
                let item = find_mut(&mut items, id).ok_or_else(not_found)?;
                match color {
                    Some(c) => {
                        let mut arr = self.inner.new_array()?;
                        for v in c {
                            arr.array_push(PdfObject::new_real(v.clamp(0.0, 1.0))?)?;
                        }
                        item.obj.dict_put("C", arr)?;
                    }
                    None => item.obj.dict_delete("C")?,
                }
                let flags = i32::from(italic) | (i32::from(bold) << 1);
                if flags == 0 {
                    item.obj.dict_delete("F")?;
                } else {
                    item.obj.dict_put("F", PdfObject::new_int(flags)?)?;
                }
            }
            OutlineOp::SetDest { id, page, at } => {
                let dest = self.dest_array(page, at)?;
                let item = find_mut(&mut items, id).ok_or_else(not_found)?;
                item.obj.dict_delete("A")?;
                item.obj.dict_put("Dest", dest)?;
            }
            OutlineOp::SetOpen { .. } => unreachable!("tratado em edit_outline"),
        }
        self.write_outline(root, &items)?;
        Ok(created)
    }

    /// `[página /XYZ x y null]` (o ponto vem em coordenadas do MuPDF), ou `[página /Fit]`.
    fn dest_array(&self, page: i32, at: Option<(f32, f32)>) -> Result<PdfObject> {
        self.check_page(page)?;
        let mut dest = self.inner.new_array()?;
        dest.array_push(self.inner.find_page(page)?)?;
        match at {
            Some((x, y)) => {
                let ctm = self.inner.load_pdf_page(page)?.ctm()?;
                let inv: Matrix = ctm.invert().unwrap_or(Matrix::IDENTITY);
                let p = mupdf::Point::new(x, y).transform(&inv);
                dest.array_push(PdfObject::new_name("XYZ")?)?;
                dest.array_push(PdfObject::new_real(p.x)?)?;
                dest.array_push(PdfObject::new_real(p.y)?)?;
                dest.array_push(PdfObject::new_null())?;
            }
            None => dest.array_push(PdfObject::new_name("Fit")?)?,
        }
        Ok(dest)
    }

    fn write_outline(&mut self, root: Option<PdfObject>, items: &[Item]) -> Result<()> {
        let mut root = match root {
            Some(r) => r,
            None => {
                let mut dict = self.inner.new_dict()?;
                dict.dict_put("Type", PdfObject::new_name("Outlines")?)?;
                let r = self.inner.add_object(&dict)?;
                self.inner.catalog()?.dict_put("Outlines", r.clone())?;
                r
            }
        };
        write_kids(&mut root, items)?;
        root.dict_put("Count", PdfObject::new_int(visible(items))?)?;
        Ok(())
    }

    /// (título, `/Count`) de cada marcador, em ordem (para os testes conferirem a gravação).
    #[doc(hidden)]
    pub fn outline_counts(&self) -> Vec<(String, i32)> {
        let Ok((_, items)) = self.read_outline() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        fn walk(items: &[Item], out: &mut Vec<(String, i32)>) {
            for i in items {
                let title = get(&i.obj, "Title")
                    .and_then(|t| t.as_string().ok())
                    .unwrap_or_default();
                let count = get(&i.obj, "Count")
                    .and_then(|c| c.as_int().ok())
                    .unwrap_or(0);
                out.push((title, count));
                walk(&i.kids, out);
            }
        }
        walk(&items, &mut out);
        out
    }
}

fn not_found() -> PdfError {
    refused("marcador não encontrado")
}
