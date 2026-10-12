//! XFDF (os comentários em XML, como o Acrobat exporta): escrever a partir dos dicionários das
//! anotações e ler de volta para dicionários novos. As coordenadas são as do PDF. A aparência não
//! vai no arquivo: o MuPDF a refaz ao importar.
use crate::error::{refused, Result};
use crate::page_content::num;
use crate::pdf_obj::{get, name_of};
use crate::types::AnnotInfo;
use mupdf::pdf::{PdfDocument, PdfObject};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::fmt::Write as _;

/// Maior XFDF aceito.
const MAX_XFDF_BYTES: usize = 10 * 1024 * 1024;
/// Teto de comentários lidos.
const MAX_ITEMS: usize = 5000;
/// Teto de números numa lista (vértices, traços, quadriláteros).
const MAX_NUMBERS: usize = 20_000;
/// Teto do texto de um comentário.
const MAX_TEXT: usize = 100_000;

/// Elemento do XFDF e o /Subtype correspondente.
const KINDS: [(&str, &str); 15] = [
    ("text", "Text"),
    ("highlight", "Highlight"),
    ("underline", "Underline"),
    ("strikeout", "StrikeOut"),
    ("squiggly", "Squiggly"),
    ("freetext", "FreeText"),
    ("square", "Square"),
    ("circle", "Circle"),
    ("line", "Line"),
    ("polygon", "Polygon"),
    ("polyline", "PolyLine"),
    ("ink", "Ink"),
    ("stamp", "Stamp"),
    ("caret", "Caret"),
    ("redact", "Redact"),
];

/// Chaves de texto do dicionário e os atributos do XFDF (os dois sentidos usam a mesma tabela).
const TEXT_ATTRS: [(&str, &str); 4] = [
    ("T", "title"),
    ("M", "date"),
    ("CreationDate", "creationdate"),
    ("Subj", "subject"),
];

/// Bits de /F e os nomes do XFDF.
const FLAGS: [(i32, &str); 10] = [
    (1, "invisible"),
    (2, "hidden"),
    (4, "print"),
    (8, "nozoom"),
    (16, "norotate"),
    (32, "noview"),
    (64, "readonly"),
    (128, "locked"),
    (256, "togglenoview"),
    (512, "lockedcontents"),
];

/// Comentário lido do XFDF.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct XfdfAnnot {
    pub subtype: &'static str,
    pub page: i32,
    pub attrs: Vec<(String, String)>,
    pub contents: Option<String>,
    pub gestures: Vec<String>,
    /// /DA da caixa de texto (fonte, tamanho e cor).
    pub da: Option<String>,
}

impl XfdfAnnot {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Bits de /F a partir das palavras do XFDF (`print,nozoom`).
pub(crate) fn flag_bits(words: &str) -> i32 {
    words
        .split([',', ' '])
        .filter_map(|w| FLAGS.iter().find(|(_, n)| n.eq_ignore_ascii_case(w.trim())))
        .fold(0, |f, (bit, _)| f | bit)
}

/// Nome PDF simples (ícone, intenção, ponta de linha): só letras e números, até 64.
pub(crate) fn plain_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// O conteúdo parece XFDF (XML)?
pub(crate) fn looks_like_xfdf(bytes: &[u8]) -> bool {
    let start = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = start.trim_ascii_start();
    start.starts_with(b"<?xml") || start.starts_with(b"<xfdf")
}

fn escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Cor PDF (0..1) como `#RRGGBB`.
pub(crate) fn hex_color(c: &[f32]) -> Option<String> {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match c {
        [r, g, b] => Some(format!("#{:02X}{:02X}{:02X}", to(*r), to(*g), to(*b))),
        [k] => Some(format!("#{0:02X}{0:02X}{0:02X}", to(*k))),
        _ => None,
    }
}

/// `#RRGGBB` como cor PDF.
pub(crate) fn parse_color(s: &str) -> Option<[f32; 3]> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let part = |k: usize| {
        u8::from_str_radix(&hex[k..k + 2], 16)
            .ok()
            .map(|v| v as f32 / 255.0)
    };
    Some([part(0)?, part(2)?, part(4)?])
}

