//! Propriedades do documento editáveis, como no Acrobat: descrição e propriedades personalizadas
//! (`/Info` e XMP), visualização inicial e opções de janela, idioma e preferências de impressão
//! (`/ViewerPreferences`); e a lista das fontes usadas.
use crate::doc::{pdf_now, PdfDoc};
use crate::error::{refused, Result};
use crate::pdf_obj::{get, get_bool, name_of};
use crate::types::{
    Duplex, FontInfo, InitialLayout, InitialPanel, InitialView, InitialZoom, PrintPrefs,
    PropertiesEdit, WindowOptions,
};
use mupdf::pdf::{PdfDocument, PdfObject};
use std::collections::HashSet;

/// Chaves do `/Info` que não são propriedades personalizadas.
const STANDARD_INFO: [&str; 9] = [
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
    "Trapped",
];
/// Nome de propriedade personalizada: até isso (limite de nome do PDF).
const MAX_NAME_BYTES: usize = 127;
/// Teto de fontes listadas e de níveis de XObject percorridos.
const MAX_FONTS: usize = 10_000;
/// Maior XMP que o app lê.
const MAX_XMP_BYTES: usize = 4 * 1024 * 1024;
const MAX_XOBJECT_DEPTH: u32 = 10;

/// Nome aceito numa propriedade personalizada: não vazio, curto, fora dos nomes padrão do `/Info`
/// e sem espaço, controle ou delimitador do PDF.
pub fn valid_custom_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !STANDARD_INFO.contains(&name)
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "/()<>[]{}%#".contains(c))
}

impl PdfDoc {
    /// Propriedades personalizadas do `/Info` (nome, valor), na ordem do arquivo.
    pub(crate) fn custom_properties(&self) -> Vec<(String, String)> {
        let Some(info) = self.info_dict() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for i in 0..info.dict_len().unwrap_or(0) as i32 {
            let (Ok(Some(k)), Ok(Some(v))) = (info.get_dict_key(i), info.get_dict_val(i)) else {
                continue;
            };
            let Some(name) = name_of(&k).filter(|n| !STANDARD_INFO.contains(&n.as_str())) else {
                continue;
            };
            if v.is_string().unwrap_or(false) {
                if let Ok(text) = v.as_string() {
                    out.push((name, text));
                }
            }
        }
        out
    }

    pub(crate) fn info_dict(&self) -> Option<PdfObject> {
        self.inner.trailer().ok().and_then(|t| get(&t, "Info"))
    }

    fn viewer_prefs(&self) -> Option<PdfObject> {
        self.inner
            .catalog()
            .ok()
            .and_then(|c| get(&c, "ViewerPreferences"))
    }

    pub(crate) fn window_options(&self) -> WindowOptions {
        let Some(vp) = self.viewer_prefs() else {
            return WindowOptions::default();
        };
        WindowOptions {
            fit_window: get_bool(&vp, "FitWindow"),
            center_window: get_bool(&vp, "CenterWindow"),
            display_title: get_bool(&vp, "DisplayDocTitle"),
            hide_menubar: get_bool(&vp, "HideMenubar"),
            hide_toolbar: get_bool(&vp, "HideToolbar"),
            hide_window_ui: get_bool(&vp, "HideWindowUI"),
        }
    }

    /// O título, quando o arquivo pede para mostrá-lo no lugar do nome (`DisplayDocTitle`).
    pub(crate) fn display_title(&self) -> Option<String> {
        if !self.window_options().display_title {
            return None;
        }
        let title = self.inner.metadata(mupdf::MetadataName::Title).ok()?;
        (!title.trim().is_empty()).then(|| title.trim().to_owned())
    }

