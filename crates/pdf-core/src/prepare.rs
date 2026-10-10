//! Preparar formulário (como no Acrobat): criar, editar, mover e excluir campos AcroForm. Cada
//! alteração entra no histórico como instantâneo, como Limpar formulário.
use crate::annot_edit::SNAPSHOT_BUDGET;
use crate::doc::{annot_id, PdfDoc};
use crate::error::{PdfError, Result};
use crate::forms::{field_head, kids_of, on_state, options_of};
use crate::types::PdfRect;
use mupdf::pdf::{PdfAnnotationType, PdfDocument, PdfObject, PdfWidget};
use mupdf::{Buffer, Matrix, Rect};

/// Tipo do campo a criar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewFieldKind {
    Text,
    Checkbox,
    Radio,
    Combo,
    List,
    Signature,
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
        props: FieldProps,
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
}

const READ_ONLY: i32 = 1;
const REQUIRED: i32 = 1 << 1;
const MULTILINE: i32 = 1 << 12;
const PASSWORD: i32 = 1 << 13;
const NO_TOGGLE_TO_OFF: i32 = 1 << 14;
const RADIO: i32 = 1 << 15;
const COMBO: i32 = 1 << 17;
const EDIT: i32 = 1 << 18;

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
    if acro.get_dict("DA")?.is_none() {
        acro.dict_put("DA", PdfObject::new_string("/Helv 0 Tf 0 g")?)?;
    }
    let mut dr = match acro.get_dict("DR")? {
        Some(dr) => dr,
        None => {
            acro.dict_put("DR", doc.new_dict()?)?;
            acro.get_dict("DR")?.expect("acabou de ser posto")
        }
    };
    let mut fonts = match dr.get_dict("Font")? {
        Some(f) => f,
        None => {
            dr.dict_put("Font", doc.new_dict()?)?;
            dr.get_dict("Font")?.expect("acabou de ser posto")
        }
    };
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