/// Números separados por vírgula, ponto e vírgula ou espaço (só os finitos; `None` se algum não
/// for número), até o teto.
pub(crate) fn numbers(s: &str) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    for part in s
        .split([',', ';', ' ', '\n', '\t', '\r'])
        .filter(|p| !p.is_empty())
    {
        let v: f32 = part.parse().ok()?;
        if !v.is_finite() || out.len() >= MAX_NUMBERS {
            return None;
        }
        out.push(v);
    }
    Some(out)
}

fn floats(obj: &PdfObject) -> Vec<f32> {
    (0..obj.len().unwrap_or(0).min(MAX_NUMBERS))
        .filter_map(|k| obj.get_array(k as i32).ok().flatten()?.as_float().ok())
        .collect()
}

fn joined(values: &[f32], sep: &str) -> String {
    values.iter().map(|&v| num(v)).collect::<Vec<_>>().join(sep)
}

/// Pares `x,y;x,y` (vértices e traços).
fn pairs(values: &[f32]) -> String {
    values
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[x, y]| format!("{},{}", num(*x), num(*y)))
        .collect::<Vec<_>>()
        .join(";")
}

fn text_of(dict: &PdfObject, key: &str) -> Option<String> {
    get(dict, key).and_then(|v| v.as_string().ok())
}

/// Nome do comentário no arquivo: o /NM, ou um feito da página e do id.
fn name_for(dict: &PdfObject, a: &AnnotInfo) -> String {
    text_of(dict, "NM")
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("gera-{}-{}", a.page, a.id))
}

/// Escreve o XFDF dos comentários `picked` (na ordem dada).
pub(crate) fn write(doc: &PdfDocument, picked: &[AnnotInfo], file_name: &str) -> Result<String> {
    // os dicionários de cada página numa passada só
    let mut found: HashMap<(i32, i32), PdfObject> = HashMap::new();
    let pages: std::collections::BTreeSet<i32> = picked.iter().map(|a| a.page).collect();
    for page in pages {
        for annot in doc.load_pdf_page(page)?.annotations() {
            if let (Ok(id), Some(dict)) = (annot.object().as_indirect(), annot.object().resolve()?)
            {
                found.insert((page, id), dict);
            }
        }
    }
    let mut names = HashMap::new();
    let mut dicts = Vec::new();
    for a in picked {
        if let Some(dict) = found.remove(&(a.page, a.id)) {
            names.insert(a.id, name_for(&dict, a));
            dicts.push((a, dict));
        }
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n<annots>\n",
    );
    for (a, dict) in &dicts {
        let Some(sub) = get(dict, "Subtype").and_then(|s| name_of(&s)) else {
            continue;
        };
        let Some((tag, _)) = KINDS.iter().find(|(_, s)| *s == sub) else {
            continue;
        };
        let mut attrs: Vec<(&str, String)> =
            vec![("page", a.page.to_string()), ("name", names[&a.id].clone())];
        if let Some(r) = get(dict, "Rect") {
            attrs.push(("rect", joined(&floats(&r), ",")));
        }
        if let Some(c) = get(dict, "C").and_then(|c| hex_color(&floats(&c))) {
            attrs.push(("color", c));
        }
        if let Some(c) = get(dict, "IC").and_then(|c| hex_color(&floats(&c))) {
            attrs.push(("interior-color", c));
        }
        if let Some(o) = get(dict, "CA").and_then(|o| o.as_float().ok()) {
            attrs.push(("opacity", num(o)));
        }
        for (key, attr) in TEXT_ATTRS {
            if let Some(v) = text_of(dict, key) {
                attrs.push((attr, v));
            }
        }
        if let Some(f) = get(dict, "F").and_then(|f| f.as_int().ok()) {
            let words: Vec<&str> = FLAGS
                .iter()
                .filter(|(bit, _)| f & bit != 0)
                .map(|(_, w)| *w)
                .collect();
            if !words.is_empty() {
                attrs.push(("flags", words.join(",")));
            }
        }
        if let Some(parent) = a.reply_to.or(a.group).and_then(|p| names.get(&p)) {
            attrs.push(("inreplyto", parent.clone()));
            let group = get(dict, "RT").and_then(|r| name_of(&r)).as_deref() == Some("Group");
            attrs.push(("replyType", if group { "group" } else { "reply" }.into()));
        }
        if let Some(n) = get(dict, "Name").and_then(|n| name_of(&n)) {
            attrs.push(("icon", n));
        }
        if let Some(w) = get(dict, "BS")
            .and_then(|b| get(&b, "W"))
            .and_then(|w| w.as_float().ok())
        {
            attrs.push(("width", num(w)));
        }
        if let Some(q) = get(dict, "QuadPoints") {
            attrs.push(("coords", joined(&floats(&q), ",")));
        }
        if let Some(l) = get(dict, "L").map(|l| floats(&l)).filter(|l| l.len() == 4) {
            attrs.push(("start", format!("{},{}", num(l[0]), num(l[1]))));
            attrs.push(("end", format!("{},{}", num(l[2]), num(l[3]))));
        }
        if let Some(le) = get(dict, "LE") {
            let ends: Vec<String> = (0..2)
                .filter_map(|k| le.get_array(k).ok().flatten().and_then(|n| name_of(&n)))
                .collect();
            if let [head, tail] = ends.as_slice() {
                attrs.push(("head", head.clone()));
                attrs.push(("tail", tail.clone()));
            }
        }
        if let Some(it) = get(dict, "IT").and_then(|i| name_of(&i)) {
            attrs.push(("intent", it));
        }
        if let Some(cl) = get(dict, "CL") {
            attrs.push(("callout", joined(&floats(&cl), ",")));
        }
        if let Some(v) = get(dict, "Vertices") {
            attrs.push(("vertices", pairs(&floats(&v))));
        }
        let _ = write!(out, "<{tag}");
        for (k, v) in &attrs {
            let _ = write!(out, " {k}=\"{}\"", escape(v));
        }
        out.push('>');
        if let Some(text) = text_of(dict, "Contents") {
            let _ = write!(out, "<contents>{}</contents>", escape(&text));
        }
        if let Some(da) = text_of(dict, "DA") {
            let _ = write!(
                out,
                "<defaultappearance>{}</defaultappearance>",
                escape(&da)
            );
        }
        if let Some(ink) = get(dict, "InkList") {
            out.push_str("<inklist>");
            for k in 0..ink.len()? {
                if let Some(stroke) = ink.get_array(k as i32)? {
                    let _ = write!(out, "<gesture>{}</gesture>", pairs(&floats(&stroke)));
                }
            }
            out.push_str("</inklist>");
        }
        let _ = writeln!(out, "</{tag}>");
    }
    let _ = write!(
        out,
        "</annots>\n<f href=\"{}\"/>\n</xfdf>\n",
        escape(file_name)
    );
    Ok(out)
}

