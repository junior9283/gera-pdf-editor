//! Destinos nomeados (painel Destinos do Acrobat): a árvore `/Names /Dests` e o dicionário antigo
//! `/Dests` do catálogo. Criar, renomear, redefinir e excluir são etapas do desfazer; o que é
//! gravado vai sempre para a árvore, como lista `/Names` simples e ordenada.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::outline::PageCache;
use crate::pdf_obj::{get, name_of};
use mupdf::pdf::PdfObject;
use std::collections::HashSet;

/// Tetos da leitura.
const MAX_NAMES: usize = 50_000;
const MAX_DEPTH: usize = 32;
/// Maior nome aceito ao criar ou renomear.
const MAX_NAME_CHARS: usize = 256;

/// Destino nomeado: nome, página (se existir) e ponto (coordenadas do MuPDF).
#[derive(Debug, Clone, PartialEq)]
pub struct DestInfo {
    pub name: String,
    pub page: Option<i32>,
    pub at: Option<(f32, f32)>,
}

/// Edição de um destino.
#[derive(Debug, Clone, PartialEq)]
pub enum DestOp {
    /// Cria na página e no ponto (da vista atual); o nome repetido ganha " (2)".
    Add {
        name: String,
        page: i32,
        at: Option<(f32, f32)>,
    },
    Rename {
        name: String,
        to: String,
    },
    /// Aponta o destino para outra página/ponto ("Definir destino").
    Set {
        name: String,
        page: i32,
        at: Option<(f32, f32)>,
    },
    Remove {
        name: String,
    },
}

/// De onde veio o destino.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Tree,
    OldDict,
}

/// O destino em si: o valor pode ser o array ou um dicionário com `/D`.
fn dest_of(value: &PdfObject) -> PdfObject {
    if value.is_dict().unwrap_or(false) {
        if let Some(d) = get(value, "D") {
            return d;
        }
    }
    value.clone()
}

fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(refused("nome de destino inválido"));
    }
    Ok(name.to_owned())
}

impl PdfDoc {
    /// Todos os destinos: (nome, valor, origem), primeiro os da árvore.
    fn named_dests(&self) -> Vec<(String, PdfObject, Source)> {
        let Ok(catalog) = self.inner.catalog() else {
            return Vec::new();
        };
        let mut tree = Vec::new();
        if let Some(root) = get(&catalog, "Names").and_then(|n| get(&n, "Dests")) {
            tree = crate::pdf_obj::tree_pairs(&root, "Names", MAX_NAMES, MAX_DEPTH)
                .into_iter()
                .filter_map(|(k, v)| Some((k.as_string().ok()?, v)))
                .collect();
        }
        let mut seen: HashSet<String> = tree.iter().map(|(n, _)| n.clone()).collect();
        let mut out: Vec<_> = tree
            .into_iter()
            .map(|(n, v)| (n, v, Source::Tree))
            .collect();
        if let Some(old) = get(&catalog, "Dests").filter(|d| d.is_dict().unwrap_or(false)) {
            let len = old.dict_len().unwrap_or(0).min(MAX_NAMES) as i32;
            for k in 0..len {
                if out.len() >= MAX_NAMES {
                    break;
                }
                let key = old.get_dict_key(k).ok().flatten().and_then(|k| name_of(&k));
                let value = old.get_dict_val(k).ok().flatten();
                if let (Some(key), Some(value)) = (key, value) {
                    if seen.insert(key.clone()) {
                        out.push((key, value, Source::OldDict));
                    }
                }
            }
        }
        out
    }

