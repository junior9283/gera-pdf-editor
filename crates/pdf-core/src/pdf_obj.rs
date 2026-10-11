//! Leitura tolerante de objetos PDF: chave ausente ou de tipo errado vira `None`/vazio.
use mupdf::pdf::PdfObject;

pub(crate) fn get(obj: &PdfObject, key: &str) -> Option<PdfObject> {
    obj.get_dict(key).ok().flatten()
}

/// O nome (`/Nome`) que o objeto é, ou `None` se não for um nome.
pub(crate) fn name_of(obj: &PdfObject) -> Option<String> {
    obj.is_name()
        .ok()?
        .then(|| obj.as_name().ok())
        .flatten()
        .map(|n| String::from_utf8_lossy(&n).into_owned())
}

/// Texto da chave (vazio se faltar).
pub(crate) fn text(obj: &PdfObject, key: &str) -> String {
    get(obj, key)
        .and_then(|v| v.as_string().ok())
        .unwrap_or_default()
}

pub(crate) fn get_bool(obj: &PdfObject, key: &str) -> bool {
    get(obj, key).is_some_and(|v| v.as_bool().unwrap_or(false))
}

/// Pares (chave, valor) de uma árvore de nomes ou de números do PDF (`key` = "Names" ou "Nums",
/// descendo por `/Kids`): cada nó uma vez (ciclos), até `max` pares e `max_depth` níveis.
pub(crate) fn tree_pairs(
    root: &PdfObject,
    key: &str,
    max: usize,
    max_depth: usize,
) -> Vec<(PdfObject, PdfObject)> {
    fn walk(
        node: &PdfObject,
        key: &str,
        depth: usize,
        limits: (usize, usize),
        visited: &mut std::collections::HashSet<i32>,
        out: &mut Vec<(PdfObject, PdfObject)>,
    ) {
        let (max, max_depth) = limits;
        if depth > max_depth || out.len() >= max {
            return;
        }
        if let Ok(id) = node.as_indirect() {
            if id != 0 && !visited.insert(id) {
                return;
            }
        }
        if let Some(pairs) = get(node, key).filter(|n| n.is_array().unwrap_or(false)) {
            let len = pairs.len().unwrap_or(0).min(2 * max) as i32;
            for k in 0..len / 2 {
                if out.len() >= max {
                    return;
                }
                let at = |i| pairs.get_array(i).ok().flatten();
                if let (Some(k), Some(v)) = (at(2 * k), at(2 * k + 1)) {
                    out.push((k, v));
                }
            }
        }
        if let Some(kids) = get(node, "Kids").filter(|k| k.is_array().unwrap_or(false)) {
            for k in 0..kids.len().unwrap_or(0).min(max) as i32 {
                if out.len() >= max {
                    return;
                }
                if let Ok(Some(kid)) = kids.get_array(k) {
                    walk(&kid, key, depth + 1, limits, visited, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(
        root,
        key,
        0,
        (max, max_depth),
        &mut std::collections::HashSet::new(),
        &mut out,
    );
    out
}