/// Dicionário da anotação a partir do elemento do XFDF (`rect` já conferido).
pub(crate) fn annot_dict(doc: &PdfDocument, item: &XfdfAnnot, rect: &[f32]) -> Result<PdfObject> {
    let mut d = doc.new_dict()?;
    let reals = |v: &[f32]| -> Result<PdfObject> {
        let mut a = doc.new_array()?;
        for &x in v {
            a.array_push(PdfObject::new_real(x)?)?;
        }
        Ok(a)
    };
    let name = |n: &str| PdfObject::new_name(n);
    d.dict_put("Type", name("Annot")?)?;
    d.dict_put("Subtype", name(item.subtype)?)?;
    d.dict_put("Rect", reals(rect)?)?;
    for (attr, key) in [("color", "C"), ("interior-color", "IC")] {
        if let Some(c) = item.attr(attr).and_then(parse_color) {
            d.dict_put(key, reals(&c)?)?;
        }
    }
    if let Some(o) = item
        .attr("opacity")
        .and_then(|o| o.trim().parse::<f32>().ok())
    {
        if o.is_finite() {
            d.dict_put("CA", PdfObject::new_real(o.clamp(0.0, 1.0))?)?;
        }
    }
    for &(key, attr) in TEXT_ATTRS.iter().chain(&[("NM", "name")]) {
        if let Some(v) = item.attr(attr) {
            d.dict_put(key, PdfObject::new_string(v)?)?;
        }
    }
    if let Some(words) = item.attr("flags") {
        let f = flag_bits(words);
        if f != 0 {
            d.dict_put("F", PdfObject::new_int(f)?)?;
        }
    }
    if item.attr("replyType") == Some("group") {
        d.dict_put("RT", name("Group")?)?;
    }
    for (attr, key) in [("icon", "Name"), ("intent", "IT")] {
        if let Some(v) = item.attr(attr).filter(|v| plain_name(v)) {
            d.dict_put(key, name(v)?)?;
        }
    }
    if let Some(w) = item
        .attr("width")
        .and_then(|w| w.trim().parse::<f32>().ok())
    {
        if w.is_finite() {
            let mut bs = doc.new_dict()?;
            bs.dict_put("W", PdfObject::new_real(w.clamp(0.0, 100.0))?)?;
            d.dict_put("BS", bs)?;
        }
    }
    let list = |attr: &str, step: usize| {
        item.attr(attr)
            .and_then(numbers)
            .filter(|v| !v.is_empty() && v.len() % step == 0)
    };
    if let Some(q) = list("coords", 8) {
        d.dict_put("QuadPoints", reals(&q)?)?;
    }
    if let Some(v) = list("vertices", 2) {
        d.dict_put("Vertices", reals(&v)?)?;
    }
    if let Some(cl) = list("callout", 2).filter(|c| c.len() <= 6) {
        d.dict_put("CL", reals(&cl)?)?;
    }
    if let (Some(a), Some(b)) = (list("start", 2), list("end", 2)) {
        if a.len() == 2 && b.len() == 2 {
            d.dict_put("L", reals(&[a[0], a[1], b[0], b[1]])?)?;
        }
    }
    if let (Some(h), Some(t)) = (item.attr("head"), item.attr("tail")) {
        if plain_name(h) && plain_name(t) {
            let mut le = doc.new_array()?;
            le.array_push(name(h)?)?;
            le.array_push(name(t)?)?;
            d.dict_put("LE", le)?;
        }
    }
    if !item.gestures.is_empty() {
        let mut ink = doc.new_array()?;
        for g in &item.gestures {
            if let Some(v) = numbers(g).filter(|v| v.len() >= 2 && v.len() % 2 == 0) {
                ink.array_push(reals(&v)?)?;
            }
        }
        d.dict_put("InkList", ink)?;
    }
    if let Some(text) = &item.contents {
        d.dict_put("Contents", PdfObject::new_string(text)?)?;
    }
    if let Some(da) = &item.da {
        d.dict_put("DA", PdfObject::new_string(da)?)?;
    }
    Ok(d)
}

