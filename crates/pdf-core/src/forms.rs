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

/// O que o formulário tem e o app não executa: a barra avisa (o Acrobat roda ambos).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormNotes {
    /// Formulário XFA (LiveCycle): os campos do PDF podem não ser os que o autor desenhou.
    pub xfa: bool,
    /// JavaScript de cálculo, formatação ou validação nos campos.
    pub scripts: bool,
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
pub(crate) fn options_of(obj: &PdfObject) -> Result<Vec<(String, String)>> {
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
pub(crate) fn on_state(obj: &PdfObject) -> Result<String> {
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

/// Nó do campo (com /T) de um widget: os botões de opção são filhos sem nome do grupo.
/// A subida é limitada: um /Parent em ciclo (PDF malicioso) não pode travar o app.
pub(crate) fn field_head(obj: &PdfObject) -> Result<PdfObject> {
    const MAX_DEPTH: usize = 32;
    let mut node = obj.try_clone()?;
    for _ in 0..MAX_DEPTH {
        if node.get_dict("T")?.is_some() {
            break;
        }
        match node.get_dict("Parent")? {
            Some(parent) => node = parent,
            None => break,
        }
    }
    Ok(node)
}

/// Widgets do campo: os /Kids, ou o próprio nó quando campo e widget são o mesmo objeto.
pub(crate) fn kids_of(head: &PdfObject) -> Result<Vec<PdfObject>> {
    match head.get_dict("Kids")? {
        Some(kids) => (0..kids.len()? as i32)
            .filter_map(|i| kids.get_array(i).transpose())
            .collect::<std::result::Result<_, _>>()
            .map_err(Into::into),
        None => Ok(vec![head.try_clone()?]),
    }
}

/// Liga o estado `value` no grupo inteiro (caixa ou botões de opção): /V vira o nome do estado e
/// cada widget mostra `value` se tiver essa aparência, senão "Off" — como o Acrobat grava.
fn set_buttons(head: &mut PdfObject, value: &str) -> Result<()> {
    let value = if value.is_empty() { "Off" } else { value };
    head.dict_put("V", PdfObject::new_name(value)?)?;
    for mut kid in kids_of(head)? {
        let has = kid
            .get_dict("AP")?
            .and_then(|ap| ap.get_dict("N").ok().flatten())
            .and_then(|n| n.get_dict(value).ok().flatten())
            .is_some();
        kid.dict_put("AS", PdfObject::new_name(if has { value } else { "Off" })?)?;
    }
    Ok(())
}

/// Campo oculto (flags Hidden ou NoView): o Acrobat não mostra nem deixa preencher.
fn hidden(obj: &PdfObject) -> Result<bool> {
    const HIDDEN: i32 = 2;
    const NO_VIEW: i32 = 32;
    Ok(obj
        .get_dict("F")?
        .map(|f| f.as_int())
        .transpose()?
        .is_some_and(|f| f & (HIDDEN | NO_VIEW) != 0))
}

fn is_button(w: &PdfWidget) -> Result<bool> {
    Ok(matches!(
        w.r#type()?,
        WidgetType::Checkbox | WidgetType::RadioButton
    ))
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
        let mut out = Vec::new();
        for w in page_ref.widgets() {
            // campo defeituoso (ex.: ciclo de /Parent) fica de fora; os outros continuam
            if !hidden(&w.annotation().object())? {
                if let Ok(f) = info(&w, page) {
                    out.push(f);
                }
            }
        }
        Ok(out)
    }

    pub fn all_fields(&self) -> Result<Vec<FieldInfo>> {
        let mut all = Vec::new();
        for p in 0..self.page_count()? {
            all.extend(self.fields(p)?);
        }
        Ok(all)
    }

    /// Avisos sobre o formulário (XFA, scripts de campo).
    pub fn form_notes(&self) -> Result<FormNotes> {
        let acro = self.inner.catalog()?.get_dict("AcroForm")?;
        let xfa = match &acro {
            Some(a) => a.get_dict("XFA")?.is_some(),
            None => false,
        };
        let mut scripts = false;
        'pages: for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                let head = field_head(&w.annotation().object())?;
                if let Some(aa) = head.get_dict("AA")? {
                    for key in ["C", "F", "K", "V"] {
                        if aa.get_dict(key)?.is_some() {
                            scripts = true;
                            break 'pages;
                        }
                    }
                }
            }
        }
        Ok(FormNotes { xfa, scripts })
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
        if is_button(&widget)? {
            set_buttons(&mut field_head(&widget.annotation().object())?, value)?;
            self.update_widgets()?;
        } else if !widget.set_value(&mut self.inner, value, true)? {
            return Err(PdfError::InvalidAnnotation(
                "valor recusado pelo campo".into(),
            ));
        }
        widget.update()?;
        Ok(old)
    }

    /// Redesenha os widgets (um grupo pode ter botões em várias páginas).
    fn update_widgets(&self) -> Result<()> {
        for p in 0..self.page_count()? {
            for mut w in self.inner.load_pdf_page(p)?.widgets() {
                w.update()?;
            }
        }
        Ok(())
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
                if is_button(&w)? {
                    // o reset do MuPDF não volta o grupo de botões: padrão (/DV) ou "Off"
                    let mut head = field_head(&w.annotation().object())?;
                    let default = match head.get_dict("DV")? {
                        Some(dv) => String::from_utf8_lossy(&dv.as_name()?).into_owned(),
                        None => "Off".into(),
                    };
                    set_buttons(&mut head, &default)?;
                } else {
                    w.reset(&mut self.inner)?;
                }
            }
        }
        self.update_widgets()?;
        if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
            self.mark_unsaved();
        }
        Ok(())
    }
}