    pub(crate) fn print_prefs(&self) -> PrintPrefs {
        let Some(vp) = self.viewer_prefs() else {
            return PrintPrefs::default();
        };
        let ranges = get(&vp, "PrintPageRange")
            .filter(|a| a.is_array().unwrap_or(false))
            .map(|a| {
                let n = a.len().unwrap_or(0).min(512) as i32;
                let num = |i| {
                    a.get_array(i)
                        .ok()
                        .flatten()
                        .and_then(|v| v.as_int().ok())
                        .and_then(|v| u32::try_from(v).ok())
                };
                (0..n / 2)
                    .filter_map(|k| Some((num(2 * k)? + 1, num(2 * k + 1)? + 1)))
                    .collect()
            })
            .unwrap_or_default();
        PrintPrefs {
            scaling_none: get(&vp, "PrintScaling")
                .and_then(|v| name_of(&v))
                .as_deref()
                == Some("None"),
            duplex: match get(&vp, "Duplex").and_then(|v| name_of(&v)).as_deref() {
                Some("Simplex") => Some(Duplex::Simplex),
                Some("DuplexFlipShortEdge") => Some(Duplex::ShortEdge),
                Some("DuplexFlipLongEdge") => Some(Duplex::LongEdge),
                _ => None,
            },
            copies: get(&vp, "NumCopies")
                .and_then(|v| v.as_int().ok())
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| (1..=999).contains(v)),
            ranges,
        }
    }

    pub(crate) fn language(&self) -> String {
        self.inner
            .catalog()
            .ok()
            .and_then(|c| get(&c, "Lang"))
            .and_then(|l| l.as_string().ok())
            .unwrap_or_default()
    }

    /// O pacote XMP (`/Metadata` do catálogo), se houver.
    pub fn xmp(&self) -> Option<String> {
        let meta = get(&self.inner.catalog().ok()?, "Metadata")?;
        // com teto: o XMP vem do arquivo e pode ser uma bomba de compressão
        let bytes = crate::attachments::read_capped(&meta, MAX_XMP_BYTES).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Fontes usadas nas páginas (e nos XObjects de formulário), sem repetir.
    pub(crate) fn fonts(&self) -> Vec<FontInfo> {
        let mut out = Vec::new();
        let mut seen_fonts = HashSet::new();
        let mut seen_xobjects = HashSet::new();
        for p in 0..self.page_count().unwrap_or(0) {
            let Ok(page) = self.inner.load_pdf_page(p) else {
                continue;
            };
            if let Some(res) = get(&page.object(), "Resources") {
                collect_fonts(&res, 0, &mut out, &mut seen_fonts, &mut seen_xobjects);
            }
            if out.len() >= MAX_FONTS {
                break;
            }
        }
        out
    }

    /// Grava as propriedades (uma etapa do desfazer).
    pub fn set_properties(&mut self, e: &PropertiesEdit) -> Result<()> {
        for (name, _) in &e.custom {
            if !valid_custom_name(name) {
                return Err(refused(format!("nome de propriedade inválido: {name:?}")));
            }
        }
        self.require_modifiable("alterar as propriedades")?;
        self.with_snapshot(|d| d.write_properties(e))
    }

    fn write_properties(&mut self, e: &PropertiesEdit) -> Result<()> {
        let mut info = match self.info_dict() {
            Some(i) => i,
            None => {
                let i = self.inner.new_dict()?;
                let i = self.inner.add_object(&i)?;
                self.inner.trailer()?.dict_put("Info", i.clone())?;
                i
            }
        };
        for (key, value) in [
            ("Title", &e.title),
            ("Author", &e.author),
            ("Subject", &e.subject),
            ("Keywords", &e.keywords),
        ] {
            put_text(&mut info, key, value)?;
        }
        // personalizadas: as antigas saem, as novas entram
        for (name, _) in self.custom_properties() {
            info.dict_delete(name.as_str())?;
        }
        for (name, value) in &e.custom {
            put_text(&mut info, name, value)?;
        }
        let now = pdf_now();
        info.dict_put("ModDate", PdfObject::new_string(&now)?)?;
        // a visualização inicial só é regravada quando muda: o que o app não representa (ação
        // JavaScript, /UseOC, destino FitV…) continua como estava
        let unchanged = self.initial_view() == e.initial;
        let mut catalog = self.inner.catalog()?;
        if !unchanged {
            write_initial_view(&self.inner, &mut catalog, &e.initial)?;
        }
        put_text(&mut catalog, "Lang", &e.lang)?;
        write_viewer_prefs(&self.inner, &mut catalog, &e.window, &e.print)?;
        let old_xmp = self.xmp();
        let text = |k: &str| crate::pdf_obj::text(&info, k);
        let stamps = XmpStamps {
            producer: text("Producer"),
            creator: text("Creator"),
            created: text("CreationDate"),
            modified: now,
        };
        let xmp = xmp_packet(e, old_xmp.as_deref(), &stamps);
        let mut dict = self.inner.new_dict()?;
        dict.dict_put("Type", PdfObject::new_name("Metadata")?)?;
        dict.dict_put("Subtype", PdfObject::new_name("XML")?)?;
        let buf = mupdf::Buffer::from_bytes(xmp.as_bytes())?;
        let meta = self.inner.add_stream(&buf, Some(&dict), false)?;
        catalog.dict_put("Metadata", meta)?;
        Ok(())
    }
}

