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

    /// O menor retângulo que contém os dois.
    pub fn union(&self, o: &PdfRect) -> PdfRect {
        PdfRect::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }

    /// O mesmo retângulo deslocado de (`dx`, `dy`).
    pub fn translated(&self, dx: f32, dy: f32) -> PdfRect {
        PdfRect::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }

    /// O ponto está dentro (bordas incluídas).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }

    /// Os cantos estão a menos de `tol` dos de `o`.
    pub fn near(&self, o: &PdfRect, tol: f32) -> bool {
        (self.x0 - o.x0).abs() < tol
            && (self.y0 - o.y0).abs() < tol
            && (self.x1 - o.x1).abs() < tol
            && (self.y1 - o.y1).abs() < tol
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
    /// Marca de redação (a aplicar).
    Redact,
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
            AnnotKind::Highlight
                | AnnotKind::Underline
                | AnnotKind::StrikeOut
                | AnnotKind::Redact
                | AnnotKind::Other
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
    /// Texto do Preencher e assinar: sem borda, preto, a partir do canto `at`.
    FillText {
        page: i32,
        at: (f32, f32),
        text: String,
        size: f32,
    },
    /// Marca do Preencher e assinar (preta, ajustada ao retângulo).
    Mark {
        page: i32,
        rect: PdfRect,
        mark: Mark,
    },
    /// Assinatura ou iniciais do Preencher e assinar.
    Signature {
        page: i32,
        rect: PdfRect,
        art: SignatureArt,
        initials: bool,
    },
    /// Marca de redação (Redigir): sobre o texto (uma caixa por linha) ou uma área.
    Redact {
        page: i32,
        rects: Vec<PdfRect>,
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

/// Marcas do Preencher e assinar (como no Acrobat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    Cross,
    Check,
    Circle,
    Line,
    Dot,
}

/// Comando de caminho vetorial (coordenadas da arte, y para baixo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    Move(f32, f32),
    Line(f32, f32),
    Curve(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Caminho da arte de uma assinatura: preenchido (letras) ou traçado com `width` (desenho).
#[derive(Debug, Clone, PartialEq)]
pub struct VectorPath {
    pub cmds: Vec<PathCmd>,
    pub fill: bool,
    pub width: f32,
}

/// Arte da assinatura: vetorial (digitada ou desenhada; caixa 0..aspect × 0..1) ou imagem.
#[derive(Debug, Clone, PartialEq)]
pub enum SignatureArt {
    Vector { paths: Vec<VectorPath>, aspect: f32 },
    Image { path: PathBuf },
}

impl SignatureArt {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Vector { paths, .. } => paths.iter().all(|p| p.cmds.len() < 2),
            Self::Image { .. } => false,
        }
    }
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
            | Self::Signature { page, .. }
            | Self::FillText { page, .. }
            | Self::Mark { page, .. }
            | Self::Redact { page, .. }
            | Self::Stamp { page, .. } => *page,
        }
    }

    /// Anotação sem conteúdo desenhável (não deve ser guardada).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Highlight { rects, .. }
            | Self::Underline { rects, .. }
            | Self::StrikeOut { rects, .. } => rects.is_empty(),
            Self::Redact { rects, .. } => rects.iter().all(|r| r.width() < 2.0 || r.height() < 2.0),
            Self::Note { text, .. } => text.trim().is_empty(),
            Self::Ink { strokes, .. } => strokes.iter().all(|s| s.len() < 2),
            Self::TextBox { text, .. } | Self::FillText { text, .. } => text.trim().is_empty(),
            Self::Mark { rect, .. } => rect.width() < 2.0 || rect.height() < 2.0,
            Self::Signature { art, .. } if art.is_empty() => true,
            Self::Shape { rect, .. } | Self::Stamp { rect, .. } | Self::Signature { rect, .. } => {
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

/// Opções do Localizar (como no Acrobat). Todas desligadas por padrão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub whole_words: bool,
    pub match_case: bool,
    pub bookmarks: bool,
    pub comments: bool,
}

/// Link de uma página (`/Subtype /Link`), já classificado pelo que o clique faz.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkInfo {
    pub page: i32,
    /// Área clicável, em coordenadas do MuPDF (y para baixo).
    pub rect: PdfRect,
    pub target: LinkTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// Página deste documento, opcionalmente num ponto dela (coordenadas do MuPDF).
    Page { page: i32, at: Option<(f32, f32)> },
    /// Endereço `http`, `https` ou `mailto` (abre só depois do aviso de segurança).
    Uri(String),
    /// Outro PDF (caminho absoluto) e a página nele.
    File { path: PathBuf, page: Option<i32> },
    /// Programa ou endereço não permitido: nunca é seguido. Guarda o alvo, para a mensagem.
    Blocked(String),
}

/// Visualização inicial pedida pelo arquivo (`/PageMode`, `/PageLayout`, `/OpenAction`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InitialView {
    pub panel: Option<InitialPanel>,
    pub full_screen: bool,
    pub layout: Option<InitialLayout>,
    pub page: Option<i32>,
    pub zoom: Option<InitialZoom>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialPanel {
    Bookmarks,
    Thumbnails,
    Attachments,
}

