//! Campos de formulário (AcroForm): ler, preencher (com desfazer) e limpar, como no Acrobat.
use crate::annot_edit::{Step, SNAPSHOT_BUDGET};
use crate::doc::{annot_id, PdfDoc};
use crate::error::{PdfError, Result};
use crate::types::PdfRect;
use mupdf::pdf::{FieldFlags, PdfObject, PdfWidget, WidgetType};

/// Tipo de campo, com o que a interface precisa para editá-lo.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text {
        multiline: bool,
        password: bool,
        /// Limite de caracteres (/MaxLen).
        max_len: Option<usize>,
    },
    /// `on` = nome do estado marcado (ex.: "Yes", "Sim").
    Checkbox {
        on: String,
    },
    /// `on` = estado deste botão do grupo.
    Radio {
        on: String,
    },
    /// Opções (valor de exportação, rótulo); `editable`: aceita texto livre.
    Combo {
        options: Vec<(String, String)>,
        editable: bool,
    },
    List {
        options: Vec<(String, String)>,
    },
    Signature,
    Button,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    /// Número do objeto do widget (cada botão de opção tem o seu).
    pub id: i32,
    pub page: i32,
    /// Nome completo do campo.
    pub name: String,
    pub kind: FieldKind,
    pub rect: PdfRect,
    pub value: String,
    pub readonly: bool,
    pub required: bool,
    /// Dica (/TU), quando houver.
    pub tooltip: String,
}

/// Opções de uma lista: cada item é `(texto)` ou `[(exportação) (rótulo)]`.
fn options_of(obj: &PdfObject) -> Result<Vec<(String, String)>> {
    let Some(opt) = obj.get_dict_inheritable("Opt")? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for i in 0..opt.len()? {
        let Some(item) = opt.get_array(i as i32)? else {
            continue;
        };
        if item.is_array()? {
            let text = |k: i32| -> Result<String> {
                Ok(item
                    .get_array(k)?
                    .map(|o| o.as_string())
                    .transpose()?
                    .unwrap_or_default())
            };
            out.push((text(0)?, text(1)?));
        } else {
            let s = item.as_string()?;
            out.push((s.clone(), s));
        }
    }
    Ok(out)
}

/// Estado "ligado" de uma caixa ou botão de opção: a chave de /AP /N que não é "Off".
fn on_state(obj: &PdfObject) -> Result<String> {
    let Some(normal) = obj
        .get_dict("AP")?
        .and_then(|ap| ap.get_dict("N").ok().flatten())
    else {
        return Ok("Yes".into());
    };
    for i in 0..normal.dict_len()? {
        if let Some(key) = normal.get_dict_key(i as i32)? {
            let name = String::from_utf8_lossy(&key.as_name()?).into_owned();
            if name != "Off" {
                return Ok(name);
            }
        }
    }
    Ok("Yes".into())
}

fn info(w: &PdfWidget, page: i32) -> Result<FieldInfo> {
    let obj = w.annotation().object();
    let flags = w.field_flags()?;
    let kind = match w.r#type()? {
        WidgetType::Text => FieldKind::Text {
            multiline: flags.contains(FieldFlags::MULTILINE),
            password: flags.contains(FieldFlags::PASSWORD),
            max_len: obj
                .get_dict_inheritable("MaxLen")?
                .map(|m| m.as_int())
                .transpose()?
                .filter(|&n| n > 0)
                .map(|n| n as usize),
        },
        WidgetType::Checkbox => FieldKind::Checkbox {
            on: on_state(&obj)?,
        },
        WidgetType::RadioButton => FieldKind::Radio {
            on: on_state(&obj)?,
        },
        WidgetType::Combobox => FieldKind::Combo {
            options: options_of(&obj)?,
            editable: flags.contains(FieldFlags::EDIT),
        },
        WidgetType::Listbox => FieldKind::List {
            options: options_of(&obj)?,
        },
        WidgetType::Signature => FieldKind::Signature,
        _ => FieldKind::Button,
    };
    Ok(FieldInfo {
        id: annot_id(w.annotation())?,
        page,
        name: w.name()?.unwrap_or_default(),
        kind,
        rect: w.annotation().rect()?.into(),
        value: w.value()?.unwrap_or_default(),
        readonly: flags.contains(FieldFlags::READ_ONLY),
        required: flags.contains(FieldFlags::REQUIRED),
        tooltip: obj
            .get_dict_inheritable("TU")?
            .map(|t| t.as_string())
            .transpose()?
            .unwrap_or_default(),
    })
}

impl PdfDoc {
    /// Campos de formulário da página (cada botão de opção aparece separado, com o seu estado).
    pub fn fields(&self, page: i32) -> Result<Vec<FieldInfo>> {
        self.check_page(page)?;
        let page_ref = self.inner.load_pdf_page(page)?;
        page_ref.widgets().map(|w| info(&w, page)).collect()
    }

    pub fn all_fields(&self) -> Result<Vec<FieldInfo>> {
        let mut all = Vec::new();
        for p in 0..self.page_count()? {
            all.extend(self.fields(p)?);
        }
        Ok(all)
    }

    /// Grava o valor no campo e devolve o anterior. Botão de opção: `value` = estado dele.
    pub(crate) fn write_field(&mut self, page: i32, id: i32, value: &str) -> Result<String> {
        self.check_page(page)?;
        let page_ref = self.inner.load_pdf_page(page)?;
        let mut widget = page_ref
            .widgets()
            .find(|w| annot_id(w.annotation()).ok() == Some(id))
            .ok_or_else(|| PdfError::InvalidAnnotation("campo não encontrado".into()))?;
        if widget.field_flags()?.contains(FieldFlags::READ_ONLY) {
            return Err(PdfError::InvalidAnnotation(
                "o campo é somente leitura".into(),
            ));
        }
        let old = widget.value()?.unwrap_or_default();
        if !widget.set_value(&mut self.inner, value, true)? {
            return Err(PdfError::InvalidAnnotation(
                "valor recusado pelo campo".into(),
            ));
        }
        widget.update()?;
        Ok(old)
    }

    /// Preenche o campo; desfazer volta o valor anterior.
    pub fn set_field(&mut self, page: i32, id: i32, value: &str) -> Result<()> {
        let old = self.write_field(page, id, value)?;
        self.history.record_step(Step::Field {
            page,
            id,
            value: old,
        });
        Ok(())
    }

    /// Limpar formulário: todos os campos voltam ao valor padrão (desfazer por instantâneo).
    pub fn reset_form(&mut self) -> Result<()> {
        let before = self.snapshot()?;
        for p in 0..self.page_count()? {
            let page_ref = self.inner.load_pdf_page(p)?;
            for mut w in page_ref.widgets() {
                w.reset(&mut self.inner)?;
                w.update()?;
            }
        }
        if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
            self.mark_unsaved();
        }
        Ok(())
    }
}
