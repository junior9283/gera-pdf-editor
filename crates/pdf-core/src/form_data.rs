//! Dados do formulário (como em Preparar formulário ▸ Mais ▸ Gerenciar dados do formulário do
//! Acrobat): exportar e importar os valores dos campos em FDF, XFDF, XML ou TXT, e mesclar vários
//! arquivos numa planilha CSV. Arquivos de outros programas são lidos com tetos.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::forms::FieldKind;
use mupdf::pdf::{PdfDocument, PdfObject, PdfWriteOptions};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::path::Path;

/// Formato do arquivo de dados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Fdf,
    Xfdf,
    Xml,
    Txt,
}

impl DataFormat {
    pub const ALL: [DataFormat; 4] = [Self::Fdf, Self::Xfdf, Self::Xml, Self::Txt];

    /// Pela extensão do arquivo.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "fdf" => Self::Fdf,
            "xfdf" => Self::Xfdf,
            "xml" => Self::Xml,
            "txt" => Self::Txt,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Fdf => "fdf",
            Self::Xfdf => "xfdf",
            Self::Xml => "xml",
            Self::Txt => "txt",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fdf => "Arquivos FDF",
            Self::Xfdf => "Arquivos XFDF",
            Self::Xml => "Arquivos XML",
            Self::Txt => "Texto delimitado por tabulação",
        }
    }
}

/// Formato do Enviar formulário (bits de /Flags do /SubmitForm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitFormat {
    Fdf,
    Xfdf,
    Html,
    Pdf,
}

impl SubmitFormat {
    pub const ALL: [SubmitFormat; 4] = [Self::Fdf, Self::Html, Self::Xfdf, Self::Pdf];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fdf => "FDF",
            Self::Xfdf => "XFDF",
            Self::Html => "HTML",
            Self::Pdf => "PDF (o documento inteiro)",
        }
    }

    /// Os bits de /Flags: ExportFormat (HTML), XFDF e SubmitPDF.
    pub(crate) fn flags(self) -> i32 {
        match self {
            Self::Fdf => 0,
            Self::Html => 4,
            Self::Xfdf => 32,
            Self::Pdf => 256,
        }
    }

    /// Pelo /Flags; `None` se há outros bits que o app não reproduz (a ação fica como está).
    pub(crate) fn from_flags(flags: i32) -> Option<Self> {
        Some(match flags {
            0 => Self::Fdf,
            4 => Self::Html,
            32 => Self::Xfdf,
            256 => Self::Pdf,
            _ => return None,
        })
    }
}

/// Para onde o Enviar formulário pode mandar: só http, https e mailto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitTarget {
    Http(String),
    Mail(String),
}

/// O destino do Enviar, se permitido.
pub fn submit_target(url: &str) -> Option<SubmitTarget> {
    let url = url.trim();
    if url.len() > 4096 || url.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = url.split_once(':')?;
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let authority = rest
                .strip_prefix("//")?
                .split(['/', '?', '#', '\\'])
                .next()?;
            public_host(authority).then(|| SubmitTarget::Http(url.to_string()))
        }
        "mailto" if !rest.is_empty() => Some(SubmitTarget::Mail(url.to_string())),
        _ => None,
    }
}

/// O host de `user@host:porta` é público: nada de localhost, nomes sem domínio (intranet), `.local`
/// nem IP de loopback, rede privada ou link-local, em qualquer grafia (127.1, 2130706433, 0x7f…).
fn public_host(authority: &str) -> bool {
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(v6) => match v6.split_once(']').and_then(|(ip, _)| ip.parse().ok()) {
            Some(ip) => return is_public_ip(std::net::IpAddr::V6(ip)),
            None => return false,
        },
        None => host_port.split(':').next().unwrap_or_default(),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(ip) = legacy_ipv4(&host) {
        return is_public_ip(std::net::IpAddr::V4(ip));
    }
    let last = host.rsplit('.').next().unwrap_or_default();
    host.contains('.')
        && !last.is_empty()
        && !last.bytes().all(|b| b.is_ascii_digit())
        && !matches!(
            last,
            "localhost" | "local" | "internal" | "lan" | "home" | "localdomain"
        )
}

