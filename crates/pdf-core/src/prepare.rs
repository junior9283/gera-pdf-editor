//! Preparar formulário (como no Acrobat): criar, editar, mover e excluir campos AcroForm. Cada
//! alteração entra no histórico como instantâneo, como Limpar formulário.
use crate::doc::{annot_id, PdfDoc};
use crate::error::{PdfError, Result};
use crate::form_scripts::{
    calc_script, format_scripts, parse_calc, parse_format, parse_validate, validate_script,
    FieldCalc, FieldFormat, FieldValidate,
};
use crate::forms::{aa_script, field_head, kids_of, on_state, options_of, resets_form};
use crate::types::PdfRect;
use mupdf::pdf::{PdfAnnotationType, PdfDocument, PdfObject, PdfWidget};
use mupdf::{Buffer, Rect};

/// Tipo do campo a criar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewFieldKind {
    Text,
    Checkbox,
    Radio,
    Combo,
    List,
    Signature,
    /// Botão de ação (rótulo; ação Redefinir formulário).
    Button,
    /// Campo de texto com Formatar = Data (`dd/mm/aaaa`) e o seletor de data ao preencher.
    Date,
}

impl NewFieldKind {
    /// Prefixo do nome automático ("Texto1", "Caixa1"…).
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Text => "Texto",
            Self::Checkbox => "Caixa",
            Self::Radio => "Grupo",
            Self::Combo => "Lista suspensa",
            Self::List => "Lista",
            Self::Signature => "Assinatura",
            Self::Button => "Botão",
            Self::Date => "Data",
        }
    }
}

/// Uma alteração do Preparar formulário (pelo worker).
#[derive(Debug, Clone, PartialEq)]
pub enum PrepareOp {
    Add {
        page: i32,
        rect: PdfRect,
        kind: NewFieldKind,
        name: String,
    },
    SetProps {
        page: i32,
        id: i32,
        props: Box<FieldProps>,
    },
    Move {
        page: i32,
        id: i32,
        rect: PdfRect,
    },
    Delete {
        page: i32,
        id: i32,
    },
    /// Ordem dos cálculos (/AcroForm /CO), pelos nomes dos campos.
    SetCalcOrder {
        names: Vec<String>,
    },
    /// Ordem de tabulação da página (/Tabs; a manual reordena /Annots).
    SetTabOrder {
        page: i32,
        order: TabOrder,
    },
}

/// Ordem de tabulação, como no menu do Acrobat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabOrder {
    /// Por linha (/Tabs /R): de cima para baixo, da esquerda para a direita.
    Row,
    /// Por coluna (/Tabs /C).
    Column,
    /// Pela estrutura (/Tabs /S, ou sem /Tabs): a ordem de /Annots.
    Structure,
    /// Manual: os widgets nesta ordem (grava /Annots e /Tabs /S).
    Manual(Vec<i32>),
}

/// Propriedades editáveis de um campo (a janela Propriedades do Acrobat). Os itens que não se
/// aplicam ao tipo ficam no padrão.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldProps {
    /// Nome do campo (do grupo, no botão de opção).
    pub name: String,
    pub tooltip: String,
    pub required: bool,
    pub readonly: bool,
    pub multiline: bool,
    pub password: bool,
    pub max_len: Option<usize>,
    /// Valor padrão do texto (também vira o valor atual).
    pub default: String,
    /// Tamanho da fonte; 0 = automático.
    pub font_size: f32,
    /// Opções das listas: (valor de exportação, rótulo).
    pub options: Vec<(String, String)>,
    /// Lista suspensa aceita texto digitado.
    pub editable: bool,
    /// Valor de exportação da caixa ou do botão de opção.
    pub export: String,
    pub checked_default: bool,
    /// Formatar, Validar e Calcular (campos de texto). `Custom` = script do arquivo, mantido.
    pub format: FieldFormat,
    pub validate: FieldValidate,
    pub calc: FieldCalc,
    /// Botão: rótulo (/MK /CA) e a ação Redefinir formulário.
    pub button_label: String,
    pub reset_action: bool,
}

/// Formato do campo Data novo, como o Acrobat em português.
const DATE_FORMAT: &str = "dd/mm/yyyy";

const READ_ONLY: i32 = 1;
const REQUIRED: i32 = 1 << 1;
const MULTILINE: i32 = 1 << 12;
const PASSWORD: i32 = 1 << 13;
const NO_TOGGLE_TO_OFF: i32 = 1 << 14;
const RADIO: i32 = 1 << 15;
const COMBO: i32 = 1 << 17;
const EDIT: i32 = 1 << 18;
const PUSHBUTTON: i32 = 1 << 16;

/// Valor de exportação padrão da caixa de seleção nova.
const CHECK_ON: &str = "Sim";

fn invalid(msg: &str) -> PdfError {
    PdfError::InvalidAnnotation(msg.into())
}

/// Nome aceito para um campo: sem espaços nas pontas, não vazio e sem ponto (o ponto criaria uma
/// hierarquia de campos).
fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("o campo precisa de um nome"));
    }
    if name.contains('.') {
        return Err(invalid("o nome do campo não pode ter ponto"));
    }
    Ok(name)
}

