#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Highlight,
    Underline,
    StrikeOut,
    /// Substituir texto (riscado + cursor com nota) e Inserir texto (cursor com nota).
    ReplaceText,
    InsertText,
    Note,
    Ink,
    TextBox,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Polygon,
    /// Linhas conectadas (polilinha).
    PolyLine,
    Cloud,
    /// Borracha do lápis.
    Eraser,
    /// Texto explicativo.
    Callout,
    /// O carimbo escolhido fica no estado de edição.
    Stamp,
    /// Anexar arquivo como comentário.
    Attach,
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

    /// As ferramentas da linha Comentar.
    pub const ALL: [Tool; 20] = [
        Tool::Select,
        Tool::Highlight,
        Tool::Underline,
        Tool::StrikeOut,
        Tool::ReplaceText,
        Tool::InsertText,
        Tool::Note,
        Tool::TextBox,
        Tool::Callout,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Line,
        Tool::Cloud,
        Tool::Polygon,
        Tool::PolyLine,
        Tool::Ink,
        Tool::Eraser,
        Tool::Stamp,
        Tool::Attach,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selecionar",
            Tool::Highlight => "Destacar",
            Tool::Underline => "Sublinhar",
            Tool::StrikeOut => "Riscar",
            Tool::ReplaceText => "Substituir texto",
            Tool::InsertText => "Inserir texto",
            Tool::Polygon => "Polígono",
            Tool::PolyLine => "Linhas conectadas",
            Tool::Cloud => "Nuvem",
            Tool::Eraser => "Borracha",
            Tool::Callout => "Texto explicativo",
            Tool::Attach => "Anexar arquivo",
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

/// Grupos que uma tecla com Shift percorre (como no Acrobat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolGroup {
    /// `Shift+U`: Destacar, Sublinhar, Riscar.
    Markup,
    /// `Shift+D`: as ferramentas de desenho.
    Drawing,
    /// `Shift+J`: Carimbo e Anexar arquivo.
    Stamp,
}

impl ToolGroup {
    pub fn tools(self) -> &'static [Tool] {
        match self {
            ToolGroup::Markup => &[Tool::Highlight, Tool::Underline, Tool::StrikeOut],
            ToolGroup::Drawing => &[
                Tool::Rect,
                Tool::Ellipse,
                Tool::Arrow,
                Tool::Line,
                Tool::Cloud,
                Tool::Polygon,
                Tool::PolyLine,
                Tool::Ink,
                Tool::Eraser,
            ],
            ToolGroup::Stamp => &[Tool::Stamp, Tool::Attach],
        }
    }
}

/// A ferramenta seguinte do grupo (fora dele: a primeira).
pub fn cycle(group: ToolGroup, current: Tool) -> Tool {
    let tools = group.tools();
    match tools.iter().position(|&t| t == current) {
        Some(k) => tools[(k + 1) % tools.len()],
        None => tools[0],
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
    fn shift_u_cycles_highlight_underline_strikeout() {
        use Tool::*;
        let g = ToolGroup::Markup;
        assert_eq!(cycle(g, Highlight), Underline);
        assert_eq!(cycle(g, Underline), StrikeOut);
        assert_eq!(cycle(g, StrikeOut), Highlight);
        assert_eq!(cycle(g, Note), Highlight, "fora do grupo: a primeira");
    }

    #[test]
    fn shift_d_cycles_the_drawing_tools_like_acrobat() {
        use Tool::*;
        let mut t = Rect;
        let mut seen = vec![t];
        for _ in 0..8 {
            t = cycle(ToolGroup::Drawing, t);
            seen.push(t);
        }
        assert_eq!(
            seen,
            [Rect, Ellipse, Arrow, Line, Cloud, Polygon, PolyLine, Ink, Eraser]
        );
        assert_eq!(cycle(ToolGroup::Drawing, Eraser), Rect);
    }

    #[test]
    fn shift_j_alternates_stamp_and_attach() {
        assert_eq!(cycle(ToolGroup::Stamp, Tool::Stamp), Tool::Attach);
        assert_eq!(cycle(ToolGroup::Stamp, Tool::Attach), Tool::Stamp);
    }

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
