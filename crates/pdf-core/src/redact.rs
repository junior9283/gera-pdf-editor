//! Redigir (Acrobat): localizar o que redigir (frase ou padrões), aplicar as marcas de redação
//! de forma permanente e limpar as informações ocultas do documento.
//!
//! As marcas são anotações `/Redact` (criadas pelo caminho das anotações, com desfazer). Aplicar
//! usa a redação do MuPDF — o texto sob a marca sai, os pixels das imagens são apagados e os
//! desenhos cobertos saem — e pinta cada caixa com a cor e o texto sobreposto escolhidos.
use crate::doc::PdfDoc;
use crate::edit_text::{glyph_run, width};
use crate::error::Result;
use crate::page_content::{
    invert, matrix_ops, num, own_resource, page_content, rgb_ops, Lexer, Tok,
};
use crate::types::PdfRect;
use mupdf::pdf::{
    PdfAnnotationType, PdfObject, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::{Buffer, Font, Rect, TextPageFlags};
use regex::Regex;

/// Dado sensível que o Localizar sabe achar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactPattern {
    Cpf,
    Cnpj,
    Phone,
    Email,
    CreditCard,
    Cep,
    Date,
}

impl RedactPattern {
    pub const ALL: [RedactPattern; 7] = [
        RedactPattern::Cpf,
        RedactPattern::Cnpj,
        RedactPattern::Phone,
        RedactPattern::Email,
        RedactPattern::CreditCard,
        RedactPattern::Cep,
        RedactPattern::Date,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RedactPattern::Cpf => "CPF",
            RedactPattern::Cnpj => "CNPJ",
            RedactPattern::Phone => "Telefone",
            RedactPattern::Email => "E-mail",
            RedactPattern::CreditCard => "Cartão de crédito",
            RedactPattern::Cep => "CEP",
            RedactPattern::Date => "Data",
        }
    }

    fn regex(self) -> &'static str {
        // só algarismos ASCII ([0-9]): o \d do Unicode casaria dígitos de outras escritas
        match self {
            RedactPattern::Cpf => r"\b[0-9]{3}[. ]?[0-9]{3}[. ]?[0-9]{3}[- ]?[0-9]{2}\b",
            RedactPattern::Cnpj => r"\b[0-9]{2}\.?[0-9]{3}\.?[0-9]{3}/?[0-9]{4}-?[0-9]{2}\b",
            // com DDD (entre parênteses ou não) e +55 opcional: "2019-2023" não é telefone
            RedactPattern::Phone => {
                r"(?:\+55\s?)?(?:\([0-9]{2}\)|\b[0-9]{2})\s?9?[0-9]{4}-?[0-9]{4}\b"
            }
            RedactPattern::Email => r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+",
            // 13 a 19 algarismos, com espaços ou hífens entre eles (Visa, Master, Amex…)
            RedactPattern::CreditCard => r"\b[0-9](?:[ -]?[0-9]){12,18}\b",
            RedactPattern::Cep => r"\b[0-9]{2}\.?[0-9]{3}-[0-9]{3}\b",
            RedactPattern::Date => {
                r"\b(?:[0-9]{1,2}[/.][0-9]{1,2}[/.][0-9]{4}|[0-9]{1,2}/[0-9]{1,2}/[0-9]{2}|[0-9]{4}-[0-9]{2}-[0-9]{2})\b"
            }
        }
    }

    /// Confere o achado além do formato: dígitos verificadores (CPF, CNPJ) e Luhn (cartão).
    fn accepts(self, found: &str) -> bool {
        let d: Vec<u32> = found.chars().filter_map(|c| c.to_digit(10)).collect();
        match self {
            RedactPattern::Cpf => {
                d.len() == 11 && d.iter().any(|&x| x != d[0]) && {
                    let dv = |n: usize| {
                        let sum: u32 = (0..n).map(|i| d[i] * (n as u32 + 1 - i as u32)).sum();
                        (sum * 10 % 11) % 10
                    };
                    dv(9) == d[9] && dv(10) == d[10]
                }
            }
            RedactPattern::Cnpj => {
                const W: [u32; 13] = [6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2];
                d.len() == 14 && {
                    let dv = |n: usize| {
                        let sum: u32 = (0..n).map(|i| d[i] * W[13 - n + i]).sum();
                        if sum % 11 < 2 {
                            0
                        } else {
                            11 - sum % 11
                        }
                    };
                    dv(12) == d[12] && dv(13) == d[13]
                }
            }
            RedactPattern::CreditCard => {
                let sum: u32 = d
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, &x)| {
                        if i % 2 == 1 {
                            (x * 2) % 10 + x * 2 / 10
                        } else {
                            x
                        }
                    })
                    .sum();
                sum.is_multiple_of(10)
            }
            _ => true,
        }
    }
}