fn name_obj(s: &str) -> Result<PdfObject> {
    Ok(PdfObject::new_name(s)?)
}

fn int(v: i32) -> Result<PdfObject> {
    Ok(PdfObject::new_int(v)?)
}

fn flags_of(head: &PdfObject) -> Result<i32> {
    Ok(head
        .get_dict_inheritable("Ff")?
        .map(|f| f.as_int())
        .transpose()?
        .unwrap_or(0))
}

fn set_flag(flags: &mut i32, bit: i32, on: bool) {
    if on {
        *flags |= bit;
    } else {
        *flags &= !bit;
    }
}

/// Tamanho da fonte em /DA ("/Helv 12 Tf 0 g"); 0 = automático.
fn da_size(obj: &PdfObject) -> Result<f32> {
    let Some(da) = obj.get_dict_inheritable("DA")? else {
        return Ok(0.0);
    };
    let da = da.as_string()?;
    let words: Vec<&str> = da.split_whitespace().collect();
    Ok(words
        .windows(2)
        .find(|w| w[1] == "Tf")
        .and_then(|w| w[0].parse().ok())
        .unwrap_or(0.0))
}

fn remove_ref(array: &mut PdfObject, num: i32) -> Result<()> {
    for i in (0..array.len()? as i32).rev() {
        if let Some(item) = array.get_array(i)? {
            if item.as_indirect()? == num {
                array.array_delete(i)?;
            }
        }
    }
    Ok(())
}

/// /AcroForm do documento, criado se faltar, com /Fields, /DA e as fontes de /DR que os campos
/// novos usam (Helvetica e ZapfDingbats).
fn acroform(doc: &mut PdfDocument) -> Result<PdfObject> {
    let mut catalog = doc.catalog()?;
    let mut acro = match catalog.get_dict("AcroForm")? {
        Some(a) => a,
        None => {
            let a = doc.add_object(&doc.new_dict()?)?;
            catalog.dict_put("AcroForm", a.clone())?;
            a
        }
    };
    if acro.get_dict("Fields")?.is_none() {
        acro.dict_put("Fields", doc.new_array()?)?;
    }
    // com XFA, o Acrobat desenha pelo modelo XFA e o campo novo ficaria invisível: editar o
    // formulário o torna um AcroForm comum (como o pdf.js e o pdf-lib fazem)
    acro.dict_delete("XFA")?;
    if acro.get_dict("DA")?.is_none() {
        acro.dict_put("DA", PdfObject::new_string("/Helv 0 Tf 0 g")?)?;
    }
    let mut dr = child_or_new(doc, &mut acro, "DR", false)?;
    let mut fonts = child_or_new(doc, &mut dr, "Font", false)?;
    for (key, base, enc) in [
        ("Helv", "Helvetica", "/Encoding/WinAnsiEncoding"),
        ("ZaDb", "ZapfDingbats", ""),
    ] {
        if fonts.get_dict(key)?.is_none() {
            let font = doc.new_object_from_str(&format!(
                "<</Type/Font/Subtype/Type1/BaseFont/{base}{enc}>>"
            ))?;
            fonts.dict_put(key, doc.add_object(&font)?)?;
        }
    }
    Ok(acro)
}

/// O dicionário (ou array) `key` de `obj`, criado vazio se faltar ou for de outro tipo.
fn child_or_new(
    doc: &PdfDocument,
    obj: &mut PdfObject,
    key: &str,
    array: bool,
) -> Result<PdfObject> {
    if let Some(c) = obj.get_dict(key)? {
        if if array { c.is_array()? } else { c.is_dict()? } {
            return Ok(c);
        }
    }
    obj.dict_put(
        key,
        if array {
            doc.new_array()?
        } else {
            doc.new_dict()?
        },
    )?;
    obj.get_dict(key)?
        .ok_or_else(|| invalid(&format!("/{key} inválido")))
}

/// Põe (ou tira, com `js` vazio) a ação JavaScript `key` de /AA do campo.
fn set_aa(doc: &mut PdfDocument, head: &mut PdfObject, key: &str, js: &str) -> Result<()> {
    if js.is_empty() && head.get_dict("AA")?.is_none() {
        return Ok(());
    }
    let mut aa = child_or_new(doc, head, "AA", false)?;
    if js.is_empty() {
        aa.dict_delete(key)?;
        if aa.dict_len()? == 0 {
            head.dict_delete("AA")?;
        }
        return Ok(());
    }
    let mut action = doc.new_dict()?;
    action.dict_put("S", name_obj("JavaScript")?)?;
    action.dict_put("JS", PdfObject::new_string(js)?)?;
    aa.dict_put(key, action)?;
    Ok(())
}

/// A ordem de tabulação gravada no /Tabs da página.
pub(crate) fn tab_order_of(page: &PdfObject) -> Result<TabOrder> {
    let tabs = page.get_dict("Tabs")?.and_then(|t| t.as_name().ok());
    Ok(match tabs.as_deref() {
        Some(b"R") => TabOrder::Row,
        Some(b"C") => TabOrder::Column,
        _ => TabOrder::Structure,
    })
}