    /// Destinos nomeados em ordem de nome, com página e ponto.
    pub fn destinations(&self) -> Vec<DestInfo> {
        let mut pages = PageCache::new();
        let mut out: Vec<DestInfo> = self
            .named_dests()
            .into_iter()
            .map(|(name, value, _)| {
                let (page, at) = self
                    .dest_target(&dest_of(&value), &mut pages)
                    .unwrap_or((None, None));
                DestInfo { name, page, at }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Aplica a edição (uma etapa do desfazer). `Add` devolve o nome que o destino recebeu.
    pub fn edit_destination(&mut self, op: DestOp) -> Result<Option<String>> {
        let existing = self.named_dests();
        let exists = |name: &str| existing.iter().any(|(n, _, _)| n == name);
        // confere antes de mexer: nada muda num pedido inválido
        match &op {
            DestOp::Add { page, .. } | DestOp::Set { page, .. } => self.check_page(*page)?,
            _ => {}
        }
        match &op {
            DestOp::Rename { name, .. } | DestOp::Set { name, .. } | DestOp::Remove { name }
                if !exists(name) =>
            {
                return Err(refused("destino não encontrado"));
            }
            _ => {}
        }
        self.require_modifiable("alterar os destinos")?;
        self.with_snapshot(|d| d.apply_destination(op, existing))
    }

    fn apply_destination(
        &mut self,
        op: DestOp,
        existing: Vec<(String, PdfObject, Source)>,
    ) -> Result<Option<String>> {
        let mut tree: Vec<(String, PdfObject)> = existing
            .iter()
            .filter(|(_, _, s)| *s == Source::Tree)
            .map(|(n, v, _)| (n.clone(), v.clone()))
            .collect();
        let taken: Vec<String> = existing.iter().map(|(n, _, _)| n.clone()).collect();
        // o destino editado sai de onde estava (do dicionário antigo, passa para a árvore)
        let mut drop_old = None;
        let mut take_out = |tree: &mut Vec<(String, PdfObject)>, name: &str| {
            tree.retain(|(n, _)| n != name);
            let old = existing
                .iter()
                .any(|(n, _, s)| n == name && *s == Source::OldDict);
            if old {
                drop_old = Some(name.to_owned());
            }
        };
        let mut created = None;
        match op {
            DestOp::Add { name, page, at } => {
                let wanted = valid_name(&name)?;
                let name = crate::attachments::numbered(&wanted, &wanted, "", &taken);
                tree.push((name.clone(), self.dest_array(page, at)?));
                created = Some(name);
            }
            DestOp::Rename { name, to } => {
                let to = valid_name(&to)?;
                if to != name && taken.contains(&to) {
                    return Err(refused("já existe um destino com esse nome"));
                }
                // edit_destination já conferiu que o nome existe
                let value = existing
                    .iter()
                    .find(|(n, _, _)| *n == name)
                    .map(|(_, v, _)| v.clone());
                take_out(&mut tree, &name);
                tree.extend(value.map(|v| (to, v)));
            }
            DestOp::Set { name, page, at } => {
                let dest = self.dest_array(page, at)?;
                take_out(&mut tree, &name);
                tree.push((name, dest));
            }
            DestOp::Remove { name } => take_out(&mut tree, &name),
        }
        let mut catalog = self.inner.catalog()?;
        if let Some(name) = drop_old {
            if let Some(mut old) = get(&catalog, "Dests") {
                old.dict_delete(name.as_str())?;
            }
        }
        tree.sort_by(|a, b| a.0.cmp(&b.0));
        let mut names = self.inner.new_array()?;
        for (name, value) in &tree {
            names.array_push(PdfObject::new_string(name)?)?;
            names.array_push(value.clone())?;
        }
        let mut root = self.inner.new_dict()?;
        root.dict_put("Names", names)?;
        let root = self.inner.add_object(&root)?;
        let mut name_dict = match get(&catalog, "Names").filter(|n| n.is_dict().unwrap_or(false)) {
            Some(n) => n,
            None => {
                let n = self.inner.new_dict()?;
                catalog.dict_put("Names", n.clone())?;
                get(&catalog, "Names").unwrap_or(n)
            }
        };
        name_dict.dict_put("Dests", root)?;
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unique_names_keep_dots_in_the_name() {
        let taken = vec!["cap.1".to_string()];
        let n = crate::attachments::numbered("cap.1", "cap.1", "", &taken);
        assert_eq!(n, "cap.1 (2)");
    }
}
