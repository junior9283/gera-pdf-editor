//! Catálogo de ferramentas: fonte única do hub Ferramentas, do painel direito e dos menus.
//! Cada fase seguinte só troca o `status` de um item para `Ready`.
use Category::*;
use Status::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    CreateEdit,
    Review,
    Protect,
    Organize,
}

impl Category {
    pub const ALL: [Category; 4] = [CreateEdit, Review, Protect, Organize];

    pub fn label(self) -> &'static str {
        match self {
            CreateEdit => "Criar e editar",
            Review => "Revisar e aprovar",
            Protect => "Proteger e padronizar",
            Organize => "Organizar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ready,
    /// Chega na fase indicada (ex.: "v2").
    Planned(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub category: Category,
    pub status: Status,
    pub description: &'static str,
}

pub const COMMENT: &str = "comentar";
pub const FILL_SIGN: &str = "assinar";
pub const PREPARE_FORM: &str = "formulario";
pub const CERTIFICATES: &str = "certificados";
pub const PROTECT: &str = "proteger";
pub const COMPRESS: &str = "comprimir";
pub const CREATE: &str = "criar";
pub const EDIT_PDF: &str = "editar";
pub const WATERMARK: &str = "marca-dagua";
pub const REDACT: &str = "redigir";
pub const HEADER_FOOTER: &str = "cabecalho";
pub const EXPORT: &str = "exportar";
pub const OCR: &str = "ocr";
/// Ferramentas que abrem o Organizar páginas.
pub const ORGANIZE: &str = "organizar";
pub const INSERT: &str = "inserir";
pub const CROP: &str = "recortar";
pub const SPLIT: &str = "dividir";
pub const COMBINE: &str = "combinar";

const fn tool(
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    category: Category,
    status: Status,
    description: &'static str,
) -> ToolEntry {
    ToolEntry {
        id,
        name,
        icon,
        category,
        status,
        description,
    }
}

#[rustfmt::skip]
pub const CATALOG: &[ToolEntry] = &[
    tool(CREATE, "Criar PDF", "📄", CreateEdit, Ready, "PDF a partir de imagens"),
    tool(EDIT_PDF, "Editar PDF", "✏", CreateEdit, Ready, "Editar texto e imagens"),
    tool(EXPORT, "Exportar PDF", "📤", CreateEdit, Ready, "Converter para Word, texto ou imagens"),
    tool(OCR, "Digitalizar e OCR", "🔎", CreateEdit, Ready, "Reconhecer texto em páginas escaneadas"),
    tool(COMBINE, "Combinar arquivos", "🗐", CreateEdit, Ready, "Juntar vários PDFs em um"),
    tool(COMMENT, "Comentar", "💬", Review, Ready, "Destacar, sublinhar, notas e desenho"),
    tool(FILL_SIGN, "Preencher e assinar", "🖊", Review, Ready, "Texto, marcas, assinatura e iniciais"),
    tool(CERTIFICATES, "Certificados", "🔏", Review, Ready, "Assinatura digital com certificado e validação"),
    tool("comparar", "Comparar arquivos", "⚖", Review, Planned("v5"), "Diferenças entre duas versões"),
    tool(PROTECT, "Proteger PDF", "🔒", Protect, Ready, "Senha e permissões"),
    tool(REDACT, "Redigir", "⬛", Protect, Ready, "Apagar informações sensíveis de vez"),
    tool(COMPRESS, "Comprimir PDF", "⛃", Protect, Ready, "Reduzir o tamanho do arquivo"),
    tool(PREPARE_FORM, "Preparar formulário", "🗋", Protect, Ready, "Criar e editar campos de formulário"),
    tool("organizar", "Organizar páginas", "🗁", Organize, Ready, "Reordenar, girar e excluir páginas"),
    tool(SPLIT, "Dividir PDF", "✂", Organize, Ready, "Separar em vários arquivos"),
    tool("inserir", "Inserir páginas", "➕", Organize, Ready, "Trazer páginas de outro arquivo"),
    tool(WATERMARK, "Marca d'água", "💧", Organize, Ready, "Texto ou imagem em todas as páginas"),
    tool(HEADER_FOOTER, "Cabeçalho e rodapé", "📑", Organize, Ready, "Numeração, data e Bates nas margens"),
    tool("recortar", "Recortar páginas", "⛶", Organize, Ready, "Ajustar as margens das páginas"),
];

pub fn find(id: &str) -> Option<&'static ToolEntry> {
    CATALOG.iter().find(|t| t.id == id)
}

/// "Em breve · v2" para o que ainda não existe; vazio para o que está pronto.
pub fn badge(t: &ToolEntry) -> String {
    match t.status {
        Ready => String::new(),
        Planned(phase) => format!("Em breve · {phase}"),
    }
}

pub fn in_category(c: Category) -> impl Iterator<Item = &'static ToolEntry> {
    CATALOG.iter().filter(move |t| t.category == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let ids: std::collections::HashSet<_> = CATALOG.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn every_category_has_tools_and_every_tool_is_described() {
        for c in Category::ALL {
            assert!(in_category(c).count() > 0, "{c:?}");
        }
        for t in CATALOG {
            assert!(
                !t.name.is_empty() && !t.description.is_empty() && !t.icon.is_empty(),
                "{}",
                t.id
            );
        }
    }

    #[test]
    fn tools_of_the_finished_phases_are_ready() {
        let ready: Vec<_> = CATALOG
            .iter()
            .filter(|t| t.status == Status::Ready)
            .map(|t| t.id)
            .collect();
        assert_eq!(
            ready,
            [
                CREATE,
                EDIT_PDF,
                EXPORT,
                OCR,
                COMBINE,
                COMMENT,
                FILL_SIGN,
                CERTIFICATES,
                PROTECT,
                REDACT,
                COMPRESS,
                PREPARE_FORM,
                ORGANIZE,
                SPLIT,
                INSERT,
                WATERMARK,
                HEADER_FOOTER,
                CROP
            ]
        );
        assert_eq!(find(COMMENT).map(badge).as_deref(), Some(""));
        assert_eq!(
            find("comparar").map(badge).as_deref(),
            Some("Em breve · v5")
        );
        assert!(find("inexistente").is_none());
        assert!(CATALOG.iter().all(|t| t.status == Status::Ready
            || matches!(t.status, Status::Planned(p) if p.starts_with('v'))));
    }
}
