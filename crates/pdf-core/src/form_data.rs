//! Dados do formulário (como em Preparar formulário ▸ Mais ▸ Gerenciar dados do formulário do
//! Acrobat): exportar e importar os valores dos campos em FDF, XFDF, XML ou TXT, e mesclar vários
//! arquivos numa planilha CSV. Arquivos de outros programas são lidos com tetos.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::forms::FieldKind;
use crate::properties_edit::xml_escape;
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
        Self::ALL.into_iter().find(|f| f.extension() == ext)
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
        Self::ALL.into_iter().find(|f| f.flags() == flags)
    }
}

/// Para onde o Enviar formulário pode mandar: só http, https e mailto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitTarget {
    Http(String),
    Mail(String),
}

impl SubmitTarget {
    /// Quem recebe: o servidor (http/https) ou os destinatários (mailto), para a confirmação.
    pub fn recipient(&self) -> String {
        match self {
            Self::Http(u) => url::Url::parse(u)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default(),
            Self::Mail(u) => {
                let Ok(parsed) = url::Url::parse(u) else {
                    return String::new();
                };
                let mut out = percent_decode(parsed.path());
                for (k, v) in mail_pairs(parsed.query().unwrap_or("")) {
                    match k.as_str() {
                        "cc" => out.push_str(&format!(" · cópia (cc): {v}")),
                        "bcc" => out.push_str(&format!(" · cópia oculta (bcc): {v}")),
                        _ => {}
                    }
                }
                out
            }
        }
    }
}

/// O destino do Enviar, se permitido.
pub fn submit_target(url: &str) -> Option<SubmitTarget> {
    let url = url.trim();
    if url.len() > 4096 || url.chars().any(char::is_control) {
        return None;
    }
    // o mesmo analisador do envio: o que a confirmação mostra é o que sai
    let parsed = url::Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => {
            // usuário e senha no endereço só servem para enganar ("banco.com.br@outro.com")
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return None;
            }
            let public = match parsed.host()? {
                url::Host::Domain(d) => public_domain(d),
                url::Host::Ipv4(ip) => is_public_ip(ip.into()),
                url::Host::Ipv6(ip) => is_public_ip(ip.into()),
            };
            public.then(|| SubmitTarget::Http(parsed.to_string()))
        }
        "mailto" => mail_target(&parsed),
        _ => None,
    }
}

/// Nome de domínio da internet: nada de localhost, nomes sem domínio (intranet) nem `.local`.
fn public_domain(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    let last = host.rsplit('.').next().unwrap_or_default();
    host.contains('.')
        && !last.is_empty()
        && !last.bytes().all(|b| b.is_ascii_digit())
        && !matches!(
            last,
            "localhost" | "local" | "internal" | "lan" | "home" | "localdomain"
        )
}

/// mailto só com destinatários, assunto, cc e bcc: o resto (`attach`, que alguns programas de
/// e-mail usam para anexar um arquivo local, e o `body`, que o app preenche) sai.
fn mail_target(parsed: &url::Url) -> Option<SubmitTarget> {
    // destinatários sem parâmetros escondidos (nem codificados: %3F = ?, %26 = &, %0A…)
    let to = parsed.path();
    let low = to.to_ascii_lowercase();
    if to.is_empty()
        || to.contains(['?', '&', '='])
        || ["%3f", "%26", "%3d", "%0a", "%0d", "%00"]
            .iter()
            .any(|e| low.contains(e))
    {
        return None;
    }
    let kept: Vec<String> = mail_pairs(parsed.query().unwrap_or(""))
        .into_iter()
        .filter(|(k, _)| matches!(k.as_str(), "subject" | "cc" | "bcc"))
        .map(|(k, v)| format!("{k}={}", mail_encode(&v)))
        .collect();
    let query = if kept.is_empty() {
        String::new()
    } else {
        format!("?{}", kept.join("&"))
    };
    Some(SubmitTarget::Mail(format!("mailto:{to}{query}")))
}

/// Parâmetros de um mailto (chave em minúsculas). Sem a regra do formulário: o + é literal
/// (ana+rh@x.y), como lê o programa de e-mail.
fn mail_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k).to_lowercase(), percent_decode(v))
        })
        .collect()
}

/// Decodifica %XX (UTF-8 com substituição se inválido; o + fica literal).
fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// O mailto com os dados no corpo (o mailto não leva anexo).
pub(crate) fn mail_with_body(base: &str, body: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}body={}", mail_encode(body))
}

