use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PdfRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl PdfRect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// Retângulo com os cantos em qualquer ordem, reordenados para x0<=x1 e y0<=y1.
    pub fn from_corners(a: (f32, f32), b: (f32, f32)) -> Self {
        Self::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// O mesmo retângulo deslocado de (`dx`, `dy`).
    pub fn translated(&self, dx: f32, dy: f32) -> PdfRect {
        PdfRect::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }

    pub fn intersects(&self, o: &PdfRect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

impl From<mupdf::Rect> for PdfRect {
    fn from(r: mupdf::Rect) -> Self {
        Self::new(r.x0, r.y0, r.x1, r.y1)
    }
}

impl From<PdfRect> for mupdf::Rect {
    fn from(r: PdfRect) -> Self {
        mupdf::Rect::new(r.x0, r.y0, r.x1, r.y1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// 3 bytes por pixel (RGB), linhas contíguas.
    pub rgb: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: PdfRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnotKind {
    Highlight,
    Underline,
    Note,
    Ink,
    StrikeOut,
    TextBox,
    Rect,
    Ellipse,
    Line,
    Stamp,
    /// Tipo que não criamos (de outro programa): selecionável e apagável.
    #[default]
    Other,
}

/// Início e fim de uma linha (pontos da página).
pub type Segment = ((f32, f32), (f32, f32));

/// Cor dos comentários novos (contorno das formas e linhas, texto da caixa), como no Acrobat.
pub const COMMENT_RED: [f32; 3] = [1.0, 0.0, 0.0];

/// Modelo de estado de um comentário (como o Acrobat grava): revisão ou visto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateModel {
    /// Revisão: /State Accepted, Rejected, Cancelled, Completed ou None.
    Review,
    /// Visto: /State Marked ou Unmarked.
    Marked,
}

impl StateModel {
    pub fn pdf_name(self) -> &'static str {
        match self {
            Self::Review => "Review",
            Self::Marked => "Marked",
        }
    }
}

/// Ponta de uma linha ou seta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    Start,
    End,
}

impl AnnotKind {
    /// Tem traço/borda com espessura editável.
    pub fn has_border(self) -> bool {
        matches!(
            self,
            Self::Ink | Self::TextBox | Self::Rect | Self::Ellipse | Self::Line
        )
    }

    /// Tem preenchimento (formas) ou fundo (caixa de texto).
    pub fn has_fill(self) -> bool {
        matches!(self, Self::Rect | Self::Ellipse | Self::TextBox)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotInfo {
    /// Número do objeto no PDF: identidade estável da anotação.
    pub id: i32,
    pub kind: AnnotKind,
    pub page: i32,
    pub rect: PdfRect,
    pub contents: String,
    pub color: Option<[f32; 3]>,
    pub opacity: f32,
    /// Espessura do traço (desenho, formas, linhas e borda da caixa de texto).
    pub width: Option<f32>,
    /// Cor de preenchimento (formas e caixa de texto).
    pub fill: Option<[f32; 3]>,
    /// Pontos de início e fim (linha e seta).
    pub line: Option<Segment>,
    /// Linha com ponta de seta.
    pub arrow: bool,
    /// Tamanho e cor do texto (caixa de texto).
    pub font: Option<(f32, [f32; 3])>,
    /// Autor (/T); vazio quando não há.
    pub author: String,
    /// Comentário de que esta é resposta (ou status): id do pai (/IRT).
    pub reply_to: Option<i32>,
    /// Anotação de status: modelo e estado (nome PDF, ex. "Accepted").
    pub state: Option<(StateModel, String)>,
    /// Datas cruas do PDF (`D:AAAAMMDDhhmmss…`): criação e última modificação.
    pub created: String,
    pub modified: String,
}

impl AnnotInfo {
    /// Pode ser arrastada (marcações de texto ficam presas às palavras, como no Acrobat).
    pub fn movable(&self) -> bool {
        !matches!(
            self.kind,
            AnnotKind::Highlight | AnnotKind::Underline | AnnotKind::StrikeOut | AnnotKind::Other
        )
    }

    /// Resposta ou status de outro comentário (fica no painel Comentários, não na página).
    pub fn is_reply(&self) -> bool {
        self.reply_to.is_some()
    }

    /// Tem alças nos cantos para mudar de tamanho (a linha tem alças nas pontas).
    pub fn resizable(&self) -> bool {
        matches!(
            self.kind,
            AnnotKind::Ink
                | AnnotKind::TextBox
                | AnnotKind::Rect
                | AnnotKind::Ellipse
                | AnnotKind::Stamp
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewAnnot {
    Highlight {
        page: i32,
        rects: Vec<PdfRect>,
    },
    Underline {
        page: i32,
        rects: Vec<PdfRect>,
    },
    StrikeOut {
        page: i32,
        rects: Vec<PdfRect>,
    },
    Note {
        page: i32,
        at: (f32, f32),
        text: String,
    },
    Ink {
        page: i32,
        strokes: Vec<Vec<(f32, f32)>>,
    },
    TextBox {
        page: i32,
        rect: PdfRect,
        text: String,
        size: f32,
        color: [f32; 3],
    },
    /// Retângulo ou elipse.
    Shape {
        page: i32,
        rect: PdfRect,
        ellipse: bool,
    },
    /// Linha; com `arrow`, ponta de seta no fim.
    Line {
        page: i32,
        a: (f32, f32),
        b: (f32, f32),
        arrow: bool,
    },
    /// Carimbo: título e, nos dinâmicos, a linha com nome, data e hora.
    Stamp {
        page: i32,
        rect: PdfRect,
        title: String,
        subtitle: Option<String>,
        color: [f32; 3],
    },
}

impl NewAnnot {
    pub fn page(&self) -> i32 {
        match self {
            Self::Highlight { page, .. }
            | Self::Underline { page, .. }
            | Self::StrikeOut { page, .. }
            | Self::Note { page, .. }
            | Self::Ink { page, .. }
            | Self::TextBox { page, .. }
            | Self::Shape { page, .. }
            | Self::Line { page, .. }
            | Self::Stamp { page, .. } => *page,
        }
    }

    /// Anotação sem conteúdo desenhável (não deve ser guardada).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Highlight { rects, .. }
            | Self::Underline { rects, .. }
            | Self::StrikeOut { rects, .. } => rects.is_empty(),
            Self::Note { text, .. } => text.trim().is_empty(),
            Self::Ink { strokes, .. } => strokes.iter().all(|s| s.len() < 2),
            Self::TextBox { text, .. } => text.trim().is_empty(),
            Self::Shape { rect, .. } | Self::Stamp { rect, .. } => {
                rect.width() < 3.0 || rect.height() < 3.0
            }
            Self::Line { a, b, .. } => (b.0 - a.0).hypot(b.1 - a.1) < 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<i32>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct DocSummary {
    pub path: PathBuf,
    pub sizes: Vec<PageSize>,
    pub outline: Vec<OutlineItem>,
    pub annots: Vec<AnnotInfo>,
    /// Campos de formulário de todas as páginas.
    pub fields: Vec<crate::FieldInfo>,
    pub form_notes: crate::FormNotes,
}

/// Rotação da vista, no sentido horário. Só muda a tela, nunca o arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn degrees(self) -> u16 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    /// Um quarto de volta no sentido horário.
    pub fn cw(self) -> Self {
        match self {
            Rotation::R0 => Rotation::R90,
            Rotation::R90 => Rotation::R180,
            Rotation::R180 => Rotation::R270,
            Rotation::R270 => Rotation::R0,
        }
    }

    /// Um quarto de volta no sentido anti-horário.
    pub fn ccw(self) -> Self {
        self.cw().cw().cw()
    }

    /// A 90° e 270° largura e altura trocam de lugar.
    pub fn is_sideways(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}

/// Texto escolhido numa página: as áreas marcadas (em ordem de leitura) e o texto delas.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextSelection {
    pub quads: Vec<PdfRect>,
    pub text: String,
}

/// Operação do usuário sobre as anotações (aplicada na hora, com desfazer).
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotOp {
    Create(NewAnnot),
    Move {
        page: i32,
        id: i32,
        dx: f32,
        dy: f32,
    },
    Resize {
        page: i32,
        id: i32,
        rect: PdfRect,
    },
    Style {
        page: i32,
        id: i32,
        color: Option<[f32; 3]>,
        opacity: f32,
        width: Option<f32>,
    },
    SetContents {
        page: i32,
        id: i32,
        text: String,
    },
    Delete {
        page: i32,
        id: i32,
    },
    /// Responde ao comentário `parent` (resposta na conversa, como no Acrobat).
    Reply {
        page: i32,
        parent: i32,
        text: String,
    },
    /// Define o status (revisão) ou o visto do comentário `parent`; `state` é o nome PDF.
    SetState {
        page: i32,
        parent: i32,
        model: StateModel,
        state: String,
    },
    /// Leva uma ponta da linha/seta para `to`.
    MoveLinePoint {
        page: i32,
        id: i32,
        end: LineEnd,
        to: (f32, f32),
    },
    /// Preenchimento (formas) ou fundo (caixa de texto); `None` tira.
    Fill {
        page: i32,
        id: i32,
        color: Option<[f32; 3]>,
    },
    /// Tamanho e cor do texto da caixa de texto (a borda segue a cor do texto).
    Font {
        page: i32,
        id: i32,
        size: f32,
        color: [f32; 3],
    },
}

/// Propriedades do documento (Arquivo → Propriedades). As datas vêm cruas (`D:AAAAMMDD…`).
#[derive(Debug, Clone, PartialEq)]
pub struct DocProperties {
    pub path: PathBuf,
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    /// Aplicativo que criou o documento original.
    pub creator: String,
    /// Programa que gerou o PDF.
    pub producer: String,
    pub created: String,
    pub modified: String,
    /// Ex.: "PDF 1.7".
    pub format: String,
    pub encrypted: bool,
    pub pages: i32,
    pub first_page: PageSize,
    pub file_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(kind: AnnotKind) -> AnnotInfo {
        AnnotInfo {
            id: 1,
            kind,
            page: 0,
            rect: PdfRect::new(0.0, 0.0, 1.0, 1.0),
            opacity: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn only_notes_and_drawings_move_and_only_drawings_resize() {
        assert!(info(AnnotKind::Note).movable() && !info(AnnotKind::Note).resizable());
        assert!(info(AnnotKind::Ink).movable() && info(AnnotKind::Ink).resizable());
        for k in [
            AnnotKind::Highlight,
            AnnotKind::Underline,
            AnnotKind::StrikeOut,
            AnnotKind::Other,
        ] {
            assert!(!info(k).movable() && !info(k).resizable(), "{k:?}");
        }
    }
}

/// Estado do desfazer/refazer das edições de anotação.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
    /// Há edições ainda não gravadas no arquivo.
    pub dirty: bool,
}

/// Operação na organização das páginas (índices a partir de 0). Desfazer volta um instantâneo.
#[derive(Debug, Clone, PartialEq)]
pub enum PageOp {
    /// Gira as páginas em quartos de volta (positivo: horário).
    Rotate { pages: Vec<i32>, quarter_turns: i32 },
    /// Exclui as páginas (nunca todas).
    Delete { pages: Vec<i32> },
    /// Leva as páginas (na ordem em que estão) para antes da página `to` (`to` = total: fim).
    Move { pages: Vec<i32>, to: i32 },
    /// Página em branco antes de `at`, com o tamanho em pontos.
    InsertBlank { at: i32, size: (f32, f32) },
    /// Páginas de outro PDF antes de `at` (`range` = primeira e última, a partir de 0).
    InsertFrom {
        at: i32,
        path: PathBuf,
        password: Option<String>,
        range: Option<(i32, i32)>,
    },
    /// Troca cada página de `pages` pela página `from + k` do outro PDF.
    Replace {
        pages: Vec<i32>,
        path: PathBuf,
        password: Option<String>,
        from: i32,
    },
    /// Recorta (CropBox) as margens em pontos: cima, baixo, esquerda, direita.
    Crop { pages: Vec<i32>, margins: [f32; 4] },
}

/// O que um desfazer/refazer mudou: anotações destas páginas, ou o documento inteiro (páginas).
#[derive(Debug, Clone, PartialEq)]
pub enum Changed {
    Pages(Vec<i32>),
    Document,
    /// Valores de campos de formulário (um campo pode ter widgets em várias páginas).
    Fields,
}