/// Nome completo do campo (os /T do nó e dos pais, com ponto).
pub(crate) fn full_name(head: &PdfObject) -> Result<String> {
    let mut parts = Vec::new();
    let mut node = Some(head.try_clone()?);
    for _ in 0..32 {
        let Some(n) = node else { break };
        if let Some(t) = n.get_dict("T")? {
            parts.push(t.as_string().unwrap_or_default());
        }
        node = n.get_dict("Parent")?;
    }
    parts.reverse();
    Ok(parts.join("."))
}

/// /AP com os nomes dos estados ("ligado" e "Off") de uma caixa ou botão de opção. O desenho é do
/// MuPDF (a partir de /MK /CA, já em pé na página girada por /MK /R); aqui só ficam os nomes.
fn state_ap(doc: &mut PdfDocument, state: &str) -> Result<PdfObject> {
    let mut normal = doc.new_dict()?;
    for key in [state, "Off"] {
        let empty = doc.new_object_from_str("<</Type/XObject/Subtype/Form/BBox[0 0 1 1]>>")?;
        normal.dict_put(
            key,
            doc.add_stream(&Buffer::from_bytes(b"")?, Some(&empty), false)?,
        )?;
    }
    let mut ap = doc.new_dict()?;
    ap.dict_put("N", normal)?;
    Ok(ap)
}

/// /Fields do /AcroForm.
fn fields_of(acro: &PdfObject) -> Result<PdfObject> {
    acro.get_dict("Fields")?
        .filter(|f| f.is_array().unwrap_or(false))
        .ok_or_else(|| invalid("/AcroForm sem /Fields válido"))
}

/// /Kids de um grupo de botões de opção.
fn kids_array(head: &PdfObject) -> Result<PdfObject> {
    head.get_dict("Kids")?
        .ok_or_else(|| invalid("grupo de opção de outro programa sem /Kids: crie outro grupo"))
}

/// Nome parcial (/T) do nó do campo.
fn partial_name(head: &PdfObject) -> Result<String> {
    Ok(head
        .get_dict("T")?
        .map(|t| t.as_string())
        .transpose()?
        .unwrap_or_default())
}

/// /DA com o tamanho da fonte trocado (o resto — fonte e cor — fica).
fn with_da_size(da: &str, size: f32) -> String {
    let mut words: Vec<String> = da.split_whitespace().map(str::to_owned).collect();
    match words.iter().position(|w| w == "Tf") {
        Some(i) if i > 0 => {
            words[i - 1] = format!("{size}");
            words.join(" ")
        }
        _ => format!("/Helv {size} Tf 0 g"),
    }
}

/// Troca o nome de um estado em /AP /N (valor de exportação).
fn rename_state(obj: &mut PdfObject, old: &str, new: &str) -> Result<()> {
    if let Some(mut normal) = obj
        .get_dict("AP")?
        .and_then(|ap| ap.get_dict("N").ok().flatten())
    {
        if let Some(stream) = normal.get_dict(old)? {
            normal.dict_put(new, stream)?;
            normal.dict_delete(old)?;
        }
    }
    Ok(())
}

/// Estado novo para o próximo botão do grupo: "Opção1", "Opção2"… sem repetir.
fn next_choice(head: &PdfObject) -> Result<String> {
    let used: Vec<String> = kids_of(head)?.iter().map(on_state).collect::<Result<_>>()?;
    Ok((1..)
        .map(|n| format!("Opção{n}"))
        .find(|s| !used.contains(s))
        .expect("sequência infinita"))
}