/// Codificação de um valor de mailto (espaço como %20: o + ali é literal).
pub fn mail_encode(s: &str) -> String {
    form_encode(s).replace('+', "%20")
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
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
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
/// Maior nome completo de campo e o total lido de um arquivo (nomes + valores): um arquivo
/// pequeno com um nome pai enorme ou um valor compartilhado não multiplica a memória.
const MAX_NAME: usize = 4 * 1024;
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// Os campos lidos de um arquivo de dados, com os tetos de nome e de total.
#[derive(Default)]
struct Collected {
    out: Vec<(String, String)>,
    bytes: usize,
}

impl Collected {
    /// Guarda o campo (nome longo demais fica de fora); recusa o arquivo acima do total.
    fn push(&mut self, name: String, value: String) -> Result<()> {
        if name.len() > MAX_NAME {
            return Ok(());
        }
        self.bytes += name.len() + value.len();
        if self.bytes > MAX_OUTPUT {
            return Err(refused("o arquivo de dados tem dados demais"));
        }
        self.out.push((name, value));
        Ok(())
    }

    fn full(&self) -> bool {
        self.out.len() >= MAX_FIELDS
    }
}

/// Os valores dos campos (nome completo, valor), um por campo, na ordem do documento: sem
/// botões de ação nem assinaturas.
fn values_of(doc: &PdfDoc) -> Result<Vec<(String, String)>> {
    let mut seen = std::collections::HashSet::new();
    Ok(doc
        .all_fields()?
        .into_iter()
        .filter(|f| {
            !matches!(f.kind, FieldKind::Button | FieldKind::Signature)
                && !f.name.is_empty()
                && seen.insert(f.name.clone())
        })
        .map(|f| (f.name, f.value))
        .collect())
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

/// Nome de elemento XML a partir do nome do campo (o original vai em `xfdf:original` quando
/// muda, como o Acrobat).
fn xml_name(name: &str) -> (String, bool) {
    let mut out: String = name
        .chars()
        .map(|c| {
            // letras e algarismos ASCII: "²" e afins não são NameChar do XML
            if c.is_alphabetic() || c.is_ascii_digit() || matches!(c, '_' | '-') {
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

    /// Os dados como texto, uma linha `nome=valor` por campo (o corpo do e-mail do mailto).
    pub fn submit_text(&self) -> Result<String> {
        Ok(values_of(self)?
            .iter()
            .map(|(n, v)| format!("{n}={v}\n"))
            .collect())
    }

    /// O que a ação Enviar formulário manda, e o tipo do conteúdo.
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
        if !self.security()?.permissions.forms {
            return Err(refused("o documento não permite preencher formulários"));
        }
        let values = read_data_file(path)?;
        // o primeiro widget de cada campo que recebe valor (um grupo de botões de opção grava uma
        // vez: o estado escolhido vale para todos)
        let mut by_name: std::collections::HashMap<String, (i32, i32)> =
            std::collections::HashMap::new();
        for f in self.all_fields()? {
            // somente leitura conta como nome desconhecido (fica como está)
            if !matches!(f.kind, FieldKind::Button | FieldKind::Signature) && !f.readonly {
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
                d.write_field(*page, *id, value, false, None)?;
            }
            d.recalculate(&mut Vec::new())
        })?;
        Ok(DataImport { filled, skipped })
    }

    /// Nivelar: os campos (e, com `comments`, os comentários) viram parte da página, com a
    /// aparência que tinham, numa etapa do desfazer.
    pub fn flatten(&mut self, comments: bool) -> Result<()> {
        self.require_modifiable("nivelar")?;
        self.with_snapshot(|d| Ok(d.inner.bake(comments, true)?))
    }
}

/// Lê um arquivo de dados (pelo conteúdo: FDF, XFDF, XML ou texto delimitado por tabulação).
pub(crate) fn read_data_file(path: &Path) -> Result<Vec<(String, String)>> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(refused("o arquivo de dados é grande demais"));
    }
    let mut bytes = std::fs::read(path)?;
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        bytes.drain(..3);
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_lowercase();
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
    let mut out = Collected::default();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<(PdfObject, String, usize)> = vec![(fields, String::new(), 0)];
    while let Some((list, prefix, depth)) = stack.pop() {
        if depth > MAX_DEPTH || out.full() || !list.is_array()? {
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
            if prefix.len() + part.len() >= MAX_NAME {
                continue; // nome (e os dos filhos) longo demais
            }
            let name = match (prefix.is_empty(), part.is_empty()) {
                (true, _) => part,
                (false, true) => prefix.clone(),
                (false, false) => format!("{prefix}.{part}"),
            };
            // /V em lista (seleção múltipla) não tem como entrar num campo: fica de fora
            if let Some(v) = item
                .get_dict("V")?
                .filter(|v| !v.is_array().unwrap_or(true))
            {
                let value = if v.is_name()? {
                    String::from_utf8_lossy(&v.as_name()?).into_owned()
                } else {
                    v.as_string().unwrap_or_default()
                };
                out.push(name.clone(), value)?;
            }
            if let Some(kids) = item.get_dict("Kids")? {
                stack.push((kids, name, depth + 1));
            }
        }
    }
    Ok(out.out)
}

/// XFDF (`<field name>`/`<value>`) ou XML do Acrobat (um elemento por campo; `xfdf:original`
/// guarda o nome quando ele não é um nome XML).
fn read_xml(bytes: &[u8], xfdf: bool) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();
    let mut st = XmlFields {
        xfdf,
        ..Default::default()
    };
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|_| refused("o XML do arquivo de dados não é válido"))?;
        match event {
            Event::Start(e) => st.open(&e)?,
            // <campo/> e <value/>: um elemento sem texto (limpa o campo)
            Event::Empty(e) => {
                st.open(&e)?;
                st.close(e.local_name().as_ref())?;
            }
            // o texto e as entidades (&amp; e &#38; vêm em eventos à parte)
            Event::Text(t) => st.text(&t.decode().map(|s| s.into_owned()).unwrap_or_default()),
            Event::GeneralRef(r) => {
                let c = match r.resolve_char_ref().ok().flatten() {
                    Some(c) => Some(c),
                    None => r.decode().ok().and_then(|n| crate::xfdf::entity(&n)),
                };
                st.text(&c.map(String::from).unwrap_or_default());
            }
            Event::End(e) => st.close(e.local_name().as_ref())?,
            Event::Eof => break,
            _ => {}
        }
        if st.found.full() {
            break;
        }
        buf.clear();
    }
    Ok(st.found.out)
}