/// IPv4 na grafia que os resolvedores aceitam (inet_aton): 1 a 4 partes em decimal, octal ou hex.
fn legacy_ipv4(host: &str) -> Option<std::net::Ipv4Addr> {
    let parts: Vec<u64> = host
        .split('.')
        .map(|p| {
            if let Some(h) = p.strip_prefix("0x").or_else(|| p.strip_prefix("0X")) {
                u64::from_str_radix(h, 16).ok()
            } else if p.len() > 1 && p.starts_with('0') {
                u64::from_str_radix(&p[1..], 8).ok()
            } else {
                p.parse().ok()
            }
        })
        .collect::<Option<_>>()?;
    let (last, head) = parts.split_last()?;
    if head.len() > 3 || head.iter().any(|&p| p > 255) || *last >= 1u64 << (8 * (4 - head.len())) {
        return None;
    }
    let n = head
        .iter()
        .enumerate()
        .fold(*last, |n, (i, &p)| n | p << (24 - 8 * i));
    Some(std::net::Ipv4Addr::from(n as u32))
}

/// Endereço da internet pública (não loopback, privado, link-local, CGNAT, multicast…).
pub fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_ip(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_unspecified()
                    || v6.is_loopback()
                    || v6.is_multicast()
                    || (first & 0xfe00) == 0xfc00
                    || (first & 0xffc0) == 0xfe80
                    || first == 0x64 // NAT64 64:ff9b::
                    || first == 0x2002 // 6to4
                    || first == 0x2001 && v6.segments()[1] == 0) // Teredo
            }
        },
    }
}

/// Codificação de formulário (application/x-www-form-urlencoded).
pub fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Resultado de importar: campos preenchidos e nomes que o formulário não tem.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DataImport {
    pub filled: usize,
    pub skipped: usize,
}

/// Maior arquivo de dados lido.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Maior valor de um campo lido.
const MAX_VALUE: usize = 64 * 1024;
/// Mais campos lidos de um arquivo.
const MAX_FIELDS: usize = 10_000;
/// Hierarquia de nomes mais funda aceita.
const MAX_DEPTH: usize = 32;
/// Mais arquivos numa planilha.
const MAX_MERGE: usize = 1_000;

/// Os valores dos campos (nome completo, valor), um por campo, na ordem do documento: sem
/// botões de ação nem assinaturas.
fn values_of(doc: &PdfDoc) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for f in doc.all_fields()? {
        if matches!(f.kind, FieldKind::Button | FieldKind::Signature) || f.name.is_empty() {
            continue;
        }
        if !out.iter().any(|(n, _)| *n == f.name) {
            out.push((f.name, f.value));
        }
    }
    Ok(out)
}

/// Árvore de nomes hierárquicos ("a.b") para o FDF, o XFDF e o XML.
#[derive(Default)]
struct Node {
    children: Vec<(String, Node)>,
    value: Option<String>,
}

impl Node {
    fn insert(&mut self, name: &str, value: &str) {
        let mut node = self;
        for part in name.split('.') {
            let k = match node.children.iter().position(|(n, _)| n == part) {
                Some(k) => k,
                None => {
                    node.children.push((part.to_string(), Node::default()));
                    node.children.len() - 1
                }
            };
            node = &mut node.children[k].1;
        }
        node.value = Some(value.to_string());
    }
}

fn xml_escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Nome de elemento XML a partir do nome do campo (o original vai em `xfdf:original` quando
/// muda, como o Acrobat).
fn xml_name(name: &str) -> (String, bool) {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty()
        || !out
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        out.insert(0, '_');
    }
    let changed = out != name;
    (out, changed)
}

