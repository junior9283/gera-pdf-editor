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