/// O que o XMP repete do `/Info` (o PDF/A exige os dois iguais).
struct XmpStamps {
    producer: String,
    creator: String,
    created: String,
    modified: String,
}

/// Texto no dicionário; vazio apaga a chave.
fn put_text(dict: &mut PdfObject, key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        dict.dict_delete(key)?;
    } else {
        dict.dict_put(key, PdfObject::new_string(value)?)?;
    }
    Ok(())
}

fn put_name_or_delete(dict: &mut PdfObject, key: &str, name: Option<&str>) -> Result<()> {
    match name {
        Some(n) => dict.dict_put(key, PdfObject::new_name(n)?)?,
        None => dict.dict_delete(key)?,
    }
    Ok(())
}

fn write_initial_view(doc: &PdfDocument, catalog: &mut PdfObject, v: &InitialView) -> Result<()> {
    let mode = if v.full_screen {
        Some("FullScreen")
    } else {
        v.panel.map(|p| match p {
            InitialPanel::Bookmarks => "UseOutlines",
            InitialPanel::Thumbnails => "UseThumbs",
            InitialPanel::Attachments => "UseAttachments",
        })
    };
    put_name_or_delete(catalog, "PageMode", mode)?;
    let layout = v.layout.map(|l| match l {
        InitialLayout::Single => "SinglePage",
        InitialLayout::OneColumn => "OneColumn",
        InitialLayout::TwoColumn { cover: false } => "TwoColumnLeft",
        InitialLayout::TwoColumn { cover: true } => "TwoColumnRight",
        InitialLayout::TwoPage { cover: false } => "TwoPageLeft",
        InitialLayout::TwoPage { cover: true } => "TwoPageRight",
    });
    put_name_or_delete(catalog, "PageLayout", layout)?;
    if v.page.is_none() && v.zoom.is_none() {
        catalog.dict_delete("OpenAction")?;
        return Ok(());
    }
    let page = v.page.unwrap_or(0).max(0);
    let page_obj = doc.find_page(page)?;
    let mut dest = doc.new_array()?;
    dest.array_push(page_obj)?;
    match v.zoom {
        Some(InitialZoom::FitPage) | None => dest.array_push(PdfObject::new_name("Fit")?)?,
        Some(InitialZoom::FitWidth) => {
            dest.array_push(PdfObject::new_name("FitH")?)?;
            dest.array_push(PdfObject::new_null())?;
        }
        Some(InitialZoom::Factor(z)) => {
            dest.array_push(PdfObject::new_name("XYZ")?)?;
            dest.array_push(PdfObject::new_null())?;
            dest.array_push(PdfObject::new_null())?;
            dest.array_push(PdfObject::new_real(z)?)?;
        }
    }
    catalog.dict_put("OpenAction", dest)?;
    Ok(())
}

fn write_viewer_prefs(
    doc: &PdfDocument,
    catalog: &mut PdfObject,
    w: &WindowOptions,
    p: &PrintPrefs,
) -> Result<()> {
    let mut vp = match get(catalog, "ViewerPreferences") {
        Some(v) if v.is_dict().unwrap_or(false) => v,
        _ => doc.new_dict()?,
    };
    for (key, on) in [
        ("FitWindow", w.fit_window),
        ("CenterWindow", w.center_window),
        ("DisplayDocTitle", w.display_title),
        ("HideMenubar", w.hide_menubar),
        ("HideToolbar", w.hide_toolbar),
        ("HideWindowUI", w.hide_window_ui),
    ] {
        if on {
            vp.dict_put(key, PdfObject::new_bool(true))?;
        } else {
            vp.dict_delete(key)?;
        }
    }
    put_name_or_delete(&mut vp, "PrintScaling", p.scaling_none.then_some("None"))?;
    let duplex = p.duplex.map(|d| match d {
        Duplex::Simplex => "Simplex",
        Duplex::ShortEdge => "DuplexFlipShortEdge",
        Duplex::LongEdge => "DuplexFlipLongEdge",
    });
    put_name_or_delete(&mut vp, "Duplex", duplex)?;
    match p.copies.filter(|c| (1..=999).contains(c)) {
        Some(c) => vp.dict_put("NumCopies", PdfObject::new_int(c as i32)?)?,
        None => vp.dict_delete("NumCopies")?,
    }
    if p.ranges.is_empty() {
        vp.dict_delete("PrintPageRange")?;
    } else {
        let mut arr = doc.new_array()?;
        for &(a, b) in p.ranges.iter().take(256) {
            arr.array_push(PdfObject::new_int(a.saturating_sub(1) as i32)?)?;
            arr.array_push(PdfObject::new_int(b.saturating_sub(1) as i32)?)?;
        }
        vp.dict_put("PrintPageRange", arr)?;
    }
    if vp.dict_len()? == 0 {
        catalog.dict_delete("ViewerPreferences")?;
    } else {
        catalog.dict_put("ViewerPreferences", vp)?;
    }
    Ok(())
}

