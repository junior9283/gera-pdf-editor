//! Ações de campo e de página (como a aba Ações do Acrobat): leitura e escrita de /A, /AA e das
//! cadeias /Next. O JavaScript é guardado e não executa; só a chamada exata que o campo Imagem do
//! Acrobat grava é reconhecida.
use crate::doc::PdfDoc;
use crate::error::Result;
use mupdf::pdf::{PdfDocument, PdfObject};

/// Gatilho de uma ação de campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldTrigger {
    MouseUp,
    MouseDown,
    MouseEnter,
    MouseExit,
    Focus,
    Blur,
}

impl FieldTrigger {
    pub const ALL: [FieldTrigger; 6] = [
        Self::MouseUp,
        Self::MouseDown,
        Self::MouseEnter,
        Self::MouseExit,
        Self::Focus,
        Self::Blur,
    ];

    /// Chave em /AA.
    fn key(self) -> &'static str {
        match self {
            Self::MouseUp => "U",
            Self::MouseDown => "D",
            Self::MouseEnter => "E",
            Self::MouseExit => "X",
            Self::Focus => "Fo",
            Self::Blur => "Bl",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MouseUp => "Mouse para cima",
            Self::MouseDown => "Mouse para baixo",
            Self::MouseEnter => "Mouse entra",
            Self::MouseExit => "Mouse sai",
            Self::Focus => "Em foco",
            Self::Blur => "Fora de foco",
        }
    }
}

/// Gatilho de uma ação de página.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageTrigger {
    Open,
    Close,
}

impl PageTrigger {
    pub const ALL: [PageTrigger; 2] = [Self::Open, Self::Close];

    fn key(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::Close => "C",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Abrir página",
            Self::Close => "Fechar página",
        }
    }
}

/// Uma ação (as do menu "Selecionar ação" do Acrobat que o app entende).
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// Ir para uma página deste documento (0 = primeira).
    GoTo { page: i32 },
    /// Abrir um link da web.
    Uri(String),
    /// Mostrar ou ocultar campos.
    ShowHide { fields: Vec<String>, hide: bool },
    /// Redefinir um formulário: os campos listados (ou todos, se vazio); com `exclude`, todos
    /// menos eles.
    Reset { fields: Vec<String>, exclude: bool },
    /// Executar um item de menu (/Named: NextPage, PrevPage, FirstPage, LastPage, Print…).
    Named(String),
    /// Abrir um arquivo.
    Launch(String),
    /// Executar um JavaScript (gravado, não executado).
    JavaScript(String),
    /// Campo Imagem do Acrobat: escolher a imagem do botão (`event.target.buttonImportIcon();`).
    ImportIcon,
    /// Enviar o formulário para um endereço (http, https ou mailto), no formato pedido.
    Submit {
        url: String,
        format: crate::form_data::SubmitFormat,
    },
    /// Outra ação (multimídia, 3D, Enviar…), preservada como estava: (tipo, objeto em texto).
    Other { kind: String, raw: String },
}

impl ActionKind {
    /// Nome da ação, como na lista do Acrobat.
    pub fn label(&self) -> String {
        match self {
            Self::GoTo { page } => format!("Ir para a página {}", page + 1),
            Self::Uri(u) => format!("Abrir um link da web: {u}"),
            Self::ShowHide { fields, hide } => format!(
                "{} {}",
                if *hide { "Ocultar" } else { "Mostrar" },
                fields.join(", ")
            ),
            Self::Reset { fields, .. } if fields.is_empty() => "Redefinir o formulário".into(),
            Self::Reset { fields, exclude } => format!(
                "Redefinir {}{}",
                if *exclude { "todos menos " } else { "" },
                fields.join(", ")
            ),
            Self::Named(n) => format!("Executar um item de menu: {}", named_label(n)),
            Self::Launch(f) => format!("Abrir um arquivo: {f}"),
            Self::JavaScript(_) => "Executar um JavaScript (não executado pelo app)".into(),
            Self::ImportIcon => "Escolher a imagem do botão".into(),
            Self::Submit { url, format } => {
                format!("Enviar o formulário ({}) para {url}", format.label())
            }
            Self::Other { kind, .. } => format!("Outra ação ({kind})"),
        }
    }
}

