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
    tool("criar", "Criar PDF", "📄", CreateEdit, Planned("v5"), "PDF a partir de imagens e documentos"),
    tool("editar", "Editar PDF", "✏", CreateEdit, Planned("v4"), "Alterar texto e imagens"),
    tool("exportar", "Exportar PDF", "📤", CreateEdit, Planned("v5"), "Converter para Word, Excel ou imagens"),
    tool("ocr", "Digitalizar e OCR", "🔎", CreateEdit, Planned("v5"), "Reconhecer texto em páginas escaneadas"),
    tool("combinar", "Combinar arquivos", "🧩", CreateEdit, Planned("v2"), "Juntar vários PDFs em um"),
    tool(COMMENT, "Comentar", "💬", Review, Ready, "Destacar, sublinhar, notas e desenho"),
    tool("assinar", "Preencher e assinar", "✍", Review, Planned("v3"), "Preencher campos e assinar"),
    tool("certificados", "Certificados", "🔏", Review, Planned("v3"), "Assinatura digital com certificado"),
    tool("comparar", "Comparar arquivos", "⚖", Review, Planned("v5"), "Diferenças entre duas versões"),
    tool("proteger", "Proteger PDF", "🔒", Protect, Planned("v5"), "Senha e permissões"),
    tool("redigir", "Redigir", "⬛", Protect, Planned("v5"), "Apagar informações sensíveis de vez"),
    tool("comprimir", "Comprimir PDF", "🗜", Protect, Planned("v5"), "Reduzir o tamanho do arquivo"),
    tool("formulario", "Preparar formulário", "🗒", Protect, Planned("v3"), "Criar campos de formulário"),
    tool("organizar", "Organizar páginas", "🗂", Organize, Planned("v2"), "Reordenar, girar e excluir páginas"),
    tool("dividir", "Dividir PDF", "✂", Organize, Planned("v2"), "Separar em vários arquivos"),
    tool("inserir", "Inserir páginas", "➕", Organize, Planned("v2"), "Trazer páginas de outro arquivo"),
    tool("marca-dagua", "Marca d'água", "💧", Organize, Planned("v2"), "Texto ou imagem em todas as páginas"),
    tool("cabecalho", "Cabeçalho e rodapé", "📑", Organize, Planned("v2"), "Numeração e textos nas margens"),
    tool("recortar", "Recortar páginas", "⛶", Organize, Planned("v2"), "Ajustar as margens das páginas"),
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
    fn only_comment_is_ready_in_this_phase() {
        let ready: Vec<_> = CATALOG
            .iter()
            .filter(|t| t.status == Status::Ready)
            .map(|t| t.id)
            .collect();
        assert_eq!(ready, [COMMENT]);
        assert_eq!(find(COMMENT).map(badge).as_deref(), Some(""));
        assert_eq!(
            find("organizar").map(badge).as_deref(),
            Some("Em breve · v2")
        );
        assert!(find("inexistente").is_none());
        assert!(CATALOG.iter().all(|t| t.status == Status::Ready
            || matches!(t.status, Status::Planned(p) if p.starts_with('v'))));
    }
}
