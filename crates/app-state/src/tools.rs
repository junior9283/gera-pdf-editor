#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Highlight,
    Underline,
    Note,
    Ink,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Select,
        Tool::Highlight,
        Tool::Underline,
        Tool::Note,
        Tool::Ink,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selecionar",
            Tool::Highlight => "Destacar",
            Tool::Underline => "Sublinhar",
            Tool::Note => "Nota",
            Tool::Ink => "Desenhar",
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
        assert_eq!(labels.len(), 5, "rótulos repetidos");
        assert_eq!(Tool::default(), Tool::Select);
    }
}