/// Itens de menu (/Named) que o app executa, com o nome em português.
pub const NAMED_ITEMS: [(&str, &str); 9] = [
    ("NextPage", "Próxima página"),
    ("PrevPage", "Página anterior"),
    ("FirstPage", "Primeira página"),
    ("LastPage", "Última página"),
    ("Print", "Imprimir…"),
    ("GoBack", "Voltar à exibição anterior"),
    ("GoForward", "Avançar para a próxima exibição"),
    ("FullScreen", "Tela cheia"),
    ("Find", "Localizar"),
];

pub fn named_label(n: &str) -> &str {
    NAMED_ITEMS
        .iter()
        .find(|(k, _)| *k == n)
        .map_or(n, |(_, l)| *l)
}

/// O script que o campo Imagem do Acrobat grava.
const IMPORT_ICON: &str = "event.target.buttonImportIcon();";
/// Teto de ações lidas de uma cadeia (um PDF hostil pode ter uma enorme ou em ciclo).
const MAX_CHAIN: usize = 64;
/// Maior ação desconhecida preservada (em texto).
const MAX_RAW: usize = 64 * 1024;
/// Maior script de ação lido.
const MAX_JS: usize = 70_000;

fn name_of(obj: &PdfObject, key: &str) -> Option<String> {
    crate::pdf_obj::get(obj, key).and_then(|n| crate::pdf_obj::name_of(&n))
}