fn collect_fonts(
    res: &PdfObject,
    depth: u32,
    out: &mut Vec<FontInfo>,
    seen_fonts: &mut HashSet<i32>,
    seen_xobjects: &mut HashSet<i32>,
) {
    if depth > MAX_XOBJECT_DEPTH || out.len() >= MAX_FONTS {
        return;
    }
    if let Some(fonts) = get(res, "Font").filter(|f| f.is_dict().unwrap_or(false)) {
        for i in 0..fonts.dict_len().unwrap_or(0) as i32 {
            let Ok(Some(f)) = fonts.get_dict_val(i) else {
                continue;
            };
            let num = f.as_indirect().unwrap_or(0);
            if num != 0 && !seen_fonts.insert(num) {
                continue;
            }
            if let Some(info) = font_info(&f) {
                if !out.contains(&info) {
                    out.push(info);
                }
            }
            if out.len() >= MAX_FONTS {
                return;
            }
        }
    }
    if let Some(xobjects) = get(res, "XObject").filter(|x| x.is_dict().unwrap_or(false)) {
        for i in 0..xobjects.dict_len().unwrap_or(0) as i32 {
            let Ok(Some(x)) = xobjects.get_dict_val(i) else {
                continue;
            };
            let num = x.as_indirect().unwrap_or(0);
            if num == 0 || !seen_xobjects.insert(num) {
                continue;
            }
            let form = get(&x, "Subtype").and_then(|s| name_of(&s)).as_deref() == Some("Form");
            if let Some(inner) = get(&x, "Resources").filter(|_| form) {
                collect_fonts(&inner, depth + 1, out, seen_fonts, seen_xobjects);
            }
        }
    }
}

fn font_info(font: &PdfObject) -> Option<FontInfo> {
    let kind = get(font, "Subtype").and_then(|s| name_of(&s))?;
    let name = get(font, "BaseFont")
        .and_then(|n| name_of(&n))
        .unwrap_or_else(|| format!("({kind})"));
    // Type0: o descritor está na fonte descendente
    let descriptor_owner = if kind == "Type0" {
        get(font, "DescendantFonts").and_then(|d| d.get_array(0).ok().flatten())
    } else {
        Some(font.clone())
    };
    let embedded = descriptor_owner
        .and_then(|f| get(&f, "FontDescriptor"))
        .is_some_and(|d| {
            ["FontFile", "FontFile2", "FontFile3"]
                .iter()
                .any(|k| get(&d, k).is_some())
        });
    let subset = name.len() > 7
        && name.as_bytes()[6] == b'+'
        && name.as_bytes()[..6].iter().all(u8::is_ascii_uppercase);
    let embedded = embedded || kind == "Type3"; // Type3: desenhada no próprio PDF
    Some(FontInfo {
        name,
        kind,
        embedded,
        subset,
    })
}