fn write_xfdf(node: &Node, out: &mut String) {
    for (name, child) in &node.children {
        out.push_str(&format!("<field name=\"{}\">", xml_escape(name)));
        if let Some(v) = &child.value {
            out.push_str(&format!("<value>{}</value>", xml_escape(v)));
        }
        write_xfdf(child, out);
        out.push_str("</field>");
    }
}

fn write_xml(node: &Node, out: &mut String) {
    for (name, child) in &node.children {
        let (tag, changed) = xml_name(name);
        if changed {
            out.push_str(&format!("<{tag} xfdf:original=\"{}\">", xml_escape(name)));
        } else {
            out.push_str(&format!("<{tag}>"));
        }
        if let Some(v) = &child.value {
            out.push_str(&xml_escape(v));
        }
        write_xml(child, out);
        out.push_str(&format!("</{tag}>"));
    }
}

/// Os /Fields do FDF (com /Kids para os nomes hierárquicos).
fn fdf_fields(doc: &mut PdfDocument, node: &Node) -> Result<PdfObject> {
    let mut list = doc.new_array()?;
    for (name, child) in &node.children {
        let mut d = doc.new_dict()?;
        d.dict_put("T", PdfObject::new_string(name)?)?;
        if let Some(v) = &child.value {
            d.dict_put("V", PdfObject::new_string(v)?)?;
        }
        if !child.children.is_empty() {
            d.dict_put("Kids", fdf_fields(doc, child)?)?;
        }
        list.array_push(d)?;
    }
    Ok(list)
}

impl PdfDoc {
    /// Exporta os valores dos campos. Devolve quantos.
    pub fn export_form_data(&self, path: &Path, format: DataFormat) -> Result<usize> {
        let (bytes, count) = self.form_data_bytes(format)?;
        crate::doc::write_bytes_atomic(path, &bytes)?;
        Ok(count)
    }

    /// Os dados no formato pedido e quantos campos.
    pub(crate) fn form_data_bytes(&self, format: DataFormat) -> Result<(Vec<u8>, usize)> {
        let values = values_of(self)?;
        if values.is_empty() {
            return Err(refused("o documento não tem campos de formulário"));
        }
        let mut tree = Node::default();
        for (n, v) in &values {
            tree.insert(n, v);
        }
        let source = self
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let bytes = match format {
            DataFormat::Fdf => {
                let mut out = PdfDocument::new();
                let fields = fdf_fields(&mut out, &tree)?;
                let mut fdf = out.new_dict()?;
                fdf.dict_put("Fields", fields)?;
                fdf.dict_put("F", PdfObject::new_string(&source)?)?;
                let mut root = out.new_dict()?;
                root.dict_put("FDF", fdf)?;
                let root = out.add_object(&root)?;
                out.trailer()?.dict_put("Root", root)?;
                let mut opts = PdfWriteOptions::default();
                opts.set_garbage_level(1);
                let mut bytes = Vec::new();
                out.write_to_with_options(&mut bytes, opts)?;
                // o cabeçalho do FDF (mesmo tamanho: a tabela xref continua certa)
                if bytes.starts_with(b"%PDF-") && bytes.len() >= 8 {
                    bytes[..8].copy_from_slice(b"%FDF-1.2");
                }
                bytes
            }
            DataFormat::Xfdf => {
                let mut s = String::from(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\"><fields>",
                );
                write_xfdf(&tree, &mut s);
                s.push_str(&format!(
                    "</fields><f href=\"{}\"/></xfdf>\n",
                    xml_escape(&source)
                ));
                s.into_bytes()
            }
            DataFormat::Xml => {
                let mut s = String::from(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <fields xmlns:xfdf=\"http://ns.adobe.com/xfdf-transition/\">",
                );
                write_xml(&tree, &mut s);
                s.push_str("</fields>\n");
                s.into_bytes()
            }
            DataFormat::Txt => {
                // tabulação e quebra de linha num valor quebrariam a tabela
                let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
                let names: Vec<String> = values.iter().map(|(n, _)| clean(n)).collect();
                let vals: Vec<String> = values.iter().map(|(_, v)| clean(v)).collect();
                format!("{}\n{}\n", names.join("\t"), vals.join("\t")).into_bytes()
            }
        };
        Ok((bytes, values.len()))
    }