fn text(obj: &PdfObject) -> Result<String> {
    if obj.is_stream()? {
        let bytes = crate::attachments::read_capped(obj, MAX_JS).unwrap_or_default();
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    Ok(obj.as_string().unwrap_or_default())
}

/// Nomes de campos de /T ou /Fields: strings ou referências aos campos.
fn field_list(obj: Option<PdfObject>) -> Result<Vec<String>> {
    let Some(obj) = obj else {
        return Ok(Vec::new());
    };
    let name = |o: &PdfObject| -> Result<String> {
        Ok(if o.is_string()? {
            o.as_string()?
        } else {
            crate::prepare::full_name(o)?
        })
    };
    if !obj.is_array()? {
        return Ok(vec![name(&obj)?]);
    }
    let mut out = Vec::new();
    for k in 0..obj.len()?.min(10_000) as i32 {
        if let Some(item) = obj.get_array(k)? {
            out.push(name(&item)?);
        }
    }
    Ok(out)
}

/// Uma ação (sem a /Next).
fn read_one(doc: &PdfDocument, a: &PdfObject) -> Result<ActionKind> {
    let kind = name_of(a, "S").unwrap_or_default();
    Ok(match kind.as_str() {
        "GoTo" => {
            let page = a
                .get_dict("D")?
                .filter(|d| d.is_array().unwrap_or(false))
                .and_then(|d| d.get_array(0).ok().flatten())
                .and_then(|p| {
                    if p.is_int().unwrap_or(false) {
                        p.as_int().ok()
                    } else {
                        doc.lookup_page_number(&p).ok()
                    }
                });
            // página fora do documento (PDF hostil ou mal feito): preservada como estava
            let count = doc.page_count().unwrap_or(0);
            match page {
                Some(page) if (0..count).contains(&page) => ActionKind::GoTo { page },
                _ => other(a, &kind)?,
            }
        }
        "URI" => ActionKind::Uri(
            a.get_dict("URI")?
                .map(|u| text(&u))
                .transpose()?
                .unwrap_or_default(),
        ),
        "Hide" => ActionKind::ShowHide {
            fields: field_list(a.get_dict("T")?)?,
            hide: a
                .get_dict("H")?
                .map(|h| h.as_bool())
                .transpose()?
                .unwrap_or(true),
        },
        "ResetForm" => ActionKind::Reset {
            fields: field_list(a.get_dict("Fields")?)?,
            exclude: a
                .get_dict("Flags")?
                .and_then(|f| f.as_int().ok())
                .is_some_and(|f| f & 1 != 0),
        },
        "Named" => ActionKind::Named(name_of(a, "N").unwrap_or_default()),
        // só /F e /Flags com o formato: com /Fields ou outros bits, fica como está (Other)
        "SubmitForm" if a.get_dict("Fields")?.is_none() => {
            let url = match a.get_dict("F")? {
                Some(f) if f.is_dict()? => crate::pdf_obj::text(&f, "F"),
                Some(f) => f.as_string().unwrap_or_default(),
                None => String::new(),
            };
            let flags = crate::pdf_obj::get(a, "Flags")
                .and_then(|f| f.as_int().ok())
                .unwrap_or(0);
            match crate::form_data::SubmitFormat::from_flags(flags) {
                Some(format) => ActionKind::Submit { url, format },
                None => other(a, &kind)?,
            }
        }
        "Launch" => {
            let file = match a.get_dict("F")? {
                Some(f) if f.is_dict()? => f
                    .get_dict("UF")?
                    .or(f.get_dict("F")?)
                    .map(|s| s.as_string())
                    .transpose()?
                    .unwrap_or_default(),
                Some(f) => f.as_string().unwrap_or_default(),
                None => String::new(),
            };
            ActionKind::Launch(file)
        }
        "JavaScript" => {
            let js = a
                .get_dict("JS")?
                .map(|j| text(&j))
                .transpose()?
                .unwrap_or_default();
            if js.trim() == IMPORT_ICON {
                ActionKind::ImportIcon
            } else {
                ActionKind::JavaScript(js)
            }
        }
        _ => other(a, &kind)?,
    })
}

/// Ação desconhecida guardada em texto (sem a /Next), para ser regravada igual.
fn other(a: &PdfObject, kind: &str) -> Result<ActionKind> {
    let mut copy = a.copy_dict()?;
    copy.dict_delete("Next")?;
    let raw = copy.to_string();
    Ok(ActionKind::Other {
        kind: kind.to_string(),
        raw: if raw.len() > MAX_RAW {
            String::new()
        } else {
            raw
        },
    })
}

/// A cadeia de ações a partir de `a` (/Next em dicionário ou array), na ordem, com teto e sem
/// repetir objetos (ciclo).
fn read_chain(doc: &PdfDocument, a: PdfObject, out: &mut Vec<ActionKind>) -> Result<()> {
    let mut stack = vec![a];
    let mut seen = std::collections::HashSet::new();
    while let Some(a) = stack.pop() {
        if out.len() >= MAX_CHAIN {
            break;
        }
        // só objetos indiretos se repetem num ciclo (o número de um direto é 0)
        if a.is_indirect()? && !seen.insert(a.as_indirect()?) {
            continue;
        }
        if !a.is_dict()? {
            continue;
        }
        out.push(read_one(doc, &a)?);
        if let Some(next) = a.get_dict("Next")? {
            if next.is_array()? {
                let n = next.len()?.min(MAX_CHAIN) as i32;
                for k in (0..n).rev() {
                    if let Some(item) = next.get_array(k)? {
                        stack.push(item);
                    }
                }
            } else {
                stack.push(next);
            }
        }
    }
    Ok(())
}

/// As ações do widget, por gatilho (Mouse para cima: /A e /AA /U).
pub(crate) fn read_field_actions(
    doc: &PdfDocument,
    w: &PdfObject,
) -> Result<Vec<(FieldTrigger, ActionKind)>> {
    let out = read_actions_of(doc, w)?;
    // sem ações no widget: as do nó do campo (outros programas as põem lá)
    let head = crate::forms::field_head(w)?;
    if out.is_empty() && head.as_indirect()? != w.as_indirect()? {
        return read_actions_of(doc, &head);
    }
    Ok(out)
}

fn read_actions_of(doc: &PdfDocument, w: &PdfObject) -> Result<Vec<(FieldTrigger, ActionKind)>> {
    let mut out = Vec::new();
    let aa = w.get_dict("AA")?.filter(|aa| aa.is_dict().unwrap_or(false));
    for t in FieldTrigger::ALL {
        let mut list = Vec::new();
        if t == FieldTrigger::MouseUp {
            if let Some(a) = w.get_dict("A")? {
                read_chain(doc, a, &mut list)?;
            }
        }
        if let Some(a) = aa
            .as_ref()
            .and_then(|aa| aa.get_dict(t.key()).ok().flatten())
        {
            read_chain(doc, a, &mut list)?;
        }
        out.extend(list.into_iter().map(|a| (t, a)));
    }
    Ok(out)
}

/// Dicionário de uma ação.
fn action_obj(doc: &PdfDocument, a: &ActionKind) -> Result<Option<PdfObject>> {
    let mut d = doc.new_dict()?;
    let s = |d: &mut PdfObject, name: &str| d.dict_put("S", PdfObject::new_name(name)?);
    let names = |list: &[String]| -> Result<PdfObject> {
        let mut arr = doc.new_array()?;
        for n in list {
            arr.array_push(PdfObject::new_string(n)?)?;
        }
        Ok(arr)
    };
    match a {
        ActionKind::GoTo { page } => {
            s(&mut d, "GoTo")?;
            let mut dest = doc.new_array()?;
            dest.array_push(doc.find_page(*page)?)?;
            dest.array_push(PdfObject::new_name("Fit")?)?;
            d.dict_put("D", dest)?;
        }
        ActionKind::Uri(u) => {
            s(&mut d, "URI")?;
            d.dict_put("URI", PdfObject::new_string(u)?)?;
        }
        ActionKind::ShowHide { fields, hide } => {
            s(&mut d, "Hide")?;
            d.dict_put("T", names(fields)?)?;
            d.dict_put("H", PdfObject::new_bool(*hide))?;
        }
        ActionKind::Reset { fields, exclude } => {
            s(&mut d, "ResetForm")?;
            if !fields.is_empty() {
                d.dict_put("Fields", names(fields)?)?;
            }
            if *exclude {
                d.dict_put("Flags", PdfObject::new_int(1)?)?;
            }
        }
        ActionKind::Named(n) => {
            s(&mut d, "Named")?;
            d.dict_put("N", PdfObject::new_name(n)?)?;
        }
        ActionKind::Launch(f) => {
            s(&mut d, "Launch")?;
            d.dict_put("F", PdfObject::new_string(f)?)?;
        }
        ActionKind::JavaScript(js) => {
            s(&mut d, "JavaScript")?;
            d.dict_put("JS", PdfObject::new_string(js)?)?;
        }
        ActionKind::Submit { url, format } => {
            s(&mut d, "SubmitForm")?;
            d.dict_put("F", PdfObject::new_string(url)?)?;
            d.dict_put("Flags", PdfObject::new_int(format.flags())?)?;
        }
        ActionKind::ImportIcon => {
            s(&mut d, "JavaScript")?;
            d.dict_put("JS", PdfObject::new_string(IMPORT_ICON)?)?;
        }
        ActionKind::Other { raw, .. } if raw.is_empty() => return Ok(None),
        ActionKind::Other { raw, .. } => match doc.new_object_from_str(raw) {
            Ok(o) if o.is_dict()? => return Ok(Some(o)),
            _ => return Ok(None),
        },
    }
    Ok(Some(d))
}

/// As ações de um gatilho, na ordem.
fn of_trigger<T: PartialEq + Copy>(list: &[(T, ActionKind)], t: T) -> Vec<&ActionKind> {
    list.iter()
        .filter(|(x, _)| *x == t)
        .map(|(_, a)| a)
        .collect()
}

/// A cadeia: a primeira ação com as outras em /Next (array), como o Acrobat grava.
fn chain_obj(doc: &PdfDocument, list: &[&ActionKind]) -> Result<Option<PdfObject>> {
    let mut objs = Vec::new();
    for a in list {
        if let Some(o) = action_obj(doc, a)? {
            objs.push(o);
        }
    }
    let mut iter = objs.into_iter();
    let Some(mut first) = iter.next() else {
        return Ok(None);
    };
    let rest: Vec<PdfObject> = iter.collect();
    if !rest.is_empty() {
        let mut next = doc.new_array()?;
        for o in rest {
            next.array_push(o)?;
        }
        first.dict_put("Next", next)?;
    }
    Ok(Some(first))
}

/// Grava as ações do widget: Mouse para cima em /A e as outras em /AA (os gatilhos de valor do
/// campo — Tecla, Formatar, Validar, Calcular — ficam como estão).
pub(crate) fn write_field_actions(
    doc: &PdfDocument,
    w: &mut PdfObject,
    list: &[(FieldTrigger, ActionKind)],
) -> Result<()> {
    let mut before = read_actions_of(doc, w)?;
    // ações que estavam no nó do campo passam para o widget (senão voltariam ao ser tiradas)
    let mut head = crate::forms::field_head(w)?;
    if before.is_empty() && head.as_indirect()? != w.as_indirect()? {
        if !read_actions_of(doc, &head)?.is_empty() {
            head.dict_delete("A")?;
            if let Some(mut aa) = head
                .get_dict("AA")?
                .filter(|aa| aa.is_dict().unwrap_or(false))
            {
                for t in FieldTrigger::ALL {
                    aa.dict_delete(t.key())?;
                }
            }
        }
        before = Vec::new();
    }
    for t in FieldTrigger::ALL {
        let mine = of_trigger(list, t);
        // gatilho sem mudança fica como está no arquivo (o modelo do app resume algumas ações)
        if of_trigger(&before, t) == mine {
            continue;
        }
        let chain = chain_obj(doc, &mine)?;
        if t == FieldTrigger::MouseUp {
            w.dict_delete("A")?;
        }
        let aa = match w.get_dict("AA")? {
            Some(aa) if aa.is_dict()? => Some(aa),
            _ => None,
        };
        if let Some(mut aa) = aa.clone() {
            aa.dict_delete(t.key())?;
        }
        if let Some(chain) = chain {
            if t == FieldTrigger::MouseUp {
                w.dict_put("A", chain)?;
            } else {
                let mut aa = match aa {
                    Some(aa) => aa,
                    None => {
                        w.dict_put("AA", doc.new_dict()?)?;
                        w.get_dict("AA")?.expect("acabou de ser posto")
                    }
                };
                aa.dict_put(t.key(), chain)?;
            }
        }
    }
    if w.get_dict("AA")?
        .is_some_and(|aa| aa.dict_len().unwrap_or(1) == 0)
    {
        w.dict_delete("AA")?;
    }
    Ok(())
}

impl PdfDoc {
    /// Ações da página (Abrir e Fechar página).
    pub fn page_actions(&self, page: i32) -> Result<Vec<(PageTrigger, ActionKind)>> {
        self.check_page(page)?;
        let obj = self.inner.find_page(page)?;
        let mut out = Vec::new();
        if let Some(aa) = obj
            .get_dict("AA")?
            .filter(|aa| aa.is_dict().unwrap_or(false))
        {
            for t in PageTrigger::ALL {
                if let Some(a) = aa.get_dict(t.key())? {
                    let mut list = Vec::new();
                    read_chain(&self.inner, a, &mut list)?;
                    out.extend(list.into_iter().map(|a| (t, a)));
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn write_page_actions(
        &mut self,
        page: i32,
        list: &[(PageTrigger, ActionKind)],
    ) -> Result<()> {
        let before = self.page_actions(page)?;
        let mut obj = self.inner.find_page(page)?;
        let mut aa = match obj.get_dict("AA")? {
            Some(aa) if aa.is_dict()? => aa,
            _ => self.inner.new_dict()?,
        };
        for t in PageTrigger::ALL {
            let mine = of_trigger(list, t);
            // gatilho sem mudança fica como está no arquivo
            if of_trigger(&before, t) == mine {
                continue;
            }
            aa.dict_delete(t.key())?;
            if let Some(chain) = chain_obj(&self.inner, &mine)? {
                aa.dict_put(t.key(), chain)?;
            }
        }
        if aa.dict_len()? > 0 {
            obj.dict_put("AA", aa)?;
        } else {
            obj.dict_delete("AA")?;
        }
        Ok(())
    }

    /// Mostrar/ocultar: liga ou desliga o bit Oculto dos widgets dos campos (e dos de baixo, para
    /// um nome de grupo). Não entra no desfazer (como no Acrobat), mas marca o documento como
    /// alterado. Devolve se algo mudou.
    pub fn show_fields(&mut self, names: &[String], hide: bool) -> Result<bool> {
        use crate::forms::HIDDEN;
        let mut changed = false;
        for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                let name = w.name().ok().flatten().unwrap_or_default();
                let listed = crate::forms::named_in(&name, names);
                if !listed {
                    continue;
                }
                let mut obj = w.annotation().object();
                let flags = obj
                    .get_dict("F")?
                    .and_then(|f| f.as_int().ok())
                    .unwrap_or(0);
                let new = if hide {
                    flags | HIDDEN
                } else {
                    flags & !HIDDEN
                };
                if new != flags {
                    obj.dict_put("F", PdfObject::new_int(new)?)?;
                    changed = true;
                }
            }
        }
        if changed {
            self.mark_unsaved();
        }
        Ok(changed)
    }
}
