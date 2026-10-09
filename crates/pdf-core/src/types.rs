use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotKind {
    Highlight,
    Underline,
    Note,
    Ink,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotInfo {
    pub kind: AnnotKind,
    pub page: i32,
    pub rect: PdfRect,
    pub contents: String,
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
    Note {
        page: i32,
        at: (f32, f32),
        text: String,
    },
    Ink {
        page: i32,
        strokes: Vec<Vec<(f32, f32)>>,
    },
}

impl NewAnnot {
    pub fn page(&self) -> i32 {
        match self {
            Self::Highlight { page, .. }
            | Self::Underline { page, .. }
            | Self::Note { page, .. }
            | Self::Ink { page, .. } => *page,
        }
    }

    /// Anotação sem conteúdo desenhável (não deve ser guardada).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Highlight { rects, .. } | Self::Underline { rects, .. } => rects.is_empty(),
            Self::Note { text, .. } => text.trim().is_empty(),
            Self::Ink { strokes, .. } => strokes.iter().all(|s| s.len() < 2),
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