    /// O que a ação Enviar formulário manda, e o tipo do conteúdo.
    /// Os dados como texto, uma linha `nome=valor` por campo (o corpo do e-mail do mailto).
    pub fn submit_text(&self) -> Result<String> {
        Ok(values_of(self)?
            .iter()
            .map(|(n, v)| format!("{n}={v}\n"))
            .collect())
    }

    pub fn submit_payload(&self, format: SubmitFormat) -> Result<(Vec<u8>, &'static str)> {
        Ok(match format {
            SubmitFormat::Fdf => (
                self.form_data_bytes(DataFormat::Fdf)?.0,
                "application/vnd.fdf",
            ),
            SubmitFormat::Xfdf => (
                self.form_data_bytes(DataFormat::Xfdf)?.0,
                "application/vnd.adobe.xfdf",
            ),
            SubmitFormat::Html => {
                let pairs: Vec<String> = values_of(self)?
                    .iter()
                    .map(|(n, v)| format!("{}={}", form_encode(n), form_encode(v)))
                    .collect();
                (
                    pairs.join("&").into_bytes(),
                    "application/x-www-form-urlencoded",
                )
            }
            SubmitFormat::Pdf => {
                let mut bytes = Vec::new();
                self.inner
                    .write_to_with_options(&mut bytes, PdfWriteOptions::default())?;
                (bytes, "application/pdf")
            }
        })
    }

    /// Importa os valores de um arquivo de dados (FDF, XFDF, XML ou TXT) para os campos de mesmo
    /// nome, numa etapa do desfazer; os cálculos são refeitos. Assinaturas e botões de ação não
    /// mudam.
    pub fn import_form_data(&mut self, path: &Path) -> Result<DataImport> {
        let values = read_data_file(path)?;
        // o primeiro widget de cada campo que recebe valor (um grupo de botões de opção grava uma
        // vez: o estado escolhido vale para todos)
        let mut by_name: std::collections::HashMap<String, (i32, i32)> =
            std::collections::HashMap::new();
        for f in self.all_fields()? {
            if !matches!(f.kind, FieldKind::Button | FieldKind::Signature) {
                by_name.entry(f.name).or_insert((f.page, f.id));
            }
        }
        let mut filled = 0;
        let mut skipped = 0;
        let mut writes = Vec::new();
        for (name, value) in values {
            match by_name.get(&name) {
                Some(&(page, id)) => {
                    writes.push((page, id, value));
                    filled += 1;
                }
                None => skipped += 1,
            }
        }
        if writes.is_empty() {
            return Err(refused("nenhum campo do arquivo existe neste formulário"));
        }
        self.with_snapshot(|d| {
            for (page, id, value) in &writes {
                d.write_field(*page, *id, value, true, None)?;
            }
            d.recalculate(&mut Vec::new())
        })?;
        Ok(DataImport { filled, skipped })
    }
}

impl PdfDoc {
    /// Nivelar: os campos (e, com `comments`, os comentários) viram parte da página, com a
    /// aparência que tinham, numa etapa do desfazer.
    pub fn flatten(&mut self, comments: bool) -> Result<()> {
        self.with_snapshot(|d| Ok(d.inner.bake(comments, true)?))
    }
}

