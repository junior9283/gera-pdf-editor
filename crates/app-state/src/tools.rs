#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Highlight,
    Underline,
    Note,
    Ink,
    TextBox,
    Rect,
    Ellipse,
    Line,
    Arrow,
    /// O carimbo escolhido fica no estado de edição.
    Stamp,
    /// Uma ferramenta do Preencher e assinar (qual, fica no estado de edição); fora de `ALL`,
    /// que é a linha Comentar.
    Fill,
}

impl Tool {
    pub const ALL: [Tool; 11] = [
        Tool::Select,
        Tool::Highlight,
        Tool::Underline,
        Tool::Note,
        Tool::Ink,
        Tool::TextBox,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Arrow,
        Tool::Stamp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selecionar",
            Tool::Highlight => "Destacar",
            Tool::Underline => "Sublinhar",
            Tool::Note => "Nota",
            Tool::Ink => "Desenhar",
            Tool::TextBox => "Caixa de texto",
            Tool::Rect => "Retângulo",
            Tool::Ellipse => "Elipse",
            Tool::Line => "Linha",
            Tool::Arrow => "Seta",
            Tool::Stamp => "Carimbo",
            Tool::Fill => "Preencher e assinar",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_listed_once_with_a_distinct_label() {
        let mut labels: Vec<_> = Tool::ALL.iter().map(|t| t.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Tool::ALL.len(), "rótulos repetidos");
        assert_eq!(Tool::default(), Tool::Select);
    }
}
