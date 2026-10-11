//! Campos de formulário (AcroForm): ler, preencher (com desfazer) e limpar, como no Acrobat.
use crate::annot_edit::{Step, SNAPSHOT_BUDGET};
use crate::doc::{annot_id, PdfDoc};
use crate::error::{PdfError, Result};
use crate::form_scripts::{
    commit_value, evaluate, format_value, number_of, parse_calc, parse_format, parse_validate,
    validate as validate_field, FieldCalc, FieldFormat, FieldValidate,
};
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
    /// Botão de ação; `reset`: Redefinir formulário ao clicar.
    Button {
        reset: bool,
    },
}

impl FieldKind {
    /// Texto ou caixa de combinação: os tipos com valor digitado (Formatar, Validar, Calcular).
    pub fn is_textual(&self) -> bool {
        matches!(self, Self::Text { .. } | Self::Combo { .. })
    }
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
    /// Formatar, Tecla, Validar e Calcular (/AA /F, /K, /V e /C). A Tecla filtra a digitação.
    pub format: FieldFormat,
    pub keystroke: FieldFormat,
    pub validate: FieldValidate,
    pub calc: FieldCalc,
}

/// Tetos do recálculo (uma cadeia de cálculos em ciclo não pode travar o app): passadas e
/// gravações no total.
const MAX_CALC_PASSES: usize = 64;
const MAX_CALC_WRITES: usize = 2000;

/// Marca de script que não é confiável (lido como script arbitrário, que não executa).
const UNTRUSTED_SCRIPT: &str = "/* script não reconhecido */";
/// Maior script lido de um stream (acima disso é arbitrário de qualquer forma).
const MAX_SCRIPT_BYTES: usize = 70_000;

/// O JavaScript da ação `key` de /AA do campo (vazio se não houver). Ação encadeada (/Next),
/// de outro tipo ou grande demais vira script arbitrário: o Acrobat rodaria o resto também.
pub(crate) fn aa_script(head: &PdfObject, key: &str) -> Result<String> {
    let Some(action) = head
        .get_dict("AA")?
        .and_then(|aa| aa.get_dict(key).ok().flatten())
    else {
        return Ok(String::new());
    };
    let javascript = action
        .get_dict("S")?
        .is_some_and(|s| s.as_name().is_ok_and(|n| n == b"JavaScript"));
    if !javascript || action.get_dict("Next")?.is_some() {
        return Ok(UNTRUSTED_SCRIPT.into());
    }
    let Some(js) = action.get_dict("JS")? else {
        return Ok(String::new());
    };
    if js.is_stream()? {
        // descompactado com teto: um stream-bomba não é aberto inteiro
        return Ok(
            match crate::attachments::read_capped(&js, MAX_SCRIPT_BYTES) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => UNTRUSTED_SCRIPT.into(),
            },
        );
    }
    Ok(js.as_string().unwrap_or_default())
}