/// O que se está lendo dentro de um comentário.
#[derive(Clone, Copy, PartialEq)]
enum Inside {
    Nothing,
    Contents,
    Gesture,
    Da,
}

fn attrs_of(e: &BytesStart) -> Vec<(String, String)> {
    e.attributes()
        .filter_map(|a| a.ok())
        .filter_map(|a| {
            let key = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            Some((key, value))
        })
        .collect()
}

fn tag_of(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).to_ascii_lowercase()
}

/// Comentário que começa em `e` (dentro de <annots>): `None` se o tipo ou a página não servem.
fn begin(e: &BytesStart) -> Option<XfdfAnnot> {
    let tag = tag_of(e);
    let subtype = KINDS.iter().find(|(t, _)| *t == tag).map(|(_, s)| *s)?;
    let attrs = attrs_of(e);
    let page = attrs
        .iter()
        .find(|(k, _)| k == "page")
        .and_then(|(_, v)| v.trim().parse::<i32>().ok())
        .filter(|p| *p >= 0)?;
    Some(XfdfAnnot {
        subtype,
        page,
        attrs,
        ..Default::default()
    })
}

/// Entidades do XML que valem dentro do texto (as declaradas no DOCTYPE não são expandidas).
pub(crate) fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// Lê os comentários do XFDF. Devolve também quantos foram ignorados (tipo desconhecido, página
/// inválida ou acima do teto). XML malformado é erro.
pub(crate) fn read(bytes: &[u8]) -> Result<(Vec<XfdfAnnot>, usize)> {
    if bytes.len() > MAX_XFDF_BYTES {
        return Err(refused("o XFDF é grande demais"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| refused("o XFDF não é UTF-8"))?;
    let bad = || refused("o XFDF está malformado");
    let mut reader = Reader::from_str(text);
    let mut items: Vec<XfdfAnnot> = Vec::new();
    let mut skipped = 0;
    let mut in_annots = false;
    // comentário aberto e a profundidade dentro dele
    let mut current: Option<(XfdfAnnot, usize)> = None;
    let mut inside = Inside::Nothing;
    let mut buf = String::new();
    let accept = |items: &mut Vec<XfdfAnnot>, skipped: &mut usize, a: Option<XfdfAnnot>| match a {
        Some(a) if items.len() < MAX_ITEMS => items.push(a),
        _ => *skipped += 1,
    };
    loop {
        match reader.read_event().map_err(|_| bad())? {
            Event::Eof => break,
            Event::Start(e) => match current.as_mut() {
                Some((_, depth)) => {
                    *depth += 1;
                    inside = match tag_of(&e).as_str() {
                        "contents" => Inside::Contents,
                        "gesture" => Inside::Gesture,
                        "defaultappearance" => Inside::Da,
                        _ => Inside::Nothing,
                    };
                    buf.clear();
                }
                None if in_annots => match begin(&e) {
                    Some(a) => current = Some((a, 0)),
                    None => {
                        skipped += 1;
                        reader.read_to_end(e.name()).map_err(|_| bad())?;
                    }
                },
                None => in_annots |= tag_of(&e) == "annots",
            },
            Event::Empty(e) if current.is_none() && in_annots => {
                accept(&mut items, &mut skipped, begin(&e));
            }
            Event::End(e) => match current.as_mut() {
                Some((_, 0)) => {
                    if let Some((a, _)) = current.take() {
                        accept(&mut items, &mut skipped, Some(a));
                    }
                }
                Some((a, depth)) => {
                    *depth -= 1;
                    match inside {
                        Inside::Contents => a.contents = Some(std::mem::take(&mut buf)),
                        Inside::Da => a.da = Some(std::mem::take(&mut buf)),
                        Inside::Gesture if a.gestures.len() < MAX_NUMBERS => {
                            a.gestures.push(std::mem::take(&mut buf))
                        }
                        _ => {}
                    }
                    inside = Inside::Nothing;
                }
                None => {
                    if e.local_name().as_ref().eq_ignore_ascii_case(b"annots") {
                        in_annots = false;
                    }
                }
            },
            Event::Text(t) if inside != Inside::Nothing => {
                push_capped(&mut buf, &t.decode().map_err(|_| bad())?);
            }
            Event::CData(t) if inside != Inside::Nothing => {
                push_capped(&mut buf, &String::from_utf8_lossy(&t));
            }
            Event::GeneralRef(r) if inside != Inside::Nothing => {
                let c = match r.resolve_char_ref().map_err(|_| bad())? {
                    Some(c) => Some(c),
                    None => entity(&r.decode().map_err(|_| bad())?),
                };
                if let Some(c) = c {
                    push_capped(&mut buf, c.encode_utf8(&mut [0; 4]));
                }
            }
            _ => {}
        }
    }
    if current.is_some() {
        return Err(bad());
    }
    Ok((items, skipped))
}

fn push_capped(buf: &mut String, s: &str) {
    if buf.len() + s.len() <= MAX_TEXT {
        buf.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_go_both_ways() {
        assert_eq!(hex_color(&[1.0, 0.5, 0.0]).as_deref(), Some("#FF8000"));
        assert_eq!(parse_color("#FF8000"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(parse_color("FF8000"), None);
        assert_eq!(parse_color("#GG0000"), None);
        assert_eq!(parse_color("#ÁÁ00"), None);
    }

    #[test]
    fn number_lists_refuse_nan_and_garbage() {
        assert_eq!(numbers("1,2;3 4"), Some(vec![1.0, 2.0, 3.0, 4.0]));
        assert_eq!(numbers("1,NaN"), None);
        assert_eq!(numbers("1,inf"), None);
        assert_eq!(numbers("1,x"), None);
    }

    #[test]
    fn reads_contents_gestures_and_entities() {
        let xml = r#"<?xml version="1.0"?><xfdf><annots>
            <ink page="1" rect="0,0,10,10"><inklist><gesture>1,2;3,4</gesture></inklist></ink>
            <text page="0" rect="0,0,1,1"><contents>a &amp; b &#233;</contents></text>
            <unknown page="0"><contents>x</contents></unknown>
            </annots><fields><field name="f"><value>v</value></field></fields></xfdf>"#;
        let (items, skipped) = read(xml.as_bytes()).unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(items[0].gestures, ["1,2;3,4"]);
        assert_eq!(items[1].contents.as_deref(), Some("a & b é"));
    }

    #[test]
    fn truncated_xml_is_an_error() {
        assert!(read(b"<xfdf><annots><text page=\"0\" rect=\"0,0,1,1\"><contents>x").is_err());
    }
}