/// Um elemento aberto: nome do campo (XFDF) ou do elemento (XML), o texto e, no XFDF, se tem
/// `<value>`; no XML, se tem elementos filhos.
struct Open {
    name: String,
    text: String,
    flag: bool,
}

#[derive(Default)]
struct XmlFields {
    xfdf: bool,
    stack: Vec<Open>,
    in_value: bool,
    depth: usize,
    found: Collected,
}

impl XmlFields {
    fn open(&mut self, e: &quick_xml::events::BytesStart) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH + 3 {
            return Err(refused("o arquivo de dados é fundo demais"));
        }
        let attr = |key: &[u8]| {
            e.attributes()
                .flatten()
                .find(|a| a.key.as_ref() == key)
                .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
                .map(|v| v.into_owned())
        };
        let tag = e.local_name();
        let name = if self.xfdf {
            match tag.as_ref() {
                b"field" => attr(b"name").unwrap_or_default(),
                b"value" => {
                    self.in_value = true;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        } else if self.depth > 1 {
            // o primeiro elemento é <fields>; os outros são campos
            if let Some(parent) = self.stack.last_mut() {
                parent.flag = true;
            }
            attr(b"xfdf:original")
                .unwrap_or_else(|| String::from_utf8_lossy(tag.as_ref()).into_owned())
        } else {
            return Ok(());
        };
        self.stack.push(Open {
            name,
            text: String::new(),
            flag: false,
        });
        Ok(())
    }

    fn text(&mut self, piece: &str) {
        if self.xfdf && !self.in_value {
            return;
        }
        if let Some(top) = self.stack.last_mut() {
            if top.text.len() < MAX_VALUE {
                top.text.push_str(piece);
            }
        }
    }

    fn close(&mut self, tag: &[u8]) -> Result<()> {
        self.depth = self.depth.saturating_sub(1);
        if self.xfdf {
            match tag {
                b"value" => {
                    self.in_value = false;
                    if let Some(top) = self.stack.last_mut() {
                        top.flag = true; // tem valor
                    }
                }
                b"field" => {
                    let name = self.full_name();
                    if let Some(Open {
                        text, flag: true, ..
                    }) = self.stack.pop()
                    {
                        self.push(name, text)?;
                    }
                }
                _ => {}
            }
        } else if self.depth >= 1 && !self.stack.is_empty() {
            // só folhas (sem elementos filhos) são campos com valor
            let name = self.full_name();
            if let Some(Open {
                text, flag: false, ..
            }) = self.stack.pop()
            {
                self.push(name, text.trim().to_string())?;
            }
        }
        Ok(())
    }

    /// Os nomes da pilha unidos por ponto; `None` acima do teto (sem montar o nome enorme).
    fn full_name(&self) -> Option<String> {
        let len: usize = self.stack.iter().map(|o| o.name.len() + 1).sum();
        (len <= MAX_NAME + 1).then(|| {
            self.stack
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>()
                .join(".")
        })
    }

    fn push(&mut self, name: Option<String>, value: String) -> Result<()> {
        match name {
            Some(name) => self.found.push(name, value),
            None => Ok(()),
        }
    }
}

/// Mescla os dados de vários arquivos (PDF de formulário, FDF, XFDF, XML ou TXT) numa planilha
/// CSV: uma coluna por campo (na ordem em que aparecem) e uma linha por arquivo. Devolve quantas
/// linhas.
pub fn merge_form_data(files: &[std::path::PathBuf], dest: &Path) -> Result<usize> {
    if files.len() > MAX_MERGE {
        return Err(refused("arquivos demais para uma planilha"));
    }
    // a planilha é um .csv novo, nunca por cima de um dos arquivos a mesclar
    let csv = dest
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    if !csv || files.iter().any(|f| crate::pages::same_file(f, dest)) {
        return Err(refused(
            "a planilha precisa de um nome .csv que não seja um dos arquivos a mesclar",
        ));
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
        // ";" e tabulação também: o Excel em pt-BR separa por ";"
        if s.contains([',', ';', '\t', '"', '\n', '\r']) {
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