/// `cover`: a primeira página sozinha (variantes `…Right`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialLayout {
    Single,
    OneColumn,
    TwoColumn { cover: bool },
    TwoPage { cover: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InitialZoom {
    FitPage,
    FitWidth,
    Factor(f32),
}

/// Anexo (painel Anexos).
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentInfo {
    /// Nome na árvore de anexos (ou o do arquivo, nos de comentário).
    pub name: String,
    pub description: String,
    pub size: usize,
    pub origin: AttachmentOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentOrigin {
    /// Anexado ao documento (`/EmbeddedFiles`).
    Document,
    /// Anexo de um comentário `FileAttachment`.
    Comment { page: i32, id: i32 },
}

/// Marcador (painel Marcadores), em ordem de exibição.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineNode {
    /// Número do objeto do marcador.
    pub id: i32,
    pub parent: Option<i32>,
    pub depth: usize,
    pub title: String,
    pub page: Option<i32>,
    /// Ponto do destino (coordenadas do MuPDF), quando o destino diz qual.
    pub at: Option<(f32, f32)>,
    /// Filhos à mostra (sinal de `/Count`).
    pub open: bool,
    pub color: Option<[f32; 3]>,
    pub bold: bool,
    pub italic: bool,
}

/// Edição dos marcadores. `after: None` põe no começo da lista dos irmãos.
#[derive(Debug, Clone, PartialEq)]
pub enum OutlineOp {
    Add {
        parent: Option<i32>,
        after: Option<i32>,
        title: String,
        page: i32,
        at: Option<(f32, f32)>,
    },
    Rename {
        id: i32,
        title: String,
    },
    /// Exclui com os filhos.
    Delete {
        id: i32,
    },
    Move {
        id: i32,
        parent: Option<i32>,
        after: Option<i32>,
    },
    Style {
        id: i32,
        color: Option<[f32; 3]>,
        bold: bool,
        italic: bool,
    },
    SetDest {
        id: i32,
        page: i32,
        at: Option<(f32, f32)>,
    },
    /// Expandir ou recolher (não entra no desfazer).
    SetOpen {
        id: i32,
        open: bool,
    },
    /// Expandir ou recolher todos (uma gravação só; fora do desfazer).
    SetAllOpen {
        open: bool,
    },
}

#[derive(Debug, Clone)]
pub struct DocSummary {
    pub path: PathBuf,
    pub sizes: Vec<PageSize>,
    pub outline: Vec<OutlineNode>,
    pub annots: Vec<AnnotInfo>,
    /// Links de todas as páginas.
    pub links: Vec<LinkInfo>,
    pub initial_view: InitialView,
    pub attachments: Vec<AttachmentInfo>,
    /// Título a mostrar no lugar do nome do arquivo ("Mostrar o título do documento").
    pub display_title: Option<String>,
    /// Rótulo de cada página (vazio quando o documento não tem rótulos).
    pub labels: Vec<String>,
    /// Campos de formulário de todas as páginas.
    pub fields: Vec<crate::FieldInfo>,
    pub form_notes: crate::FormNotes,
    /// Proteção e permissões de quem abriu.
    pub security: crate::SecurityInfo,
    /// Resultado do Comparar arquivos (`/Info /GeraCompare`): a aba abre a linha Comparação.
    pub is_comparison: bool,
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
    /// Colar: cópia da anotação em `to_page`, deslocada (`dx`, `dy`).
    Duplicate {
        page: i32,
        id: i32,
        to_page: i32,
        dx: f32,
        dy: f32,
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
    /// Propriedades personalizadas do `/Info` (nome, valor).
    pub custom: Vec<(String, String)>,
    pub initial: InitialView,
    pub window: WindowOptions,
    /// Idioma do documento (`/Lang`).
    pub lang: String,
    pub print: PrintPrefs,
    pub fonts: Vec<FontInfo>,
}

/// Opções de janela da Visualização inicial (`/ViewerPreferences`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowOptions {
    pub fit_window: bool,
    pub center_window: bool,
    /// Mostrar o título do documento (não o nome do arquivo).
    pub display_title: bool,
    pub hide_menubar: bool,
    pub hide_toolbar: bool,
    pub hide_window_ui: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplex {
    Simplex,
    ShortEdge,
    LongEdge,
}

/// Preferências de impressão pedidas pelo documento.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintPrefs {
    /// Escala de página: Nenhuma (tamanho real).
    pub scaling_none: bool,
    pub duplex: Option<Duplex>,
    pub copies: Option<u32>,
    /// Intervalos de páginas, a partir de 1, inclusive.
    pub ranges: Vec<(u32, u32)>,
}

/// Fonte usada no documento (aba Fontes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontInfo {
    pub name: String,
    /// Type1, TrueType, Type0, Type3…
    pub kind: String,
    pub embedded: bool,
    /// Embutida só com as letras usadas (nome com prefixo `ABCDEF+`).
    pub subset: bool,
}

/// O que a janela Propriedades grava.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertiesEdit {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub custom: Vec<(String, String)>,
    pub initial: InitialView,
    pub window: WindowOptions,
    pub lang: String,
    pub print: PrintPrefs,
}

impl From<&DocProperties> for PropertiesEdit {
    fn from(p: &DocProperties) -> Self {
        Self {
            title: p.title.clone(),
            author: p.author.clone(),
            subject: p.subject.clone(),
            keywords: p.keywords.clone(),
            custom: p.custom.clone(),
            initial: p.initial,
            window: p.window,
            lang: p.lang.clone(),
            print: p.print.clone(),
        }
    }
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
    /// Marcadores (com o id do criado, se houver).
    Outline(Option<i32>),
    Attachments,
}