/// Texto escapado para XML (sem os caracteres de controle que o XML 1.0 proíbe).
fn xml_escape(s: &str) -> String {
    s.chars()
        .filter(|&c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Valor simples no XMP antigo (ex.: `pdfaid:part`), como elemento (`<pdfaid:part>2</…>`) ou
/// como atributo (`pdfaid:part="2"` ou com aspas simples).
fn xmp_value(xmp: &str, element: &str) -> Option<String> {
    let as_element = || {
        let open = format!("<{element}>");
        let start = xmp.find(&open)? + open.len();
        let end = start + xmp[start..].find(&format!("</{element}>"))?;
        Some(&xmp[start..end])
    };
    let as_attribute = || {
        ['"', '\''].into_iter().find_map(|q| {
            let open = format!("{element}={q}");
            let start = xmp.find(&open)? + open.len();
            Some(&xmp[start..start + xmp[start..].find(q)?])
        })
    };
    let v = as_element().or_else(as_attribute)?.trim();
    (!v.is_empty() && v.len() < 16 && v.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| v.to_owned())
}

/// `D:AAAAMMDDhhmmss…` como data ISO do XMP (`AAAA-MM-DDThh:mm:ss`); vazio se não entender.
fn iso_date(pdf: &str) -> String {
    let d: String = pdf
        .trim_start_matches("D:")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if d.len() < 8 {
        return String::new();
    }
    let part = |a: usize, b: usize, default: &str| d.get(a..b).unwrap_or(default).to_owned();
    // fuso depois dos dígitos: "Z" ou "+HH'mm'"/"-HH'mm'" → "Z" ou "+HH:mm"
    let rest = pdf.trim_start_matches("D:").get(d.len()..).unwrap_or("");
    let digits: String = rest.chars().filter(char::is_ascii_digit).take(4).collect();
    let zone = match rest.chars().next() {
        Some('Z') => "Z".to_owned(),
        Some(sign @ ('+' | '-')) if digits.len() >= 2 => {
            let minutes = digits.get(2..4).unwrap_or("00");
            format!("{sign}{}:{minutes}", &digits[..2])
        }
        _ => String::new(),
    };
    format!(
        "{}-{}-{}T{}:{}:{}{zone}",
        &d[0..4],
        &d[4..6],
        &d[6..8],
        part(8, 10, "00"),
        part(10, 12, "00"),
        part(12, 14, "00")
    )
}

/// Pacote XMP com a descrição. As declarações de PDF/A e PDF/UA do XMP antigo continuam.
fn xmp_packet(e: &PropertiesEdit, old: Option<&str>, stamps: &XmpStamps) -> String {
    let old = old.unwrap_or("");
    let mut ids = String::new();
    for (prefix, ns, elements) in [
        (
            "pdfaid",
            "http://www.aiim.org/pdfa/ns/id/",
            &["part", "conformance"][..],
        ),
        ("pdfuaid", "http://www.aiim.org/pdfua/ns/id/", &["part"][..]),
    ] {
        let found: Vec<String> = elements
            .iter()
            .filter_map(|el| {
                let tag = format!("{prefix}:{el}");
                xmp_value(old, &tag).map(|v| format!("<{tag}>{v}</{tag}>"))
            })
            .collect();
        if !found.is_empty() {
            ids.push_str(&format!(
                "<rdf:Description rdf:about=\"\" xmlns:{prefix}=\"{ns}\">{}</rdf:Description>",
                found.join("")
            ));
        }
    }
    // cada valor só aparece quando existe no /Info (o PDF/A compara os dois)
    let tag = |name: &str, v: &str| {
        if v.trim().is_empty() {
            String::new()
        } else {
            format!("<{name}>{}</{name}>", xml_escape(v))
        }
    };
    let alt = |name: &str, v: &str| {
        if v.trim().is_empty() {
            String::new()
        } else {
            format!(
                "<{name}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{name}>",
                xml_escape(v)
            )
        }
    };
    let creator = if e.author.trim().is_empty() {
        String::new()
    } else {
        format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            xml_escape(&e.author)
        )
    };
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
<dc:format>application/pdf</dc:format>{}{creator}{}</rdf:Description>\
<rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">{}{}</rdf:Description>\
<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">{}{}{}</rdf:Description>\
{ids}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>",
        alt("dc:title", &e.title),
        alt("dc:description", &e.subject),
        tag("pdf:Keywords", &e.keywords),
        tag("pdf:Producer", &stamps.producer),
        tag("xmp:CreatorTool", &stamps.creator),
        tag("xmp:CreateDate", &iso_date(&stamps.created)),
        tag("xmp:ModifyDate", &iso_date(&stamps.modified)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_escaping() {
        assert_eq!(
            iso_date("D:20261011093005-03'00'"),
            "2026-10-11T09:30:05-03:00",
            "o fuso acompanha (o PDF/A compara com o /Info)"
        );
        assert_eq!(iso_date("D:20261011093005Z"), "2026-10-11T09:30:05Z");
        assert_eq!(iso_date("D:20261011"), "2026-10-11T00:00:00");
        assert_eq!(iso_date("D:2026"), "");
        assert_eq!(xml_escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(
            xml_escape("a\u{1}b\tc\u{0}d\u{7f}"),
            "ab\tcd",
            "controles proibidos no XML 1.0 saem"
        );
        assert_eq!(xmp_value("<x:a>1</x:a>", "x:a").as_deref(), Some("1"));
        assert_eq!(xmp_value("<x:a><evil/></x:a>", "x:a"), None);
    }
}