/// Aparências "ligado" (símbolo ZapfDingbats `glyph`) e "Off" (vazia) de uma caixa ou botão de
/// opção. A /Matrix contra-gira a rotação da página, como os campos do Acrobat (/MK /R).
fn button_ap(
    doc: &mut PdfDocument,
    w: f32,
    h: f32,
    rotation: i32,
    state: &str,
    glyph: char,
) -> Result<PdfObject> {
    // em pé na tela: numa página girada a caixa da aparência tem os lados trocados
    let (bw, bh) = if rotation % 180 == 0 { (w, h) } else { (h, w) };
    let size = bw.min(bh) * 0.8;
    // larguras do ZapfDingbats: ✔ ("4") 0,846 em; ● ("l") 0,791 em
    let glyph_w = if glyph == '4' { 0.846 } else { 0.791 } * size;
    let (x, y) = ((bw - glyph_w) / 2.0, (bh - size * 0.7) / 2.0);
    let m = Matrix::new_rotate(rotation as f32);
    let mut make = |ops: &str| -> Result<PdfObject> {
        let dict = doc.new_object_from_str(&format!(
            "<</Type/XObject/Subtype/Form/BBox[0 0 {bw} {bh}]/Matrix[{} {} {} {} 0 0]\
             /Resources<</Font<</ZaDb<</Type/Font/Subtype/Type1/BaseFont/ZapfDingbats>>>>>>>>",
            m.a, m.b, m.c, m.d
        ))?;
        Ok(doc.add_stream(&Buffer::from_bytes(ops.as_bytes())?, Some(&dict), false)?)
    };
    let on = make(&format!(
        "q 0 g BT /ZaDb {size:.2} Tf {x:.2} {y:.2} Td ({glyph}) Tj ET Q"
    ))?;
    let off = make("")?;
    let mut normal = doc.new_dict()?;
    normal.dict_put(state, on)?;
    normal.dict_put("Off", off)?;
    let mut ap = doc.new_dict()?;
    ap.dict_put("N", normal)?;
    Ok(ap)
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
    /// Executa `f` com instantâneo no histórico (desfazer volta tudo); se falhar, o documento volta
    /// como estava.
    fn edit_form<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let before = self.snapshot()?;
        match f(self) {
            Ok(v) => {
                if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
                    self.mark_unsaved();
                }
                Ok(v)
            }
            Err(e) => {
                self.restore(&before)?;
                Err(e)
            }
        }
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

    fn widget(&self, page: i32, id: i32) -> Result<PdfWidget> {
        self.check_page(page)?;
        self.inner
            .load_pdf_page(page)?
            .widgets()
            .find(|w| annot_id(w.annotation()).ok() == Some(id))
            .ok_or_else(|| invalid("campo não encontrado"))
    }

    /// Redesenha os campos da página (aparências geradas pelo MuPDF).
    fn refresh_widgets(&self, page: i32) -> Result<()> {
        for mut w in self.inner.load_pdf_page(page)?.widgets() {
            // um campo defeituoso de outro programa fica como está; os nossos são redesenhados
            let _ = w.update();
        }
        Ok(())
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
        let name = valid_name(name)?;
        let group = match (self.field_named(name)?, kind) {
            (None, _) => None,
            (Some(head), NewFieldKind::Radio) if flags_of(&head)? & RADIO != 0 => Some(head),
            (Some(_), _) => return Err(invalid("já existe um campo com esse nome")),
        };
        self.edit_form(|d| d.create_field(page, rect, kind, name, group))
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
        w.dict_put("DA", PdfObject::new_string("/Helv 0 Tf 0 g")?)?;
        let mut mk = self.inner.new_dict()?;
        if rotation != 0 {
            mk.dict_put("R", int(rotation)?)?;
        }
        let (rw, rh) = (rect.width(), rect.height());
        // o nó do campo: o próprio widget, ou o grupo de botões de opção
        let head_is_widget = kind != NewFieldKind::Radio;
        match kind {
            NewFieldKind::Text => {
                w.dict_put("FT", name_obj("Tx")?)?;
                w.dict_put("V", PdfObject::new_string("")?)?;
            }
            NewFieldKind::Checkbox => {
                w.dict_put("FT", name_obj("Btn")?)?;
                w.dict_put("V", name_obj("Off")?)?;
                w.dict_put("AS", name_obj("Off")?)?;
                mk.dict_put("CA", PdfObject::new_string("4")?)?;
                let ap = button_ap(&mut self.inner, rw, rh, rotation, CHECK_ON, '4')?;
                w.dict_put("AP", ap)?;
            }
            NewFieldKind::Radio => {
                let head = match group {
                    Some(g) => g,
                    None => {
                        let g =
                            self.inner
                                .add_object(&self.inner.new_object_from_str(&format!(
                                    "<</FT/Btn/Ff {}/V/Off/Kids[]>>",
                                    RADIO | NO_TOGGLE_TO_OFF
                                ))?)?;
                        let mut g2 = g.clone();
                        g2.dict_put("T", PdfObject::new_string(name)?)?;
                        acro.get_dict("Fields")?
                            .expect("acroform() garante /Fields")
                            .array_push(g.clone())?;
                        g
                    }
                };
                let state = next_choice(&head)?;
                w.dict_put("Parent", head.clone())?;
                w.dict_put("AS", name_obj("Off")?)?;
                mk.dict_put("CA", PdfObject::new_string("l")?)?;
                let ap = button_ap(&mut self.inner, rw, rh, rotation, &state, 'l')?;
                w.dict_put("AP", ap)?;
                head.get_dict("Kids")?
                    .expect("grupo tem /Kids")
                    .array_push(me.clone())?;
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
            acro.get_dict("Fields")?
                .expect("acroform() garante /Fields")
                .array_push(me)?;
        }
        annot.update()?;
        page_ref.update()?;
        self.refresh_widgets(page)?;
        Ok(id)
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
        }
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
        Ok(FieldProps {
            name: w.name()?.unwrap_or_default(),
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

    /// Troca as propriedades do campo `id` (desfazer volta tudo).
    pub fn set_field_props(&mut self, page: i32, id: i32, p: &FieldProps) -> Result<()> {
        let w = self.widget(page, id)?;
        let obj = w.annotation().object();
        let head = field_head(&obj)?;
        let radio = flags_of(&head)? & RADIO != 0;
        let name = valid_name(&p.name)?.to_owned();
        let current = w.name()?.unwrap_or_default();
        let target = if name == current {
            None
        } else {
            match self.field_named(&name)? {
                None => Some(None),
                Some(g) if radio && flags_of(&g)? & RADIO != 0 => Some(Some(g)),
                Some(_) => return Err(invalid("já existe um campo com esse nome")),
            }
        };
        if p.export.contains(['/', ' ']) {
            return Err(invalid(
                "o valor de exportação não pode ter espaço nem barra",
            ));
        }
        self.edit_form(|d| d.write_props(page, id, p, &name, target))
    }

    fn write_props(
        &mut self,
        page: i32,
        id: i32,
        p: &FieldProps,
        name: &str,
        target: Option<Option<PdfObject>>,
    ) -> Result<()> {
        let w = self.widget(page, id)?;
        let mut obj = w.annotation().object();
        let mut head = field_head(&obj)?;
        let radio = flags_of(&head)? & RADIO != 0;
        match target {
            // botão de opção trocado de grupo: sai do atual (que some se ficar vazio) e entra no outro
            Some(group) if radio => {
                let old = head.clone();
                remove_ref(&mut old.get_dict("Kids")?.expect("grupo tem /Kids"), id)?;
                if old.get_dict("Kids")?.map_or(Ok(0), |k| k.len())? == 0 {
                    self.unlink_field(&old)?;
                }
                head =
                    match group {
                        Some(g) => g,
                        None => {
                            let acro = acroform(&mut self.inner)?;
                            let g = self.inner.add_object(&self.inner.new_object_from_str(
                                &format!("<</FT/Btn/Ff {}/V/Off/Kids[]>>", flags_of(&old)?),
                            )?)?;
                            let mut g2 = g.clone();
                            g2.dict_put("T", PdfObject::new_string(name)?)?;
                            acro.get_dict("Fields")?
                                .expect("acroform() garante /Fields")
                                .array_push(g.clone())?;
                            g
                        }
                    };
                obj.dict_put("Parent", head.clone())?;
                head.get_dict("Kids")?
                    .expect("grupo tem /Kids")
                    .array_push(self.inner.new_indirect(id, 0)?)?;
            }
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
        let size = if p.font_size > 0.0 { p.font_size } else { 0.0 };
        obj.dict_put(
            "DA",
            PdfObject::new_string(&format!("/Helv {size} Tf 0 g"))?,
        )?;
        match ft.as_slice() {
            b"Tx" => {
                set_flag(&mut flags, MULTILINE, p.multiline);
                set_flag(&mut flags, PASSWORD, p.password);
                match p.max_len {
                    Some(n) if n > 0 => head.dict_put("MaxLen", int(n as i32)?)?,
                    _ => head.dict_delete("MaxLen")?,
                }
                head.dict_put("DV", PdfObject::new_string(&p.default)?)?;
                head.dict_put("V", PdfObject::new_string(&p.default)?)?;
            }
            b"Ch" => {
                if flags & COMBO != 0 {
                    set_flag(&mut flags, EDIT, p.editable);
                }
                let mut opt = self.inner.new_array()?;
                for (export, label) in &p.options {
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
            }
            b"Btn" => self.write_export(&mut obj, &mut head, p)?,
            _ => {}
        }
        head.dict_put("Ff", int(flags)?)?;
        self.refresh_widgets(page)
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
            if let Some(mut normal) = obj
                .get_dict("AP")?
                .and_then(|ap| ap.get_dict("N").ok().flatten())
            {
                if let Some(stream) = normal.get_dict(old.as_str())? {
                    normal.dict_put(export.as_str(), stream)?;
                    normal.dict_delete(old.as_str())?;
                }
            }
        }
        let on = |v: Option<PdfObject>| -> Result<bool> {
            Ok(v.map(|v| v.as_name()).transpose()?.as_deref() == Some(old.as_bytes()))
        };
        // marcada por padrão liga; senão o valor atual fica (com o nome novo, se mudou)
        let checked = p.checked_default || on(head.get_dict("V")?)?;
        let value = if checked { export.as_str() } else { "Off" };
        if p.checked_default {
            head.dict_put("DV", name_obj(&export)?)?;
        } else {
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
        self.edit_form(|d| {
            let mut a = d.widget(page, id)?.into_annotation();
            a.set_rect(Rect::from(rect))?;
            a.update()?;
            d.refresh_widgets(page)
        })
    }

    /// Exclui o campo `id` (o grupo de opção some junto com o último botão).
    pub fn delete_field(&mut self, page: i32, id: i32) -> Result<()> {
        self.widget(page, id)?;
        self.edit_form(|d| {
            let w = d.widget(page, id)?;
            let obj = w.annotation().object();
            let head = field_head(&obj)?;
            if head.as_indirect()? == id {
                d.unlink_field(&head)?;
            } else {
                let mut kids = head.get_dict("Kids")?.expect("pai tem /Kids");
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