impl PdfDoc {
    /// Nomes completos de todos os campos (inclusive ocultos e de outros programas).
    fn field_names(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                if let Some(n) = w.name().ok().flatten() {
                    out.push(n);
                }
            }
        }
        Ok(out)
    }

    /// Primeiro nome automático livre do tipo ("Texto1", "Texto2"…).
    fn auto_name(&self, kind: NewFieldKind) -> Result<String> {
        let taken = self.field_names()?;
        let p = kind.prefix();
        Ok((1..)
            .map(|n| format!("{p}{n}"))
            .find(|n| {
                !taken
                    .iter()
                    .any(|t| t == n || t.starts_with(&format!("{n}.")))
            })
            .expect("sequência infinita"))
    }

    /// `name` já é o nó pai de outros campos ("a" com "a.b" existindo)?
    fn is_node(&self, name: &str) -> Result<bool> {
        let prefix = format!("{name}.");
        Ok(self.field_names()?.iter().any(|n| n.starts_with(&prefix)))
    }

    /// Cabeça (nó com /T) do campo de nome `name`, se existir.
    fn field_named(&self, name: &str) -> Result<Option<PdfObject>> {
        for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                // campo defeituoso (ex.: ciclo de /Parent) não impede os outros
                if w.name().ok().flatten().as_deref() == Some(name) {
                    return Ok(Some(field_head(&w.annotation().object())?));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn widget(&self, page: i32, id: i32) -> Result<PdfWidget> {
        self.check_page(page)?;
        self.inner
            .load_pdf_page(page)?
            .widgets()
            .find(|w| annot_id(w.annotation()).ok() == Some(id))
            .ok_or_else(|| invalid("campo não encontrado"))
    }

    /// Redesenha os campos da página (aparências geradas pelo MuPDF).
    /// `plain`: o widget cujo Formatar acabou de mudar (redesenhado mesmo sem Formatar).
    fn refresh_widgets(&mut self, page: i32, plain: Option<i32>) -> Result<()> {
        for mut w in self.inner.load_pdf_page(page)?.widgets() {
            // um campo defeituoso de outro programa fica como está; os nossos são redesenhados
            let _ = w.update();
        }
        // o MuPDF redesenha com o /V cru: os campos com Formatar voltam a mostrar o formatado
        self.redraw_values(Some(page), plain)
    }

    /// Cria um campo `kind` chamado `name` no retângulo; devolve o id. Botões de opção com o nome
    /// de um grupo existente entram nele (o "Adicionar botão" do Acrobat).
    pub fn add_field(
        &mut self,
        page: i32,
        rect: PdfRect,
        kind: NewFieldKind,
        name: &str,
    ) -> Result<i32> {
        self.check_page(page)?;
        // sem nome: o próximo automático (cliques rápidos não repetem nome)
        let auto;
        let name = if name.trim().is_empty() {
            auto = self.auto_name(kind)?;
            auto.as_str()
        } else {
            valid_name(name)?
        };
        if self.is_node(name)? {
            return Err(invalid("já existe um grupo de campos com esse nome"));
        }
        let group = match (self.field_named(name)?, kind) {
            (None, _) => None,
            (Some(head), NewFieldKind::Radio) if flags_of(&head)? & RADIO != 0 => {
                kids_array(&head)?; // botão "mesclado" de outro programa: não dá para crescer
                Some(head)
            }
            (Some(_), _) => return Err(invalid("já existe um campo com esse nome")),
        };
        self.with_snapshot(|d| d.create_field(page, rect, kind, name, group))
    }

    fn create_field(
        &mut self,
        page: i32,
        rect: PdfRect,
        kind: NewFieldKind,
        name: &str,
        group: Option<PdfObject>,
    ) -> Result<i32> {
        let acro = acroform(&mut self.inner)?;
        let mut page_ref = self.inner.load_pdf_page(page)?;
        let rotation = page_ref.rotation()?.rem_euclid(360);
        let mut annot = page_ref.create_annotation(PdfAnnotationType::Widget)?;
        annot.set_rect(Rect::from(rect))?;
        let id = annot_id(&annot)?;
        let me = self.inner.new_indirect(id, 0)?;
        let mut w = annot.object();
        w.dict_put("F", int(4)?)?; // imprimir
        let font = match kind {
            NewFieldKind::Checkbox | NewFieldKind::Radio => "ZaDb",
            _ => "Helv",
        };
        w.dict_put("DA", PdfObject::new_string(&format!("/{font} 0 Tf 0 g"))?)?;
        let mut mk = self.inner.new_dict()?;
        if rotation != 0 {
            mk.dict_put("R", int(rotation)?)?;
        }
        // o nó do campo: o próprio widget, ou o grupo de botões de opção
        let head_is_widget = kind != NewFieldKind::Radio;
        match kind {
            NewFieldKind::Text => {
                w.dict_put("FT", name_obj("Tx")?)?;
                w.dict_put("V", PdfObject::new_string("")?)?;
            }
            NewFieldKind::Date => {
                w.dict_put("FT", name_obj("Tx")?)?;
                w.dict_put("V", PdfObject::new_string("")?)?;
                let (f, k) = format_scripts(&FieldFormat::Date(DATE_FORMAT.into()));
                set_aa(&mut self.inner, &mut w, "F", &f)?;
                set_aa(&mut self.inner, &mut w, "K", &k)?;
            }
            NewFieldKind::Button => {
                w.dict_put("FT", name_obj("Btn")?)?;
                w.dict_put("Ff", int(PUSHBUTTON)?)?;
                mk.dict_put("CA", PdfObject::new_string(name)?)?;
                mk.dict_put("BG", self.inner.new_object_from_str("[0.8]")?)?;
                mk.dict_put("BC", self.inner.new_object_from_str("[0.5]")?)?;
            }
            NewFieldKind::Checkbox => {
                w.dict_put("FT", name_obj("Btn")?)?;
                w.dict_put("V", name_obj("Off")?)?;
                w.dict_put("AS", name_obj("Off")?)?;
                mk.dict_put("CA", PdfObject::new_string("4")?)?;
                w.dict_put("AP", state_ap(&mut self.inner, CHECK_ON)?)?;
            }
            NewFieldKind::Radio => {
                let head = match group {
                    Some(g) => g,
                    None => self.new_radio_group(&acro, name, RADIO | NO_TOGGLE_TO_OFF)?,
                };
                let state = next_choice(&head)?;
                w.dict_put("Parent", head.clone())?;
                w.dict_put("AS", name_obj("Off")?)?;
                mk.dict_put("CA", PdfObject::new_string("l")?)?;
                w.dict_put("AP", state_ap(&mut self.inner, &state)?)?;
                kids_array(&head)?.array_push(me.clone())?;
            }
            NewFieldKind::Combo | NewFieldKind::List => {
                w.dict_put("FT", name_obj("Ch")?)?;
                if kind == NewFieldKind::Combo {
                    w.dict_put("Ff", int(COMBO)?)?;
                }
                w.dict_put("Opt", self.inner.new_array()?)?;
                w.dict_put("V", PdfObject::new_string("")?)?;
            }
            NewFieldKind::Signature => w.dict_put("FT", name_obj("Sig")?)?,
        }
        w.dict_put("MK", mk)?;
        if head_is_widget {
            w.dict_put("T", PdfObject::new_string(name)?)?;
            fields_of(&acro)?.array_push(me)?;
        }
        annot.update()?;
        page_ref.update()?;
        self.refresh_widgets(page, None)?;
        Ok(id)
    }

    /// Grupo novo de botões de opção, em /Fields.
    fn new_radio_group(&mut self, acro: &PdfObject, name: &str, flags: i32) -> Result<PdfObject> {
        let mut g = self.inner.add_object(
            &self
                .inner
                .new_object_from_str(&format!("<</FT/Btn/Ff {flags}/V/Off/Kids[]>>"))?,
        )?;
        g.dict_put("T", PdfObject::new_string(name)?)?;
        fields_of(acro)?.array_push(g.clone())?;
        Ok(g)
    }

    /// Aplica uma alteração do Preparar formulário.
    pub fn prepare(&mut self, op: PrepareOp) -> Result<()> {
        match op {
            PrepareOp::Add {
                page,
                rect,
                kind,
                name,
            } => self.add_field(page, rect, kind, &name).map(drop),
            PrepareOp::SetProps { page, id, props } => self.set_field_props(page, id, &props),
            PrepareOp::Move { page, id, rect } => self.move_field(page, id, rect),
            PrepareOp::Delete { page, id } => self.delete_field(page, id),
            PrepareOp::SetCalcOrder { names } => self.with_snapshot(|d| d.write_calc_order(&names)),
            PrepareOp::SetTabOrder { page, order } => {
                self.check_page(page)?;
                self.with_snapshot(|d| d.write_tab_order(page, &order))
            }
        }
    }

    /// A ordem de tabulação da página (pelo /Tabs).
    pub fn tab_order_kind(&self, page: i32) -> Result<TabOrder> {
        tab_order_of(&self.inner.load_pdf_page(page)?.object())
    }

    fn write_tab_order(&mut self, page: i32, order: &TabOrder) -> Result<()> {
        let mut page_obj = self.inner.load_pdf_page(page)?.object();
        let tabs = match order {
            TabOrder::Row => "R",
            TabOrder::Column => "C",
            TabOrder::Structure => "S",
            TabOrder::Manual(ids) => {
                // os widgets citados entram juntos, na ordem pedida, onde estava o primeiro deles
                let Some(annots) = page_obj
                    .get_dict("Annots")?
                    .filter(|a| a.is_array().unwrap_or(false))
                else {
                    return Ok(());
                };
                let items: Vec<PdfObject> = (0..annots.len()? as i32)
                    .filter_map(|k| annots.get_array(k).ok().flatten())
                    .collect();
                let num = |o: &PdfObject| o.as_indirect().ok();
                let listed: Vec<PdfObject> = ids
                    .iter()
                    .filter_map(|id| items.iter().find(|o| num(o) == Some(*id)).cloned())
                    .collect();
                let mut out = self.inner.new_array()?;
                let mut placed = false;
                for item in &items {
                    if num(item).is_some_and(|n| ids.contains(&n)) {
                        if !placed {
                            for l in &listed {
                                out.array_push(l.clone())?;
                            }
                            placed = true;
                        }
                    } else {
                        out.array_push(item.clone())?;
                    }
                }
                page_obj.dict_put("Annots", out)?;
                "S"
            }
        };
        page_obj.dict_put("Tabs", name_obj(tabs)?)?;
        Ok(())
    }

    /// Nomes dos campos na ordem de cálculo (/AcroForm /CO).
    pub fn calc_order_names(&self) -> Result<Vec<String>> {
        let Some(co) = self
            .inner
            .catalog()?
            .get_dict("AcroForm")?
            .and_then(|a| a.get_dict("CO").ok().flatten())
            .filter(|c| c.is_array().unwrap_or(false))
        else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for k in 0..co.len()?.min(10_000) as i32 {
            if let Some(head) = co.get_array(k)? {
                let name = full_name(&head)?;
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
        Ok(out)
    }

    /// Grava /CO com os campos nomeados (os que não existem ficam de fora).
    fn write_calc_order(&mut self, names: &[String]) -> Result<()> {
        // nome → nó, numa passada pelas páginas
        let mut heads = std::collections::HashMap::new();
        for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                // campo defeituoso (ex.: ciclo de /Parent) não impede os outros
                if let (Some(name), Ok(head)) = (
                    w.name().ok().flatten(),
                    field_head(&w.annotation().object()),
                ) {
                    heads.entry(name).or_insert(head);
                }
            }
        }
        let mut co = self.inner.new_array()?;
        for name in names {
            if let Some(head) = heads.remove(name) {
                co.array_push(head)?;
            }
        }
        acroform(&mut self.inner)?.dict_put("CO", co)?;
        Ok(())
    }

    /// Formatar, Validar e Calcular: só o que mudou é regravado (script do arquivo que o app não
    /// entende continua intacto). O campo calculado entra no fim de /CO, e sai ao deixar de ser.
    fn write_scripts(
        &mut self,
        head: &mut PdfObject,
        p: &FieldProps,
        before: &FieldProps,
    ) -> Result<()> {
        if p.format != before.format && p.format != FieldFormat::Custom {
            let (f, k) = format_scripts(&p.format);
            set_aa(&mut self.inner, head, "F", &f)?;
            set_aa(&mut self.inner, head, "K", &k)?;
        }
        if p.validate != before.validate && p.validate != FieldValidate::Custom {
            set_aa(&mut self.inner, head, "V", &validate_script(&p.validate))?;
        }
        if let FieldCalc::Sfn(expr) = &p.calc {
            if p.calc != before.calc && !crate::form_scripts::sfn_ok(expr) {
                return Err(crate::error::refused(
                    "A expressão de cálculo não é válida.",
                ));
            }
        }
        if p.calc != before.calc && p.calc != FieldCalc::Custom {
            set_aa(&mut self.inner, head, "C", &calc_script(&p.calc))?;
            let num = head.as_indirect()?;
            let mut acro = acroform(&mut self.inner)?;
            let mut co = child_or_new(&self.inner, &mut acro, "CO", true)?;
            remove_ref(&mut co, num)?;
            if p.calc != FieldCalc::None {
                co.array_push(head.clone())?;
            }
        }
        Ok(())
    }

    /// Botão de ação: rótulo e a ação Redefinir formulário.
    fn write_button(&mut self, obj: &mut PdfObject, p: &FieldProps) -> Result<()> {
        let mut mk = child_or_new(&self.inner, obj, "MK", false)?;
        mk.dict_put("CA", PdfObject::new_string(&p.button_label)?)?;
        if p.reset_action {
            obj.dict_put("A", self.inner.new_object_from_str("<</S/ResetForm>>")?)?;
        } else if resets_form(obj)? {
            obj.dict_delete("A")?;
        }
        Ok(())
    }

    /// Propriedades do campo `id`.
    pub fn field_props(&self, page: i32, id: i32) -> Result<FieldProps> {
        let w = self.widget(page, id)?;
        let obj = w.annotation().object();
        let head = field_head(&obj)?;
        let flags = flags_of(&head)?;
        let text = |o: Option<PdfObject>| -> Result<String> {
            Ok(match o {
                Some(o) if o.is_string()? => o.as_string()?,
                Some(o) if o.is_name()? => String::from_utf8_lossy(&o.as_name()?).into_owned(),
                _ => String::new(),
            })
        };
        let is_button = head
            .get_dict_inheritable("FT")?
            .map(|t| t.as_name())
            .transpose()?
            == Some(b"Btn".to_vec());
        let export = if is_button {
            on_state(&obj)?
        } else {
            String::new()
        };
        let default = text(head.get_dict("DV")?)?;
        let label = obj
            .get_dict("MK")?
            .and_then(|mk| mk.get_dict("CA").ok().flatten());
        Ok(FieldProps {
            format: parse_format(&aa_script(&head, "F")?),
            validate: parse_validate(&aa_script(&head, "V")?),
            calc: parse_calc(&aa_script(&head, "C")?),
            button_label: text(label)?,
            reset_action: is_button && flags & PUSHBUTTON != 0 && resets_form(&obj)?,
            name: partial_name(&head)?,
            tooltip: text(head.get_dict("TU")?)?,
            required: flags & REQUIRED != 0,
            readonly: flags & READ_ONLY != 0,
            multiline: flags & MULTILINE != 0,
            password: flags & PASSWORD != 0,
            max_len: head
                .get_dict_inheritable("MaxLen")?
                .map(|m| m.as_int())
                .transpose()?
                .filter(|&n| n > 0)
                .map(|n| n as usize),
            checked_default: is_button && !export.is_empty() && default == export,
            default: if is_button { String::new() } else { default },
            font_size: da_size(&obj)?,
            options: options_of(&obj)?,
            editable: flags & EDIT != 0,
            export,
        })
    }

    /// Troca as propriedades do campo `id` (desfazer volta tudo). O nome é o do nó (/T): num campo
    /// "endereco.rua", só "rua".
    pub fn set_field_props(&mut self, page: i32, id: i32, p: &FieldProps) -> Result<()> {
        let w = self.widget(page, id)?;
        let head = field_head(&w.annotation().object())?;
        let radio = flags_of(&head)? & RADIO != 0;
        let name = valid_name(&p.name)?.to_owned();
        let partial = partial_name(&head)?;
        let full = w.name().ok().flatten().unwrap_or_default();
        // "endereco.rua" → prefixo "endereco."
        let prefix = full.strip_suffix(partial.as_str()).unwrap_or_default();
        let target = if name == partial {
            None
        } else {
            let new_full = format!("{prefix}{name}");
            if self.is_node(&new_full)? {
                return Err(invalid("já existe um grupo de campos com esse nome"));
            }
            match self.field_named(&new_full)? {
                None => Some(None),
                Some(g) if radio && flags_of(&g)? & RADIO != 0 => {
                    kids_array(&g)?;
                    Some(Some(g))
                }
                Some(_) => return Err(invalid("já existe um campo com esse nome")),
            }
        };
        if p.export.contains(['/', ' ']) {
            return Err(invalid(
                "o valor de exportação não pode ter espaço nem barra",
            ));
        }
        self.with_snapshot(|d| d.write_props(page, id, p, &name, target))
    }

    fn write_props(
        &mut self,
        page: i32,
        id: i32,
        p: &FieldProps,
        name: &str,
        target: Option<Option<PdfObject>>,
    ) -> Result<()> {
        let before = self.field_props(page, id)?;
        let w = self.widget(page, id)?;
        let mut obj = w.annotation().object();
        let mut head = field_head(&obj)?;
        let radio = flags_of(&head)? & RADIO != 0;
        match target {
            // botão de opção trocado de grupo: sai do atual (que some se ficar vazio) e entra no outro
            Some(group) if radio => head = self.move_to_group(&mut obj, &head, id, name, group)?,
            Some(_) => head.dict_put("T", PdfObject::new_string(name)?)?,
            None => {}
        }
        let mut flags = flags_of(&head)?;
        set_flag(&mut flags, READ_ONLY, p.readonly);
        set_flag(&mut flags, REQUIRED, p.required);
        let ft = head
            .get_dict_inheritable("FT")?
            .map(|t| t.as_name())
            .transpose()?
            .unwrap_or_default();
        if p.tooltip.is_empty() {
            head.dict_delete("TU")?;
        } else {
            head.dict_put("TU", PdfObject::new_string(&p.tooltip)?)?;
        }
        // só o tamanho muda, e só se mudou: fonte e cor de outro programa ficam
        if p.font_size != before.font_size {
            let da = obj
                .get_dict_inheritable("DA")?
                .map(|d| d.as_string())
                .transpose()?
                .unwrap_or_default();
            let da = with_da_size(&da, p.font_size.max(0.0));
            obj.dict_put("DA", PdfObject::new_string(&da)?)?;
        }
        match ft.as_slice() {
            b"Tx" => {
                set_flag(&mut flags, MULTILINE, p.multiline);
                set_flag(&mut flags, PASSWORD, p.password);
                match p.max_len {
                    Some(n) if n > 0 => head.dict_put("MaxLen", int(n as i32)?)?,
                    _ => head.dict_delete("MaxLen")?,
                }
                // o valor preenchido só é trocado quando o padrão muda (como no Acrobat)
                if p.default != before.default {
                    head.dict_put("DV", PdfObject::new_string(&p.default)?)?;
                    head.dict_put("V", PdfObject::new_string(&p.default)?)?;
                }
                self.write_scripts(&mut head, p, &before)?;
            }
            b"Ch" => {
                if flags & COMBO != 0 {
                    set_flag(&mut flags, EDIT, p.editable);
                }
                if p.options != before.options {
                    self.write_options(&mut head, &p.options)?;
                }
            }
            b"Btn" if flags & PUSHBUTTON != 0 => self.write_button(&mut obj, p)?,
            b"Btn" => self.write_export(&mut obj, &mut head, p)?,
            _ => {}
        }
        head.dict_put("Ff", int(flags)?)?;
        self.refresh_widgets(page, Some(id))
    }

    /// Botão de opção `id` sai do grupo atual (que some se ficar vazio; o valor dele deixa de
    /// apontar para o botão) e entra em `group` (ou num grupo novo `name`), com um estado que não
    /// colida com os de lá. Devolve o grupo de destino.
    fn move_to_group(
        &mut self,
        obj: &mut PdfObject,
        old: &PdfObject,
        id: i32,
        name: &str,
        group: Option<PdfObject>,
    ) -> Result<PdfObject> {
        let state = on_state(obj)?;
        remove_ref(&mut kids_array(old)?, id)?;
        let mut old = old.clone();
        let points_here = old
            .get_dict("V")?
            .map(|v| v.as_name())
            .transpose()?
            .as_deref()
            == Some(state.as_bytes());
        if points_here {
            old.dict_put("V", name_obj("Off")?)?;
        }
        if kids_array(&old)?.len()? == 0 {
            self.unlink_field(&old)?;
        }
        let head = match group {
            Some(g) => g,
            None => {
                let acro = acroform(&mut self.inner)?;
                self.new_radio_group(&acro, name, flags_of(&old)?)?
            }
        };
        let taken: Vec<String> = kids_of(&head)?
            .iter()
            .map(on_state)
            .collect::<Result<_>>()?;
        if taken.contains(&state) {
            rename_state(obj, &state, &next_choice(&head)?)?;
        }
        obj.dict_put("AS", name_obj("Off")?)?;
        obj.dict_put("Parent", head.clone())?;
        kids_array(&head)?.array_push(self.inner.new_indirect(id, 0)?)?;
        Ok(head)
    }

    /// Opções da lista; a seleção (/I) é refeita pelo leitor e o valor que sumiu das opções é limpo.
    fn write_options(&mut self, head: &mut PdfObject, options: &[(String, String)]) -> Result<()> {
        let mut opt = self.inner.new_array()?;
        for (export, label) in options {
            if export == label || label.is_empty() {
                opt.array_push(PdfObject::new_string(export)?)?;
            } else {
                let mut pair = self.inner.new_array()?;
                pair.array_push(PdfObject::new_string(export)?)?;
                pair.array_push(PdfObject::new_string(label)?)?;
                opt.array_push(pair)?;
            }
        }
        head.dict_put("Opt", opt)?;
        head.dict_delete("I")?;
        let value = head
            .get_dict("V")?
            .filter(|v| v.is_string().unwrap_or(false))
            .map(|v| v.as_string())
            .transpose()?;
        let editable = flags_of(head)? & EDIT != 0;
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            if !editable && !options.iter().any(|(e, _)| *e == v) {
                head.dict_put("V", PdfObject::new_string("")?)?;
            }
        }
        Ok(())
    }

    /// Valor de exportação e "marcada por padrão" da caixa ou do botão de opção.
    fn write_export(
        &mut self,
        obj: &mut PdfObject,
        head: &mut PdfObject,
        p: &FieldProps,
    ) -> Result<()> {
        let old = on_state(obj)?;
        let export = if p.export.trim().is_empty() {
            old.clone()
        } else {
            p.export.trim().to_owned()
        };
        if export != old {
            rename_state(obj, &old, &export)?;
        }
        let is_old = |v: Option<PdfObject>| -> Result<bool> {
            Ok(v.map(|v| v.as_name()).transpose()?.as_deref() == Some(old.as_bytes()))
        };
        // marcada por padrão liga; senão o valor atual fica (com o nome novo, se mudou)
        let checked = p.checked_default || is_old(head.get_dict("V")?)?;
        let value = if checked { export.as_str() } else { "Off" };
        if p.checked_default {
            head.dict_put("DV", name_obj(&export)?)?;
        } else if is_old(head.get_dict("DV")?)? {
            // só o padrão que era deste botão sai (o de outro botão do grupo fica)
            head.dict_delete("DV")?;
        }
        head.dict_put("V", name_obj(value)?)?;
        obj.dict_put("AS", name_obj(value)?)?;
        // os outros botões do grupo ficam desligados se este foi marcado
        if checked {
            let me = obj.as_indirect()?;
            for mut kid in kids_of(head)? {
                if kid.as_indirect()? != me {
                    kid.dict_put("AS", name_obj("Off")?)?;
                }
            }
        }
        Ok(())
    }

    /// Tira o nó do campo de /Fields (ou do /Kids do pai).
    fn unlink_field(&mut self, head: &PdfObject) -> Result<()> {
        let num = head.as_indirect()?;
        match head.get_dict("Parent")? {
            Some(parent) => {
                if let Some(mut kids) = parent.get_dict("Kids")? {
                    remove_ref(&mut kids, num)?;
                }
            }
            None => {
                if let Some(mut fields) = acroform(&mut self.inner)?.get_dict("Fields")? {
                    remove_ref(&mut fields, num)?;
                }
            }
        }
        Ok(())
    }

    /// Move ou redimensiona o campo `id`.
    pub fn move_field(&mut self, page: i32, id: i32, rect: PdfRect) -> Result<()> {
        self.widget(page, id)?;
        self.with_snapshot(|d| {
            let mut a = d.widget(page, id)?.into_annotation();
            a.set_rect(Rect::from(rect))?;
            a.update()?;
            d.refresh_widgets(page, None)
        })
    }

    /// Exclui o campo `id` (o grupo de opção some junto com o último botão).
    pub fn delete_field(&mut self, page: i32, id: i32) -> Result<()> {
        self.widget(page, id)?;
        self.with_snapshot(|d| {
            let w = d.widget(page, id)?;
            let obj = w.annotation().object();
            let head = field_head(&obj)?;
            if head.as_indirect()? == id {
                d.unlink_field(&head)?;
            } else {
                let mut kids = kids_array(&head)?;
                remove_ref(&mut kids, id)?;
                if kids.len()? == 0 {
                    d.unlink_field(&head)?;
                }
            }
            let mut page_ref = d.inner.load_pdf_page(page)?;
            page_ref.delete_annotation(w.into_annotation())?;
            page_ref.update()?;
            Ok(())
        })
    }
}