/// O que procurar.
#[derive(Debug, Clone, PartialEq)]
pub enum RedactQuery {
    /// Frase, sem diferenciar maiúsculas.
    Phrase(String),
    Patterns(Vec<RedactPattern>),
}

/// Um trecho achado: a página, as caixas (na tela) e o texto.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactHit {
    pub page: i32,
    pub rects: Vec<PdfRect>,
    pub text: String,
}

/// Aparência das caixas ao aplicar: cor, texto sobreposto e a cor dele.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactLook {
    pub fill: [u8; 3],
    pub overlay: String,
    pub overlay_color: [u8; 3],
}

impl Default for RedactLook {
    /// Como o Acrobat: caixa preta, sem texto sobreposto (vermelho quando houver).
    fn default() -> Self {
        Self {
            fill: [0, 0, 0],
            overlay: String::new(),
            overlay_color: [255, 0, 0],
        }
    }
}

/// O que o Limpar documento tira.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SanitizeOptions {
    /// Propriedades (/Info) e metadados XMP.
    pub metadata: bool,
    /// Arquivos anexados.
    pub attachments: bool,
    /// JavaScript e ações automáticas.
    pub scripts: bool,
    /// Comentários (anotações, menos campos de formulário e links).
    pub comments: bool,
}

impl Default for SanitizeOptions {
    /// Como o Acrobat sugere: tudo menos os comentários.
    fn default() -> Self {
        Self {
            metadata: true,
            attachments: true,
            scripts: true,
            comments: false,
        }
    }
}

impl SanitizeOptions {
    /// Alguma coisa a tirar?
    pub fn any(&self) -> bool {
        self.metadata || self.attachments || self.scripts || self.comments
    }
}

/// O texto sobreposto já preparado (fonte, largura a 1 pt e glifos), igual em todas as caixas.
struct Overlay {
    font: Font,
    unit: f32,
    run: String,
}

impl Overlay {
    fn new(look: &RedactLook) -> Result<Option<Self>> {
        let text = look.overlay.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let font = Font::new("Helvetica-Bold")?;
        Ok(Some(Self {
            unit: width(&font, text, 1.0)?.max(0.01),
            run: glyph_run(&font, text)?,
            font,
        }))
    }
}

/// Chaves de texto alternativo do marcado (o que leitores usam para copiar e extrair).
const ALT_KEYS: [&str; 3] = ["ActualText", "Alt", "E"];

fn has_alternate_text(dict: &PdfObject) -> bool {
    ALT_KEYS
        .iter()
        .any(|k| dict.get_dict(*k).ok().flatten().is_some())
}

