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
    /// Redigir: marcar texto (arrastando sobre ele) ou uma área (retângulo); fora de `ALL`.
    RedactText,
    RedactArea,
    /// Mão: arrastar move a página (`H`; Espaço segurado). Fora de `ALL`.
    Hand,
    /// Zoom com marca: arrastar um retângulo amplia até ele (`Z`). Fora de `ALL`.
    Zoom,
}

impl Tool {
    /// Ferramentas de tela da barra do documento: não criam nada na página.
    pub const SCREEN: [Tool; 3] = [Tool::Select, Tool::Hand, Tool::Zoom];

    pub fn is_screen_tool(self) -> bool {
        Self::SCREEN.contains(&self)
    }

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
            Tool::RedactText => "Marcar texto para redação",
            Tool::RedactArea => "Marcar área para redação",
            Tool::Hand => "Mão",
            Tool::Zoom => "Zoom com marca",
        }
    }
}

/// Mão temporária do Espaço segurado: guarda a ferramenta de antes e a devolve ao soltar.
#[derive(Debug, Default)]
pub struct TempTool {
    saved: Option<Tool>,
}

impl TempTool {
    /// Espaço pressionado (ou ainda segurado): a ferramenta passa a ser a Mão.
    pub fn press(&mut self, current: Tool) -> Tool {
        self.saved.get_or_insert(current);
        Tool::Hand
    }

    /// Espaço solto: a ferramenta de antes, se havia uma guardada.
    pub fn release(&mut self) -> Option<Tool> {
        self.saved.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_hand_is_temporary() {
        let mut t = TempTool::default();
        assert_eq!(t.press(Tool::Note), Tool::Hand);
        assert_eq!(
            t.press(Tool::Hand),
            Tool::Hand,
            "segurar não perde a anterior"
        );
        assert_eq!(t.release(), Some(Tool::Note));
        assert_eq!(t.release(), None, "soltar de novo não muda nada");
    }

    #[test]
    fn hand_and_zoom_are_not_comment_tools() {
        for t in [Tool::Hand, Tool::Zoom] {
            assert!(!Tool::ALL.contains(&t), "{t:?} fica fora da linha Comentar");
            assert!(!t.label().is_empty());
            assert!(t.is_screen_tool());
        }
        assert!(Tool::Select.is_screen_tool() && !Tool::Note.is_screen_tool());
    }

    #[test]
    fn every_tool_is_listed_once_with_a_distinct_label() {
        let mut labels: Vec<_> = Tool::ALL.iter().map(|t| t.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Tool::ALL.len(), "rótulos repetidos");
        assert_eq!(Tool::default(), Tool::Select);
    }
}