/// O número de um cálculo como valor cru do campo (sem ruído de ponto flutuante).
fn calc_raw(v: f64) -> String {
    // número enorme: arredondar estouraria (inf)
    let rounded = if (v * 1e10).is_finite() {
        (v * 1e10).round() / 1e10
    } else {
        v
    };
    format!("{}", if rounded == 0.0 { 0.0 } else { rounded })
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

/// O botão tem a ação Redefinir formulário (/A /S /ResetForm no widget ou no campo)?
pub(crate) fn resets_form(obj: &PdfObject) -> Result<bool> {
    let is_reset = |o: &PdfObject| -> Result<bool> {
        Ok(match o.get_dict("A")? {
            Some(a) => a
                .get_dict("S")?
                .is_some_and(|s| s.as_name().is_ok_and(|n| n == b"ResetForm")),
            None => false,
        })
    };
    Ok(is_reset(obj)? || is_reset(&field_head(obj)?)?)
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
        _ => FieldKind::Button {
            reset: resets_form(&obj)?,
        },
    };
    let head = field_head(&obj)?;
    let keystroke = parse_format(&aa_script(&head, "K")?);
    let (format, validate, calc) = if kind.is_textual() {
        (
            parse_format(&aa_script(&head, "F")?),
            parse_validate(&aa_script(&head, "V")?),
            parse_calc(&aa_script(&head, "C")?),
        )
    } else {
        Default::default()
    };
    let value = w.value()?.unwrap_or_default();
    Ok(FieldInfo {
        id: annot_id(w.annotation())?,
        page,
        name: w.name()?.unwrap_or_default(),
        kind,
        rect: w.annotation().rect()?.into(),
        value,
        format,
        keystroke,
        validate,
        calc,
        readonly: flags.contains(FieldFlags::READ_ONLY),
        required: flags.contains(FieldFlags::REQUIRED),
        tooltip: obj
            .get_dict_inheritable("TU")?
            .map(|t| t.as_string())
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Grava `value` no widget de texto. Com Formatar, a aparência mostra o formatado e /V fica cru,
/// como no Acrobat. `false` se o MuPDF recusou o valor.
fn show_value(
    doc: &mut mupdf::pdf::PdfDocument,
    w: &mut PdfWidget,
    value: &str,
    format: &FieldFormat,
) -> Result<bool> {
    if !w.set_value(doc, value, true)? {
        return Ok(false);
    }
    let shown = format_value(format, value).ok().map(|f| f.text);
    if let Some(shown) = shown.filter(|s| s != value) {
        // a aparência é desenhada com o formatado; o /V volta ao cru
        if w.set_value(doc, &shown, true)? {
            w.update()?;
            field_head(&w.annotation().object())?.dict_put("V", PdfObject::new_string(value)?)?;
        }
    }
    Ok(true)
}

/// O nome de campo `name` está na lista (ele ou um grupo acima dele)?
fn named_in(name: &str, list: &[String]) -> bool {
    list.iter().any(|l| {
        name == l
            || name
                .strip_prefix(l.as_str())
                .is_some_and(|r| r.starts_with('.'))
    })
}

impl PdfDoc {
    /// Campos de formulário da página (cada botão de opção aparece separado, com o seu estado), na
    /// ordem de tabulação da página: por linha, por coluna ou pela estrutura (/Annots).
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
        let by = |column: bool| {
            move |a: &FieldInfo, b: &FieldInfo| {
                let key = |f: &FieldInfo| {
                    if column {
                        (f.rect.x0, f.rect.y0)
                    } else {
                        (f.rect.y0, f.rect.x0)
                    }
                };
                key(a)
                    .partial_cmp(&key(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        match crate::prepare::tab_order_of(&page_ref.object())? {
            crate::prepare::TabOrder::Row => out.sort_by(by(false)),
            crate::prepare::TabOrder::Column => out.sort_by(by(true)),
            _ => {}
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
        self.form_notes_of(&self.all_fields()?)
    }

    /// Os avisos a partir dos campos já lidos.
    pub(crate) fn form_notes_of(&self, fields: &[FieldInfo]) -> Result<FormNotes> {
        let acro = self.inner.catalog()?.get_dict("AcroForm")?;
        let xfa = match &acro {
            Some(a) => a.get_dict("XFA")?.is_some(),
            None => false,
        };
        // só os scripts que o app não executa (as chamadas AF do Acrobat ele executa)
        let scripts = fields.iter().any(|f| {
            [&f.format, &f.keystroke].contains(&&FieldFormat::Custom)
                || f.validate == FieldValidate::Custom
                || f.calc == FieldCalc::Custom
        });
        Ok(FormNotes { xfa, scripts })
    }

    /// Grava o valor no campo e devolve o anterior. Botão de opção: `value` = estado dele.
    /// `force`: grava mesmo somente leitura (cálculos e desfazer). Com Formatar, /V fica cru e a
    /// aparência mostra o formatado, como no Acrobat.
    pub(crate) fn write_field(
        &mut self,
        page: i32,
        id: i32,
        value: &str,
        force: bool,
        format: Option<&FieldFormat>,
    ) -> Result<String> {
        self.check_page(page)?;
        let page_ref = self.inner.load_pdf_page(page)?;
        let mut widget = page_ref
            .widgets()
            .find(|w| annot_id(w.annotation()).ok() == Some(id))
            .ok_or_else(|| PdfError::InvalidAnnotation("campo não encontrado".into()))?;
        if !force && widget.field_flags()?.contains(FieldFlags::READ_ONLY) {
            return Err(PdfError::InvalidAnnotation(
                "o campo é somente leitura".into(),
            ));
        }
        let old = widget.value()?.unwrap_or_default();
        if is_button(&widget)? {
            set_buttons(&mut field_head(&widget.annotation().object())?, value)?;
            self.update_widgets()?;
        } else {
            let parsed;
            let format = match format {
                Some(f) => f,
                None => {
                    let head = field_head(&widget.annotation().object())?;
                    parsed = parse_format(&aa_script(&head, "F")?);
                    &parsed
                }
            };
            if !show_value(&mut self.inner, &mut widget, value, format)? {
                return Err(PdfError::InvalidAnnotation(
                    "valor recusado pelo campo".into(),
                ));
            }
        }
        widget.update()?;
        Ok(old)
    }

    /// Ordem dos cálculos: os campos de /AcroForm /CO e depois os outros com Calcular, na ordem
    /// das páginas. Cada campo aparece uma vez (o primeiro widget dele).
    fn calc_order(&self, fields: &[FieldInfo]) -> Result<Vec<FieldInfo>> {
        let calculated: Vec<&FieldInfo> = fields
            .iter()
            .filter(|f| matches!(f.calc, FieldCalc::Simple { .. } | FieldCalc::Sfn(_)))
            .collect();
        let mut out: Vec<FieldInfo> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut push = |f: &FieldInfo, out: &mut Vec<FieldInfo>| {
            if seen.insert(f.name.clone()) {
                out.push(f.clone());
            }
        };
        let by_name: std::collections::HashMap<&str, &FieldInfo> =
            calculated.iter().map(|f| (f.name.as_str(), *f)).collect();
        for name in self.calc_order_names()? {
            if let Some(f) = by_name.get(name.as_str()) {
                push(f, &mut out);
            }
        }
        for f in calculated {
            push(f, &mut out);
        }
        Ok(out)
    }

    /// Refaz os cálculos até nada mudar (com tetos); devolve os valores anteriores do que mudou.
    /// Os valores anteriores vão para `changes` à medida que mudam (um erro no meio não perde o
    /// que já foi gravado).
    fn recalculate(&mut self, changes: &mut Vec<(i32, i32, String)>) -> Result<()> {
        let fields = self.all_fields()?;
        let order = self.calc_order(&fields)?;
        let start = changes.len();
        if order.is_empty() {
            return Ok(());
        }
        let mut raws: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for f in &fields {
            raws.entry(f.name.clone())
                .or_insert_with(|| f.value.clone());
        }
        for _ in 0..MAX_CALC_PASSES {
            let mut changed = false;
            for f in &order {
                if changes.len() - start >= MAX_CALC_WRITES {
                    return Ok(());
                }
                // nome de grupo ("Item") vale os campos de baixo (Item.0, Item.1), como no Acrobat
                let lookup = |n: &str| match raws.get(n) {
                    Some(v) => vec![number_of(v)],
                    None => raws
                        .iter()
                        .filter(|(k, _)| named_in(k, &[n.to_string()]))
                        .map(|(_, v)| number_of(v))
                        .collect(),
                };
                let raw = evaluate(&f.calc, &lookup).map(calc_raw).unwrap_or_default();
                if raws.get(&f.name) != Some(&raw) {
                    let old = self.write_field(f.page, f.id, &raw, true, Some(&f.format))?;
                    changes.push((f.page, f.id, old));
                    raws.insert(f.name.clone(), raw);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(())
    }

    /// Redesenha os valores dos campos de texto com Formatar (da página `page`, ou de todas): a
    /// aparência mostra o formatado. `plain`: este widget também, mesmo sem Formatar (o Formatar
    /// dele acabou de mudar).
    pub(crate) fn redraw_values(&mut self, page: Option<i32>, plain: Option<i32>) -> Result<()> {
        let pages = match page {
            Some(p) => p..p + 1,
            None => 0..self.page_count()?,
        };
        for p in pages {
            for mut w in self.inner.load_pdf_page(p)?.widgets() {
                if w.r#type()? != WidgetType::Text {
                    continue;
                }
                let head = field_head(&w.annotation().object())?;
                let format = parse_format(&aa_script(&head, "F")?);
                let mine = plain.is_some_and(|id| annot_id(w.annotation()).ok() == Some(id));
                if format == FieldFormat::Custom || (format == FieldFormat::None && !mine) {
                    continue;
                }
                let value = w.value()?.unwrap_or_default();
                if mine {
                    // o MuPDF não redesenha o mesmo valor: passa pelo vazio para refazer a aparência
                    w.set_value(&mut self.inner, "", true)?;
                }
                show_value(&mut self.inner, &mut w, &value, &format)?;
                w.update()?;
            }
        }
        Ok(())
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

    /// Preenche o campo, com Formatar (o valor digitado vira o cru), Validar (fora da regra é
    /// recusado, com o aviso do Acrobat) e os cálculos que dependem dele. Desfazer volta tudo
    /// numa etapa.
    pub fn set_field(&mut self, page: i32, id: i32, value: &str) -> Result<()> {
        let field = info(&self.widget(page, id)?, page)?;
        let raw = if field.kind.is_textual() {
            let named = |msg: String| {
                crate::error::refused(format!("{} [ {} ]", msg.trim_end_matches('.'), field.name))
            };
            let raw = commit_value(&field.format, value).map_err(named)?;
            validate_field(&field.validate, &raw).map_err(crate::error::refused)?;
            raw
        } else {
            value.to_string()
        };
        let mut steps = vec![(
            page,
            id,
            self.write_field(page, id, &raw, false, Some(&field.format))?,
        )];
        // o que já mudou entra no desfazer mesmo se um cálculo falhar no meio
        let result = self.recalculate(&mut steps);
        self.history.record_step(Step::Fields(steps));
        result
    }

    /// Botão com a ação Redefinir formulário: limpa os campos de /Fields (ou todos menos eles,
    /// com o bit 1 de /Flags), como no Acrobat.
    pub fn press_reset(&mut self, page: i32, id: i32) -> Result<()> {
        let obj = self.widget(page, id)?.annotation().object();
        let action = match obj.get_dict("A")? {
            Some(a) => Some(a),
            None => field_head(&obj)?.get_dict("A")?,
        };
        let mut filter = None;
        if let Some(a) = action {
            if let Some(list) = a
                .get_dict("Fields")?
                .filter(|l| l.is_array().unwrap_or(false))
            {
                let mut names = Vec::new();
                for k in 0..list.len()?.min(10_000) as i32 {
                    let Some(item) = list.get_array(k)? else {
                        continue;
                    };
                    names.push(if item.is_string()? {
                        item.as_string()?
                    } else {
                        crate::prepare::full_name(&item)?
                    });
                }
                let exclude = a
                    .get_dict("Flags")?
                    .and_then(|f| f.as_int().ok())
                    .is_some_and(|f| f & 1 != 0);
                filter = Some((names, exclude));
            }
        }
        self.reset_fields(filter.as_ref())
    }

    /// Limpar formulário: todos os campos voltam ao valor padrão (desfazer por instantâneo).
    pub fn reset_form(&mut self) -> Result<()> {
        self.reset_fields(None)
    }

    /// Volta ao padrão os campos de `filter` (nomes; com `true`, todos menos eles) ou todos, e
    /// recalcula (desfazer por instantâneo).
    fn reset_fields(&mut self, filter: Option<&(Vec<String>, bool)>) -> Result<()> {
        let before = self.snapshot()?;
        for p in 0..self.page_count()? {
            let page_ref = self.inner.load_pdf_page(p)?;
            for mut w in page_ref.widgets() {
                if let Some((names, exclude)) = filter {
                    let name = w.name().ok().flatten().unwrap_or_default();
                    if named_in(&name, names) == *exclude {
                        continue;
                    }
                }
                if is_button(&w)? {
                    // o reset do MuPDF não volta o grupo de botões: padrão (/DV) ou "Off"
                    let mut head = field_head(&w.annotation().object())?;
                    let default = match head.get_dict("DV")? {
                        Some(dv) => String::from_utf8_lossy(&dv.as_name()?).into_owned(),
                        None => "Off".into(),
                    };
                    set_buttons(&mut head, &default)?;
                } else if w.r#type()? == WidgetType::Signature
                    && w.annotation().object().get_dict_inheritable("V")?.is_some()
                {
                    // assinatura feita fica (como no Acrobat): apagá-la invalidaria o documento
                } else {
                    w.reset(&mut self.inner)?;
                }
            }
        }
        self.update_widgets()?;
        // o padrão aparece formatado, e os totais refeitos (como no Acrobat)
        self.redraw_values(None, None)?;
        self.recalculate(&mut Vec::new())?;
        if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
            self.mark_unsaved();
        }
        Ok(())
    }
}