/// Lê um arquivo de dados (pelo conteúdo: FDF, XFDF, XML ou texto delimitado por tabulação).
pub(crate) fn read_data_file(path: &Path) -> Result<Vec<(String, String)>> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(refused("o arquivo de dados é grande demais"));
    }
    let bytes = std::fs::read(path)?;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_lowercase();
    let mut out = if bytes.starts_with(b"%FDF-") || bytes.starts_with(b"%PDF-") {
        read_fdf(bytes)?
    } else if head.contains("<xfdf") {
        read_xml(&bytes, true)?
    } else if head.trim_start().starts_with('<') {
        read_xml(&bytes, false)?
    } else {
        read_txt(&bytes)
    };
    out.truncate(MAX_FIELDS);
    Ok(out)
}

fn read_txt(bytes: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let (Some(names), Some(values)) = (lines.next(), lines.next()) else {
        return Vec::new();
    };
    names
        .split('\t')
        .zip(values.split('\t').chain(std::iter::repeat("")))
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

/// FDF: /FDF /Fields com /Kids (nome = os /T unidos por ponto), sem repetir objetos (ciclo).
fn read_fdf(mut bytes: Vec<u8>) -> Result<Vec<(String, String)>> {
    if bytes.starts_with(b"%FDF-") && bytes.len() >= 8 {
        bytes[..8].copy_from_slice(b"%PDF-1.7");
    }
    let doc = PdfDocument::from_bytes(&bytes).map_err(|_| refused("o FDF não é válido"))?;
    let fields = doc
        .trailer()?
        .get_dict("Root")?
        .and_then(|r| r.get_dict("FDF").ok().flatten())
        .and_then(|f| f.get_dict("Fields").ok().flatten())
        .ok_or_else(|| refused("o FDF não tem campos"))?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<(PdfObject, String, usize)> = vec![(fields, String::new(), 0)];
    while let Some((list, prefix, depth)) = stack.pop() {
        if depth > MAX_DEPTH || out.len() >= MAX_FIELDS || !list.is_array()? {
            continue;
        }
        for k in (0..list.len()?.min(MAX_FIELDS) as i32).rev() {
            let Some(item) = list.get_array(k)? else {
                continue;
            };
            if item.is_indirect()? && !seen.insert(item.as_indirect()?) {
                continue;
            }
            let part = crate::pdf_obj::text(&item, "T");
            let name = match (prefix.is_empty(), part.is_empty()) {
                (true, _) => part,
                (false, true) => prefix.clone(),
                (false, false) => format!("{prefix}.{part}"),
            };
            if let Some(v) = item.get_dict("V")? {
                let value = if v.is_name()? {
                    String::from_utf8_lossy(&v.as_name()?).into_owned()
                } else {
                    v.as_string().unwrap_or_default()
                };
                out.push((name.clone(), value));
            }
            if let Some(kids) = item.get_dict("Kids")? {
                stack.push((kids, name, depth + 1));
            }
        }
    }
    Ok(out)
}

/// XFDF (`<field name>`/`<value>`) ou XML do Acrobat (um elemento por campo; `xfdf:original`
/// guarda o nome quando ele não é um nome XML).
fn read_xml(bytes: &[u8], xfdf: bool) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    // pilha de nomes dos campos (XFDF) ou dos elementos (XML), com o texto de cada um
    let mut stack: Vec<(String, String, bool)> = Vec::new();
    let mut in_value = false;
    let mut depth = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|_| refused("o XML do arquivo de dados não é válido"))?;
        match event {
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_DEPTH + 3 {
                    return Err(refused("o arquivo de dados é fundo demais"));
                }
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let attr = |key: &[u8]| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == key)
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
                        .map(|v| v.into_owned())
                };
                if xfdf {
                    match tag.as_str() {
                        "field" => {
                            stack.push((attr(b"name").unwrap_or_default(), String::new(), false))
                        }
                        "value" => in_value = true,
                        _ => {}
                    }
                } else if depth > 1 {
                    // o primeiro elemento é <fields>; os outros são campos
                    let name = attr(b"xfdf:original").unwrap_or(tag);
                    if let Some(parent) = stack.last_mut() {
                        parent.2 = true;
                    }
                    stack.push((name, String::new(), false));
                }
            }
            // o texto e as entidades (&amp; e &#38; vêm em eventos à parte)
            Event::Text(_) | Event::GeneralRef(_) if !xfdf || in_value => {
                let piece = match &event {
                    Event::Text(t) => t.decode().map(|s| s.into_owned()).unwrap_or_default(),
                    Event::GeneralRef(r) => {
                        let c = match r.resolve_char_ref().ok().flatten() {
                            Some(c) => Some(c),
                            None => r.decode().ok().and_then(|n| crate::xfdf::entity(&n)),
                        };
                        c.map(String::from).unwrap_or_default()
                    }
                    _ => String::new(),
                };
                if let Some(top) = stack.last_mut() {
                    if top.1.len() < MAX_VALUE {
                        top.1.push_str(&piece);
                    }
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let full = |stack: &[(String, String, bool)]| {
                    stack
                        .iter()
                        .map(|s| s.0.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                };
                if xfdf {
                    match tag.as_str() {
                        "value" => {
                            in_value = false;
                            if let Some(top) = stack.last_mut() {
                                top.2 = true; // tem valor
                            }
                        }
                        "field" => {
                            if let Some(top) = stack.last() {
                                if top.2 {
                                    out.push((full(&stack), top.1.clone()));
                                }
                            }
                            stack.pop();
                        }
                        _ => {}
                    }
                } else if !stack.is_empty() && depth >= 1 {
                    let top = stack.last().expect("não vazia");
                    // só folhas (sem elementos filhos) são campos com valor
                    if !top.2 {
                        out.push((full(&stack), top.1.trim().to_string()));
                    }
                    stack.pop();
                }
                if out.len() >= MAX_FIELDS {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Mescla os dados de vários arquivos (PDF de formulário, FDF, XFDF, XML ou TXT) numa planilha
/// CSV: uma coluna por campo (na ordem em que aparecem) e uma linha por arquivo. Devolve quantas
/// linhas.
pub fn merge_form_data(files: &[std::path::PathBuf], dest: &Path) -> Result<usize> {
    if files.len() > MAX_MERGE {
        return Err(refused("arquivos demais para uma planilha"));
    }
    let mut columns: Vec<String> = Vec::new();
    let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut rows: Vec<std::collections::HashMap<String, String>> = Vec::new();
    for f in files {
        let is_pdf = f.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        let values = if is_pdf {
            values_of(&PdfDoc::open(f, None)?)?
        } else {
            read_data_file(f)?
        };
        for (n, _) in &values {
            if columns.len() < MAX_FIELDS && known.insert(n.clone()) {
                columns.push(n.clone());
            }
        }
        // a primeira ocorrência de cada nome vale
        let mut row = std::collections::HashMap::new();
        for (n, v) in values {
            row.entry(n).or_insert(v);
        }
        rows.push(row);
    }
    // aspas quando preciso; e uma fórmula ("=", "+", "-", "@"…) vira texto, senão a planilha a
    // executaria ao abrir
    let cell = |s: &str| {
        // também com espaços antes, e "|" (DDE no LibreOffice)
        let risky =
            s.starts_with(['\t', '\r']) || s.trim_start().starts_with(['=', '+', '-', '@', '|']);
        let s = if risky {
            format!("'{s}")
        } else {
            s.to_string()
        };
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s
        }
    };
    let mut csv = String::new();
    csv.push_str(
        &columns
            .iter()
            .map(|c| cell(c))
            .collect::<Vec<_>>()
            .join(","),
    );
    csv.push('\n');
    for row in &rows {
        let line: Vec<String> = columns
            .iter()
            .map(|c| row.get(c).map_or_else(String::new, |v| cell(v)))
            .collect();
        csv.push_str(&line.join(","));
        csv.push('\n');
    }
    crate::doc::write_bytes_atomic(dest, csv.as_bytes())?;
    Ok(rows.len())
}
