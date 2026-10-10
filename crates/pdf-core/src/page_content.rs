//! Peças comuns para mexer no conteúdo e nos recursos da página (Editar PDF: imagens e texto).
use crate::error::{refused, Result};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Matrix;

/// Inversa da matriz (recusa as degeneradas, de PDF malformado).
pub(crate) fn invert(m: &Matrix) -> Result<Matrix> {
    m.invert()
        .ok_or_else(|| refused("transformação inválida na página"))
}

/// Número para o conteúdo da página: até 6 casas, sem zeros sobrando.
pub(crate) fn num(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// O dicionário `kind` (/XObject, /Font) próprio dos recursos da página, para acrescentar ou tirar
/// itens: os recursos (herdados ou de outras páginas) são copiados, para não mudar as outras páginas.
pub(crate) fn own_resource(
    doc: &PdfDocument,
    page_obj: &mut PdfObject,
    kind: &str,
) -> Result<PdfObject> {
    let copy = |o: Option<PdfObject>| -> Result<PdfObject> {
        Ok(match o {
            Some(d) if d.is_dict()? => d.copy_dict()?,
            _ => doc.new_dict()?,
        })
    };
    let mut res = copy(page_obj.get_dict_inheritable("Resources")?)?;
    let dict = copy(res.get_dict(kind)?)?;
    res.dict_put(kind, dict)?;
    page_obj.dict_put("Resources", res)?;
    Ok(page_obj
        .get_dict("Resources")?
        .and_then(|r| r.get_dict(kind).ok().flatten())
        .expect("acabou de ser posto"))
}