/// `content` sem as faixas `cuts` (ordenadas), cada uma trocada por um espaço.
fn cut(content: &[u8], cuts: &[std::ops::Range<usize>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    for r in cuts {
        out.extend(&content[at..r.start]);
        out.push(b' ');
        at = r.end;
    }
    out.extend(&content[at..]);
    out
}

/// Tira o texto alternativo das propriedades nomeadas do dicionário de recursos (no lugar).
fn clean_properties(resources: &PdfObject) -> Result<()> {
    let Some(props) = resources.get_dict("Properties")? else {
        return Ok(());
    };
    for (_, mut v) in props.dict_iter()?.filter_map(|e| e.ok()) {
        if v.is_dict()? {
            for key in ALT_KEYS {
                v.dict_delete(key)?;
            }
        }
    }
    Ok(())
}

/// Tira o texto alternativo dos formulários do dicionário /XObject e dos de dentro deles
/// (`seen`: cada formulário uma vez; profundidade limitada contra arquivos hostis).
fn clean_forms(
    xobjects: &PdfObject,
    seen: &mut std::collections::HashSet<i32>,
    depth: u32,
) -> Result<()> {
    if depth > 16 {
        return Ok(());
    }
    for (_, mut form) in xobjects.dict_iter()?.filter_map(|e| e.ok()) {
        let is_form = form
            .get_dict("Subtype")?
            .and_then(|s| s.as_name().ok())
            .is_some_and(|s| s == b"Form");
        let num = form.as_indirect().unwrap_or(0);
        if !is_form || (num != 0 && !seen.insert(num)) {
            continue;
        }
        let content = form.read_stream()?;
        let cuts = alternate_text_marks(&content);
        if !cuts.is_empty() {
            form.write_stream_buffer(&Buffer::from_bytes(&cut(&content, &cuts))?)?;
        }
        if let Some(res) = form.get_dict("Resources")? {
            clean_properties(&res)?;
            if let Some(inner) = res.get_dict("XObject")? {
                clean_forms(&inner, seen, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Tira o texto alternativo dos elementos da árvore de estrutura (PDF marcado) que ficam nas
/// páginas redigidas (`pages`: números dos objetos delas); o elemento sem /Pg herda o do pai.
fn clean_structure(doc: &mupdf::pdf::PdfDocument, pages: &[i32]) -> Result<()> {
    fn walk(
        node: &PdfObject,
        page: Option<i32>,
        pages: &[i32],
        seen: &mut std::collections::HashSet<i32>,
        depth: u32,
    ) -> Result<()> {
        if depth > 64 {
            return Ok(());
        }
        if node.is_array()? {
            for i in 0..node.len()? as i32 {
                if let Some(kid) = node.get_array(i)? {
                    walk(&kid, page, pages, seen, depth + 1)?;
                }
            }
            return Ok(());
        }
        if !node.is_dict()? {
            return Ok(());
        }
        let num = node.as_indirect().unwrap_or(0);
        if num != 0 && !seen.insert(num) {
            return Ok(());
        }
        let page = node
            .get_dict("Pg")?
            .and_then(|p| p.as_indirect().ok())
            .or(page);
        if page.is_some_and(|p| pages.contains(&p)) {
            let mut elem = node.clone();
            for key in ALT_KEYS {
                elem.dict_delete(key)?;
            }
        }
        if let Some(kids) = node.get_dict("K")? {
            walk(&kids, page, pages, seen, depth + 1)?;
        }
        Ok(())
    }
    if let Some(root) = doc.catalog()?.get_dict("StructTreeRoot")? {
        if let Some(kids) = root.get_dict("K")? {
            walk(&kids, None, pages, &mut std::collections::HashSet::new(), 0)?;
        }
    }
    Ok(())
}

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Apaga os campos de formulário e os comentários (com o popup) que cruzam as caixas — o
/// Acrobat também os tira; links e popups a redação do MuPDF já resolve. Devolve os números dos
/// objetos dos campos apagados, para tirá-los também do /AcroForm.
fn remove_covered_annots(page: &mut mupdf::pdf::PdfPage, boxes: &[Rect]) -> Result<Vec<i32>> {
    let covered = |r: Rect| boxes.iter().any(|b| intersects(&r, b));
    let mut widgets = Vec::new();
    let mut fields = Vec::new();
    for w in page.widgets() {
        if covered(w.annotation().rect()?) {
            fields.push(w.xref()?);
            widgets.push(w);
        }
    }
    for w in widgets {
        page.delete_widget(w)?;
    }
    let mut doomed = Vec::new();
    for a in page.annotations() {
        let keep = matches!(
            a.r#type()?,
            PdfAnnotationType::Link | PdfAnnotationType::Popup | PdfAnnotationType::Redact
        );
        if !keep && covered(a.rect()?) {
            doomed.push(a);
        }
    }
    for a in doomed {
        // o popup vai junto (senão sobra uma janela com o comentário)
        if let Some(popup) = a.object().get_dict("Popup")? {
            let num = popup.as_indirect().unwrap_or(0);
            let p = page
                .annotations()
                .find(|x| x.object().as_indirect().ok() == Some(num));
            if let Some(p) = p {
                page.delete_annotation(p)?;
            }
        }
        page.delete_annotation(a)?;
    }
    Ok(fields)
}

/// Tira de /AcroForm /Fields (e dos /Kids) os campos apagados; o pai que fica sem filhos sai.
fn prune_fields(doc: &mupdf::pdf::PdfDocument, removed: &[i32]) -> Result<()> {
    fn prune(arr: &mut mupdf::pdf::PdfObject, removed: &[i32], depth: u32) -> Result<()> {
        if depth > 32 {
            return Ok(()); // árvore de campos absurda (ou em ciclo): para por aqui
        }
        let mut i = 0;
        while i < arr.len()? as i32 {
            let Some(mut field) = arr.get_array(i)? else {
                i += 1;
                continue;
            };
            let gone = field.as_indirect().is_ok_and(|n| removed.contains(&n));
            let mut empty = false;
            if !gone {
                if let Some(mut kids) = field.get_dict("Kids")? {
                    prune(&mut kids, removed, depth + 1)?;
                    empty = kids.len()? == 0;
                    if !empty {
                        field.dict_put("Kids", kids)?;
                    }
                }
            }
            if gone || empty {
                arr.array_delete(i)?;
            } else {
                i += 1;
            }
        }
        Ok(())
    }
    let catalog = doc.catalog()?;
    if let Some(mut fields) = catalog
        .get_dict("AcroForm")?
        .and_then(|f| f.get_dict("Fields").ok().flatten())
    {
        prune(&mut fields, removed, 0)?;
    }
    Ok(())
}

/// As faixas `/Tag <<…/ActualText…>> BDC` e o `EMC` correspondente (o texto alternativo, que
/// sobra depois da redação), para tirar do conteúdo deixando o que está entre eles.
fn alternate_text_marks(content: &[u8]) -> Vec<std::ops::Range<usize>> {
    let mut lx = Lexer {
        s: content,
        i: 0,
        inline_image: false,
    };
    let mut first_operand: Option<usize> = None;
    let mut names: Vec<&[u8]> = Vec::new();
    // pilha do marcado: a faixa do BDC quando ele tem texto alternativo
    let mut stack: Vec<Option<std::ops::Range<usize>>> = Vec::new();
    let mut out = Vec::new();
    while let Some((start, tok)) = lx.next() {
        let Tok::Op(op) = tok else {
            first_operand.get_or_insert(start);
            if let Tok::Name(n) = tok {
                names.push(n);
            }
            continue;
        };
        match op {
            b"BDC" | b"BMC" => {
                let alt = op == b"BDC"
                    && names
                        .iter()
                        .any(|n| matches!(*n, b"ActualText" | b"Alt" | b"E"));
                stack.push(alt.then(|| first_operand.unwrap_or(start)..lx.i));
            }
            b"EMC" => {
                if let Some(Some(open)) = stack.pop() {
                    out.push(open);
                    out.push(start..lx.i);
                }
            }
            _ => {}
        }
        first_operand = None;
        names.clear();
    }
    out.sort_by_key(|r| r.start);
    out
}

impl PdfDoc {
    /// Os trechos de todas as páginas que combinam com a consulta, na ordem do documento.
    pub fn find_redactions(&self, query: &RedactQuery) -> Result<Vec<RedactHit>> {
        let mut hits = Vec::new();
        match query {
            RedactQuery::Phrase(phrase) if phrase.trim().is_empty() => {}
            RedactQuery::Phrase(phrase) => {
                // pelo mesmo caminho dos padrões: sem o limite de achados por página da busca
                let re = Regex::new(&format!("(?i){}", regex::escape(phrase.trim())))
                    .map_err(|e| crate::error::refused(e.to_string()))?;
                for page in 0..self.page_count()? {
                    hits.extend(self.find_patterns(page, &[(None, re.clone())])?);
                }
            }
            RedactQuery::Patterns(patterns) => {
                // compiladas uma vez para o documento todo
                let regexes: Vec<(Option<RedactPattern>, Regex)> = patterns
                    .iter()
                    .map(|&p| (Some(p), Regex::new(p.regex()).expect("padrão fixo válido")))
                    .collect();
                for page in 0..self.page_count()? {
                    hits.extend(self.find_patterns(page, &regexes)?);
                }
            }
        }
        Ok(hits)
    }

    /// Padrões numa página: cada linha do texto estruturado, com a caixa de cada letra.
    fn find_patterns(
        &self,
        page: i32,
        regexes: &[(Option<RedactPattern>, Regex)],
    ) -> Result<Vec<RedactHit>> {
        let tp = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        let mut hits = Vec::new();
        for block in tp.blocks() {
            for line in block.lines() {
                // o texto da linha e, para cada letra, onde ela começa no texto e a caixa dela
                let mut text = String::new();
                let mut boxes: Vec<(usize, Rect)> = Vec::new();
                for ch in line.chars() {
                    let Some(c) = ch.char() else { continue };
                    boxes.push((text.len(), Rect::from(ch.quad())));
                    text.push(c);
                }
                for (pattern, re) in regexes {
                    for m in re.find_iter(&text) {
                        if !pattern.is_none_or(|p| p.accepts(m.as_str())) {
                            continue;
                        }
                        let rect = boxes
                            .iter()
                            .filter(|(at, _)| *at >= m.start() && *at < m.end())
                            .map(|(_, r)| *r)
                            .reduce(|a, b| a.union(&b));
                        if let Some(r) = rect {
                            hits.push(RedactHit {
                                page,
                                rects: vec![r.into()],
                                text: m.as_str().to_owned(),
                            });
                        }
                    }
                }
            }
        }
        Ok(hits)
    }

    /// Aplica todas as marcas de redação (as do app e as de outros programas): o que está sob
    /// elas sai do arquivo e a caixa é pintada. Devolve quantas marcas foram aplicadas.
    pub fn apply_redactions(&mut self, look: &RedactLook) -> Result<usize> {
        self.require_modifiable("aplicar a redação")?;
        let overlay = Overlay::new(look)?;
        self.with_snapshot(|d| {
            // marcas desfeitas ou apagadas só estão escondidas até salvar: saem antes, para o
            // MuPDF (que aplica toda /Redact da página) não aplicá-las também
            d.purge_hidden()?;
            d.history.hidden.clear();
            let mut applied = 0;
            let mut removed_fields = Vec::new();
            let mut redacted_pages = Vec::new();
            for page in 0..d.page_count()? {
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                let mut boxes = Vec::new();
                for annot in pdf_page.annotations() {
                    if annot.r#type()? != PdfAnnotationType::Redact {
                        continue;
                    }
                    applied += 1;
                    let quads = annot.quad_points()?;
                    if quads.is_empty() {
                        boxes.push(annot.rect()?);
                    } else {
                        boxes.extend(quads.into_iter().map(Rect::from));
                    }
                }
                if boxes.is_empty() {
                    continue;
                }
                removed_fields.extend(remove_covered_annots(&mut pdf_page, &boxes)?);
                // a miniatura e os dados privados de outros programas guardam a página original
                let mut page_obj = pdf_page.object();
                page_obj.dict_delete("Thumb")?;
                page_obj.dict_delete("PieceInfo")?;
                pdf_page.apply_redactions_with_options(PdfRedactOptions {
                    black_boxes: false,
                    image_method: PdfRedactImageMethod::Pixels,
                    line_art: PdfRedactLineArtMethod::RemoveIfCovered,
                    text: PdfRedactTextMethod::Remove,
                })?;
                drop(pdf_page);
                d.strip_alternate_text(page)?;
                redacted_pages.push(d.inner.find_page(page)?.as_indirect()?);
                d.paint_boxes(page, &boxes, look, overlay.as_ref())?;
            }
            if !removed_fields.is_empty() {
                prune_fields(&d.inner, &removed_fields)?;
            }
            clean_structure(&d.inner, &redacted_pages)?;
            Ok(applied)
        })
    }

    /// Tira o texto alternativo (/ActualText, /Alt, /E) do conteúdo da página redigida: o marcado
    /// continua no fluxo depois que a redação apaga os glifos, e leitores o usam para copiar e
    /// extrair. O conteúdo de dentro fica; só o par BDC…EMC com esse texto sai.
    fn strip_alternate_text(&mut self, page: i32) -> Result<()> {
        let mut page_obj = self.inner.find_page(page)?;
        let content = page_content(&page_obj)?;
        let cuts = alternate_text_marks(&content);
        if !cuts.is_empty() {
            let stream =
                self.inner
                    .add_stream(&Buffer::from_bytes(&cut(&content, &cuts))?, None, false)?;
            page_obj.dict_put("Contents", stream)?;
        }
        let resources = page_obj.get_dict_inheritable("Resources")?;
        // listas de propriedades nomeadas (/Span /P0 BDC): cópia própria, sem o texto alternativo
        let named = resources
            .as_ref()
            .and_then(|r| r.get_dict("Properties").ok().flatten());
        if let Some(props) = named {
            let entries: Vec<(String, PdfObject)> = props
                .dict_iter()?
                .filter_map(|e| e.ok())
                .filter(|(_, v)| has_alternate_text(v))
                .filter_map(|(k, v)| Some((String::from_utf8(k.as_name().ok()?).ok()?, v)))
                .collect();
            if !entries.is_empty() {
                let mut own = own_resource(&self.inner, &mut page_obj, "Properties")?;
                for (name, v) in entries {
                    let mut clean = v.copy_dict()?;
                    for key in ALT_KEYS {
                        clean.dict_delete(key)?;
                    }
                    own.dict_put(name.as_str(), clean)?;
                }
            }
        }
        // formulários desenhados pela página, também os de dentro de outros (a redação apaga os
        // glifos deles, não o marcado nem as propriedades nomeadas)
        let forms = resources
            .as_ref()
            .and_then(|r| r.get_dict("XObject").ok().flatten());
        if let Some(xobjects) = forms {
            clean_forms(&xobjects, &mut std::collections::HashSet::new(), 0)?;
        }
        Ok(())
    }

    /// Pinta as caixas aplicadas (tela) com a cor e o texto sobreposto, por cima do conteúdo.
    fn paint_boxes(
        &mut self,
        page: i32,
        boxes: &[Rect],
        look: &RedactLook,
        overlay: Option<&Overlay>,
    ) -> Result<()> {
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        let mut page_obj = pdf_page.object();
        let to_screen = page_obj.page_ctm()?;
        let mut ops = format!(
            " q {} cm {} ",
            matrix_ops(&invert(&to_screen)?),
            rgb_ops(look.fill)
        );
        for b in boxes {
            ops.push_str(&format!(
                "{} {} {} {} re ",
                num(b.x0),
                num(b.y0),
                num(b.x1 - b.x0),
                num(b.y1 - b.y0)
            ));
        }
        ops.push_str("f ");
        if let Some(o) = overlay {
            let name = self.font_resource(&mut page_obj, &o.font)?;
            ops.push_str(&format!("BT {} ", rgb_ops(look.overlay_color)));
            for b in boxes {
                let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
                // cabe na caixa: até 12 pt, no máximo 75% da altura e 95% da largura
                let size = 12f32.min(0.75 * h).min(0.95 * w / o.unit);
                if size < 3.0 {
                    continue;
                }
                let x = b.x0 + (w - o.unit * size) / 2.0;
                let y = b.y0 + h / 2.0 + 0.35 * size;
                ops.push_str(&format!(
                    "/{name} {} Tf 1 0 0 -1 {} {} Tm {} ",
                    num(size),
                    num(x),
                    num(y),
                    o.run
                ));
            }
            ops.push_str("ET ");
        }
        ops.push_str("Q ");
        pdf_page.wrap_contents(&mut self.inner)?;
        pdf_page.insert_contents(&mut self.inner, ops.as_bytes(), true)?;
        Ok(())
    }

    /// Limpar documento (Remover informações ocultas do Acrobat).
    pub fn sanitize(&mut self, opts: &SanitizeOptions) -> Result<()> {
        self.require_modifiable("limpar o documento")?;
        self.with_snapshot(|d| {
            let mut trailer = d.inner.trailer()?;
            let mut catalog = d.inner.catalog()?;
            if opts.metadata {
                trailer.dict_delete("Info")?;
                catalog.dict_delete("Metadata")?;
            }
            let mut names = catalog.get_dict("Names")?;
            if opts.attachments {
                catalog.dict_delete("AF")?;
                if let Some(n) = names.as_mut() {
                    n.dict_delete("EmbeddedFiles")?;
                }
            }
            if opts.scripts {
                catalog.dict_delete("OpenAction")?;
                catalog.dict_delete("AA")?;
                if let Some(n) = names.as_mut() {
                    n.dict_delete("JavaScript")?;
                }
            }
            for page in 0..d.page_count()? {
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                if opts.metadata {
                    // metadados, dados privados de outros programas e a miniatura da página
                    let mut obj = pdf_page.object();
                    for key in ["Metadata", "PieceInfo", "Thumb"] {
                        obj.dict_delete(key)?;
                    }
                }
                if opts.scripts {
                    pdf_page.object().dict_delete("AA")?;
                }
                let mut doomed = Vec::new();
                for annot in pdf_page.annotations() {
                    let kind = annot.r#type()?;
                    let comment = !matches!(
                        kind,
                        PdfAnnotationType::Widget
                            | PdfAnnotationType::Link
                            | PdfAnnotationType::Popup
                    );
                    if (opts.comments && comment)
                        || (opts.attachments && kind == PdfAnnotationType::FileAttachment)
                    {
                        doomed.push(annot);
                        continue;
                    }
                    if opts.scripts {
                        let mut obj = annot.object();
                        obj.dict_delete("AA")?;
                        let js = obj
                            .get_dict("A")?
                            .and_then(|a| a.get_dict("S").ok().flatten())
                            .and_then(|s| s.as_name().ok())
                            .is_some_and(|s| s == b"JavaScript");
                        if js {
                            obj.dict_delete("A")?;
                        }
                    }
                }
                for annot in doomed {
                    pdf_page.delete_annotation(annot)?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_cards_must_pass_luhn() {
        assert!(RedactPattern::CreditCard.accepts("4111 1111 1111 1111"));
        assert!(!RedactPattern::CreditCard.accepts("1234 5678 9012 3456"));
    }

    #[test]
    fn patterns_are_valid_regexes() {
        for p in RedactPattern::ALL {
            Regex::new(p.regex()).unwrap();
        }
    }
}
