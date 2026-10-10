use crate::annot_edit::{self, reply_of, ColorEdit, Edit, History, Snapshot, Step};
use crate::error::{PdfError, Result};
use crate::stamp;
use crate::text_fit;
use crate::types::{
    AnnotInfo, AnnotKind, AnnotOp, Changed, DocProperties, DocSummary, HistoryState, LineEnd, Mark,
    NewAnnot, OutlineItem, PageSize, PdfRect, RenderedPage, Rotation, StateModel, TextSelection,
    Word, COMMENT_RED,
};
use mupdf::color::AnnotationColor;
use mupdf::pdf::{
    AnnotationFlags, Encryption, LineEndingStyle, PdfAnnotation, PdfAnnotationType, PdfDocument,
    PdfObject, PdfPage, PdfWriteOptions,
};
use mupdf::{
    Colorspace, Matrix, MetadataName, Point, Quad, Rect, TextExtractOptions, TextPageFlags,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Limite de pixels por imagem renderizada (evita estourar a memória).
const MAX_PIXELS: f64 = 40_000_000.0;
/// Limite do maior lado da imagem renderizada: texturas maiores que isso passam do máximo de
/// qualquer GPU comum e derrubariam a interface.
const MAX_SIDE_PX: f64 = 16_384.0;

/// Maior escala em que `PdfDoc::render` aceita uma página desse tamanho (por lado e por total
/// de pixels). Sempre positiva e finita, mesmo para tamanhos degenerados.
pub fn max_render_scale(size: &PageSize) -> f32 {
    let (w, h) = (
        f64::from(size.width.max(1.0)),
        f64::from(size.height.max(1.0)),
    );
    (MAX_SIDE_PX / w.max(h)).min((MAX_PIXELS / (w * h)).sqrt()) as f32
}
/// Agulhas maiores que isso (em bytes) não são buscadas.
const MAX_NEEDLE_BYTES: usize = 4096;
/// Teto de resultados por página (proteção contra páginas com milhares de ocorrências).
const MAX_HITS_PER_PAGE: u32 = 512;

pub struct PdfDoc {
    pub(crate) inner: PdfDocument,
    pub(crate) path: PathBuf,
    /// Guardada só em memória, para reabrir o arquivo (`save_with` descarta as mudanças em erro).
    pub(crate) password: Option<String>,
    /// Desfazer/refazer das edições de anotação desde a última gravação.
    pub(crate) history: History,
    /// Edições que sobreviveram a uma gravação que falhou (o histórico delas já se perdeu).
    unsaved: bool,
    /// Nome gravado como autor (/T) nas anotações criadas.
    author: Option<String>,
}

/// Abre o arquivo e, se for protegido, autentica com `password`.
pub(crate) fn open_authenticated(path: &Path, password: Option<&str>) -> Result<PdfDocument> {
    let mut inner = PdfDocument::open(path).map_err(|e| PdfError::Open(e.to_string()))?;
    if inner.needs_password()? {
        match password {
            None => return Err(PdfError::PasswordRequired),
            Some(p) if !inner.authenticate(p)? => return Err(PdfError::WrongPassword),
            Some(_) => {}
        }
    }
    Ok(inner)
}

impl PdfDoc {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let inner = open_authenticated(path, password)?;
        Ok(Self {
            inner,
            path: path.to_path_buf(),
            password: password.map(str::to_owned),
            history: History::default(),
            unsaved: false,
            author: None,
        })
    }

    /// Caminho do arquivo aberto (muda com Salvar como).
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn page_count(&self) -> Result<i32> {
        Ok(self.inner.page_count()?)
    }

    pub(crate) fn check_page(&self, page: i32) -> Result<()> {
        if page < 0 || page >= self.page_count()? {
            return Err(PdfError::PageOutOfRange(page));
        }
        Ok(())
    }

    pub(crate) fn load(&self, page: i32) -> Result<mupdf::Page> {
        self.check_page(page)?;
        Ok(self.inner.load_page(page)?)
    }

    pub fn page_size(&self, page: i32) -> Result<PageSize> {
        size_of(&self.load(page)?)
    }

    pub fn render(&self, page: i32, scale: f32, invert: bool) -> Result<RenderedPage> {
        self.render_rotated(page, scale, invert, Rotation::R0)
    }

    /// Como `render`, com a vista girada (horário). O MuPDF posiciona a imagem pela caixa da
    /// página já transformada, então a origem continua no canto superior esquerdo.
    pub fn render_rotated(
        &self,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
    ) -> Result<RenderedPage> {
        let page = self.load(page)?;
        let size = size_of(&page)?;
        if !scale.is_finite() || scale <= 0.0 || scale > max_render_scale(&size) {
            return Err(PdfError::InvalidScale(scale));
        }
        let mut ctm = Matrix::new_rotate(f32::from(rotation.degrees()));
        ctm.scale(scale, scale); // escala uniforme: comuta com a rotação
        let pix = page.to_pixmap(&ctm, &Colorspace::device_rgb(), false, true)?;
        let (w, h, stride) = (
            pix.width() as usize,
            pix.height() as usize,
            pix.stride() as usize,
        );
        debug_assert_eq!(pix.n(), 3, "device_rgb sem alfa tem 3 canais");
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in pix.samples().chunks(stride).take(h) {
            let row = &row[..w * 3];
            if invert {
                rgb.extend(row.iter().map(|b| !b));
            } else {
                rgb.extend_from_slice(row);
            }
        }
        Ok(RenderedPage {
            width: w as u32,
            height: h as u32,
            rgb,
        })
    }

    pub fn search(&self, page: i32, needle: &str) -> Result<Vec<PdfRect>> {
        let page = self.load(page)?;
        if needle.is_empty() || needle.contains('\0') || needle.len() > MAX_NEEDLE_BYTES {
            return Ok(Vec::new());
        }
        Ok(page
            .search(needle, MAX_HITS_PER_PAGE)?
            .into_iter()
            .map(|q| mupdf::Rect::from(q).into())
            .collect())
    }

    pub fn words_in_rect(&self, page: i32, area: PdfRect) -> Result<Vec<Word>> {
        Ok(self
            .load(page)?
            .words(TextExtractOptions::default())?
            .into_iter()
            .map(|w| Word {
                text: w.text,
                rect: w.bounds.into(),
            })
            .filter(|w| w.rect.intersects(&area))
            .collect())
    }
}

/// Máximo de áreas numa seleção (linhas longas com muitos pedaços).
const MAX_SELECTION_QUADS: usize = 1024;

impl PdfDoc {
    /// Seleção pelo fluxo do texto entre os pontos `a` e `b` (coordenadas da página), como o
    /// Acrobat: linhas inteiras entre a linha de `a` e a de `b`.
    pub fn select_text(&self, page: i32, a: (f32, f32), b: (f32, f32)) -> Result<TextSelection> {
        let page = self.load(page)?;
        let mut tp = page.to_text_page(TextPageFlags::empty())?;
        let buf = vec![Rect::new(0.0, 0.0, 0.0, 0.0).quad(); MAX_SELECTION_QUADS];
        let n = tp.highlight_selection(Point::new(a.0, a.1), Point::new(b.0, b.1), &buf)?;
        let quads: Vec<PdfRect> = buf[..usize::try_from(n).unwrap_or(0).min(buf.len())]
            .iter()
            .map(|q| Rect::from(q.clone()).into())
            .collect();
        let inside = |x: f32, y: f32| {
            quads
                .iter()
                .any(|r| x >= r.x0 - 0.5 && x <= r.x1 + 0.5 && y >= r.y0 - 0.5 && y <= r.y1 + 0.5)
        };
        let mut text = String::new();
        for block in tp.blocks() {
            for line in block.lines() {
                let before = text.len();
                for ch in line.chars() {
                    let r = Rect::from(ch.quad());
                    if let (Some(c), true) =
                        (ch.char(), inside((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0))
                    {
                        text.push(c);
                    }
                }
                if text.len() > before {
                    text.push('\n');
                }
            }
        }
        Ok(TextSelection { quads, text })
    }

    pub fn properties(&self) -> Result<DocProperties> {
        let meta = |name| self.inner.metadata(name).unwrap_or_default();
        Ok(DocProperties {
            path: self.path.clone(),
            title: meta(MetadataName::Title),
            author: meta(MetadataName::Author),
            subject: meta(MetadataName::Subject),
            keywords: meta(MetadataName::Keywords),
            creator: meta(MetadataName::Creator),
            producer: meta(MetadataName::Producer),
            created: meta(MetadataName::CreationDate),
            modified: meta(MetadataName::ModDate),
            format: meta(MetadataName::Format),
            encrypted: self.password.is_some()
                || !matches!(meta(MetadataName::Encryption).as_str(), "" | "None"),
            pages: self.page_count()?,
            first_page: self.page_size(0)?,
            file_size: std::fs::metadata(&self.path).map_or(0, |m| m.len()),
        })
    }

    /// Cópia para imprimir, gravada sem criptografia (o CUPS precisa ler): o documento em memória
    /// (com as edições; as apagadas, escondidas, não aparecem) ou, só "Documento", o arquivo do
    /// disco sem nenhuma anotação. O documento aberto não muda.
    pub fn export_for_print(&self, dest: &Path, with_annots: bool) -> Result<()> {
        let written = self.write_print_copy(dest, with_annots);
        if written.is_err() {
            // nada de cópia sem senha esquecida no disco (nem pela metade, nem de tentativa antiga)
            let _ = std::fs::remove_file(dest);
        }
        written
    }

    fn write_print_copy(&self, dest: &Path, with_annots: bool) -> Result<()> {
        // A cópia é sempre um documento à parte: gravar o documento aberto sem criptografia faria o
        // MuPDF tirar o /Encrypt dele, e o próximo Salvar estragaria o arquivo protegido.
        // Sempre do documento em memória: os campos de formulário saem com os valores atuais.
        let mut bytes = Vec::new();
        let mut keep = PdfWriteOptions::default();
        keep.set_encryption(Encryption::Keep);
        self.inner.write_to_with_options(&mut bytes, keep)?;
        let mut copy = PdfDocument::from_bytes(&bytes)?;
        if let Some(p) = self.password.as_deref() {
            copy.authenticate(p)?;
        }
        if !with_annots {
            // "Documento": sem comentários; os widgets (campos) não entram nessa lista e ficam
            for p in 0..copy.page_count()? {
                let mut page = copy.load_pdf_page(p)?;
                let all: Vec<_> = page.annotations().collect();
                for a in all {
                    page.delete_annotation(a)?;
                }
                page.update()?;
            }
        }
        let dest = utf8_path(dest)?;
        // o padrão do MuPDF é manter a criptografia; o CUPS precisa de uma cópia legível
        let mut opts = PdfWriteOptions::default();
        opts.set_encryption(Encryption::None);
        copy.save_with_options(dest, opts)?;
        Ok(())
    }

    /// Texto do documento inteiro (Selecionar tudo), página após página.
    pub fn document_text(&self) -> Result<String> {
        let mut out = String::new();
        for p in 0..self.page_count()? {
            out.push_str(
                &self
                    .load(p)?
                    .to_text_page(TextPageFlags::empty())?
                    .to_text()?,
            );
            out.push('\n');
        }
        Ok(out)
    }
}

fn size_of(page: &mupdf::Page) -> Result<PageSize> {
    let b = page.bounds()?;
    Ok(PageSize {
        width: b.x1 - b.x0,
        height: b.y1 - b.y0,
    })
}

fn quads(rects: &[PdfRect]) -> Result<Vec<Quad>> {
    if rects.is_empty() {
        return Err(PdfError::InvalidAnnotation("sem área para marcar".into()));
    }
    Ok(rects.iter().map(|r| Rect::from(*r).quad()).collect())
}

fn add_one(doc: &mut PdfDocument, page: &mut PdfPage, a: &NewAnnot) -> Result<PdfAnnotation> {
    Ok(match a {
        NewAnnot::Highlight { rects, .. } => page.add_highlight_annotation(quads(rects)?)?,
        NewAnnot::Underline { rects, .. } => page.add_underline_annotation(quads(rects)?)?,
        NewAnnot::StrikeOut { rects, .. } => page.add_strikeout_annotation(quads(rects)?)?,
        NewAnnot::Note { at, text, .. } => {
            page.add_text_annotation(Rect::new(at.0, at.1, at.0 + 24.0, at.1 + 24.0), text)?
        }
        NewAnnot::Ink { strokes, .. } => {
            page.add_ink_annotation(annot_edit::ink_points(strokes))?
        }
        NewAnnot::TextBox {
            rect,
            text,
            size,
            color,
            ..
        } => {
            let rect = text_fit::fitted(*rect, text, *size, 1.0)?;
            let mut a = page.add_free_text_annotation(Rect::from(rect), text)?;
            // na caixa de texto o MuPDF desenha a borda com a cor do texto; /C é o fundo
            a.set_default_appearance("Helv", *size, Some(rgb(*color)))?;
            a.set_border_width(1.0)?;
            a
        }
        NewAnnot::Shape { rect, ellipse, .. } => outlined(if *ellipse {
            page.add_circle_annotation(Rect::from(*rect))?
        } else {
            page.add_square_annotation(Rect::from(*rect))?
        })?,
        NewAnnot::Line {
            a: p, b: q, arrow, ..
        } => {
            let mut a =
                outlined(page.add_line_annotation(Point::new(p.0, p.1), Point::new(q.0, q.1))?)?;
            if *arrow {
                a.set_line_ending_styles(LineEndingStyle::None, LineEndingStyle::OpenArrow)?;
            }
            a
        }
        NewAnnot::FillText { at, text, size, .. } => {
            let start = PdfRect::new(at.0, at.1, at.0 + *size, at.1 + *size);
            let rect = text_fit::fitted(start, text, *size, 0.0)?;
            let mut a = page.add_free_text_annotation(Rect::from(rect), text)?;
            a.set_default_appearance("Helv", *size, Some(rgb([0.0; 3])))?;
            a.set_border_width(0.0)?;
            // o "Typewriter" do Acrobat: texto solto, sem caixa
            a.object()
                .dict_put("IT", PdfObject::new_name("FreeTextTypeWriter")?)?;
            a
        }
        NewAnnot::Mark { rect, mark, .. } => fill_sign_mark(page, *rect, *mark)?,
        NewAnnot::Signature {
            rect,
            art,
            initials,
            ..
        } => crate::signature::add(doc, page, *rect, art, *initials)?,
        NewAnnot::Stamp {
            rect,
            title,
            subtitle,
            color,
            ..
        } => {
            let img = stamp::image(title, subtitle.as_deref(), *color, page.rotation()?)?;
            add_stamp(doc, page, *rect, title, &img)?
        }
    })
}

fn utf8_path(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))
}

/// Carimbo cuja aparência é a imagem `img` (o MuPDF a ajusta ao retângulo ao mover/redimensionar).
fn add_stamp(
    doc: &mut PdfDocument,
    page: &mut PdfPage,
    rect: PdfRect,
    title: &str,
    img: &mupdf::Image,
) -> Result<PdfAnnotation> {
    let image_ref = doc.add_image(img)?;
    let annot = page.add_stamp_annotation(Rect::from(rect), title)?;
    let mut xobjects = doc.new_dict()?;
    xobjects.dict_put("I", image_ref)?;
    let mut resources = doc.new_dict()?;
    resources.dict_put("XObject", xobjects)?;
    let mut normal = doc.new_dict()?;
    normal.dict_put("Resources", resources)?;
    let mut ap = doc.new_dict()?;
    ap.dict_put("N", normal)?;
    annot.object().dict_put("AP", ap)?;
    Ok(annot)
}

/// Agora, no formato de data do PDF: `D:AAAAMMDDhhmmss+hh'mm'` (hora local).
fn pdf_now() -> String {
    let now = chrono::Local::now();
    let offset = now.offset().local_minus_utc() / 60;
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.abs();
    format!(
        "{}{sign}{:02}'{:02}'",
        now.format("D:%Y%m%d%H%M%S"),
        offset / 60,
        offset % 60
    )
}

/// Respostas e status de `root`, inclusive respostas de respostas (em qualquer profundidade).
fn descendants(annots: &[AnnotInfo], root: i32) -> Vec<i32> {
    let mut found = vec![root];
    let mut n = 0;
    while n < found.len() {
        let parent = found[n];
        found.extend(
            annots
                .iter()
                .filter(|a| a.reply_to == Some(parent) && !found.contains(&a.id))
                .map(|a| a.id)
                .collect::<Vec<_>>(),
        );
        n += 1;
    }
    found.remove(0);
    found
}

/// Junta à edição o crescimento da caixa de texto, quando `fit` = (atual, necessário) difere.
fn with_fit(edit: Edit, page: i32, id: i32, fit: Option<(PdfRect, PdfRect)>) -> Edit {
    match fit {
        Some((now, need)) if need != now => Edit::Batch(vec![
            edit,
            Edit::Geometry {
                page,
                id,
                rect: need,
                ink: None,
            },
        ]),
        _ => edit,
    }
}

/// Cor RGB da interface no formato do MuPDF (o inverso de `rgb_of`).
pub(crate) fn rgb([red, green, blue]: [f32; 3]) -> AnnotationColor {
    AnnotationColor::Rgb { red, green, blue }
}

/// Cor RGB pedida pela interface como edição; `None` vira `absent`.
fn rgb_edit(color: Option<[f32; 3]>, absent: ColorEdit) -> ColorEdit {
    color.map_or(absent, |c| ColorEdit::Set(rgb(c)))
}

/// Contorno padrão das formas e linhas: vermelho, 1 pt (como no Acrobat).
/// Marca preta: Xis, Visto e Linha viram tinta; Círculo e Ponto, círculo (o Ponto, cheio).
fn fill_sign_mark(page: &mut PdfPage, r: PdfRect, mark: Mark) -> Result<PdfAnnotation> {
    let black = rgb([0.0; 3]);
    let width = (r.height().min(r.width()) / 8.0).clamp(0.75, 4.0);
    // pontos relativos (0..1, y para baixo), recuados meia espessura para a tinta caber na caixa
    let inset = width / 2.0;
    let at = |fx: f32, fy: f32| {
        (
            r.x0 + inset + fx * (r.width() - 2.0 * inset),
            r.y0 + inset + fy * (r.height() - 2.0 * inset),
        )
    };
    let strokes = match mark {
        Mark::Cross => vec![
            vec![at(0.0, 0.0), at(1.0, 1.0)],
            vec![at(1.0, 0.0), at(0.0, 1.0)],
        ],
        Mark::Check => vec![vec![at(0.0, 0.55), at(0.35, 0.95), at(1.0, 0.05)]],
        Mark::Line => vec![vec![at(0.0, 0.5), at(1.0, 0.5)]],
        Mark::Circle | Mark::Dot => {
            let mut a = page.add_circle_annotation(Rect::from(r))?;
            a.set_color(black)?;
            a.set_border_width(if mark == Mark::Dot { 0.0 } else { width })?;
            if mark == Mark::Dot {
                a.set_interior_color(black)?;
            }
            return Ok(a);
        }
    };
    let mut a = page.add_ink_annotation(annot_edit::ink_points(&strokes))?;
    a.set_color(black)?;
    a.set_border_width(width)?;
    Ok(a)
}

fn outlined(mut a: PdfAnnotation) -> Result<PdfAnnotation> {
    a.set_color(rgb(COMMENT_RED))?;
    a.set_border_width(1.0)?;
    Ok(a)
}

fn kind_of(t: PdfAnnotationType) -> AnnotKind {
    match t {
        PdfAnnotationType::Highlight => AnnotKind::Highlight,
        PdfAnnotationType::Underline => AnnotKind::Underline,
        PdfAnnotationType::Text => AnnotKind::Note,
        PdfAnnotationType::Ink => AnnotKind::Ink,
        PdfAnnotationType::StrikeOut => AnnotKind::StrikeOut,
        PdfAnnotationType::FreeText => AnnotKind::TextBox,
        PdfAnnotationType::Square => AnnotKind::Rect,
        PdfAnnotationType::Circle => AnnotKind::Ellipse,
        PdfAnnotationType::Line => AnnotKind::Line,
        PdfAnnotationType::Stamp => AnnotKind::Stamp,
        _ => AnnotKind::Other,
    }
}

/// Cor da anotação em RGB (cinza e CMYK convertidos de forma simples).
pub(crate) fn rgb_of(c: AnnotationColor) -> [f32; 3] {
    match c {
        AnnotationColor::Gray(g) => [g, g, g],
        AnnotationColor::Rgb { red, green, blue } => [red, green, blue],
        AnnotationColor::Cmyk {
            cyan,
            magenta,
            yellow,
            key,
        } => [cyan, magenta, yellow].map(|v| 1.0 - (v + key).min(1.0)),
    }
}

/// Número do objeto PDF da anotação (identidade estável).
pub(crate) fn annot_id(annot: &PdfAnnotation) -> Result<i32> {
    Ok(annot.object().as_indirect()?)
}

pub(crate) fn annot_info(annot: &PdfAnnotation, page: i32) -> Result<AnnotInfo> {
    let kind = kind_of(annot.r#type()?);
    let font = (kind == AnnotKind::TextBox)
        .then(|| annot.default_appearance())
        .transpose()?
        .flatten()
        .map(|da| (da.size, da.color.map(rgb_of).unwrap_or([0.0; 3])));
    let own_color = annot.color()?.map(rgb_of);
    Ok(AnnotInfo {
        id: annot_id(annot)?,
        kind,
        page,
        rect: annot.bounds()?.into(),
        contents: annot.contents()?.unwrap_or_default().to_owned(),
        // caixa de texto: a cor é a do texto e da borda; /C é o fundo
        color: match kind {
            AnnotKind::TextBox => font.map(|f| f.1),
            _ => own_color,
        },
        opacity: annot.opacity()?,
        width: kind
            .has_border()
            .then(|| annot.border_width())
            .transpose()?,
        fill: match kind {
            AnnotKind::TextBox => own_color,
            k if k.has_fill() => annot.interior_color()?.map(rgb_of),
            _ => None,
        },
        line: (kind == AnnotKind::Line)
            .then(|| annot.line())
            .transpose()?
            .map(|(p, q)| ((p.x, p.y), (q.x, q.y))),
        font,
        arrow: kind == AnnotKind::Line && annot.line_ending_styles()?.1 != LineEndingStyle::None,
        author: annot.author()?.unwrap_or_default().to_owned(),
        reply_to: reply_of(annot)?,
        state: match annot_edit::name_entry(annot, "State")? {
            Some(state) => {
                let model = match annot_edit::name_entry(annot, "StateModel")?.as_deref() {
                    Some("Marked") => StateModel::Marked,
                    _ => StateModel::Review,
                };
                Some((model, state))
            }
            None => None,
        },
        created: annot_edit::string_entry(annot, "CreationDate")?.unwrap_or_default(),
        modified: annot_edit::string_entry(annot, "M")?.unwrap_or_default(),
    })
}

/// Apaga de `doc` as anotações (página, id) de `hidden`.
pub(crate) fn purge(doc: &PdfDocument, hidden: &HashSet<(i32, i32)>) -> Result<()> {
    for &(page, id) in hidden {
        if let Ok((mut page_ref, a)) = annot_edit::find(doc, page, id) {
            page_ref.delete_annotation(a)?;
            page_ref.update()?;
        }
    }
    Ok(())
}

/// Documento a partir de bytes em memória, autenticado com a senha (se tiver).
pub(crate) fn reopen(bytes: &[u8], password: Option<&str>) -> Result<PdfDocument> {
    let mut doc = PdfDocument::from_bytes(bytes)?;
    if let Some(p) = password {
        if doc.needs_password()? && !doc.authenticate(p)? {
            return Err(PdfError::WrongPassword);
        }
    }
    Ok(doc)
}

/// Grava `doc` em `dest` com segurança: arquivo temporário ao lado, fsync e rename. Os objetos
/// sem uso saem (garbage nível 1, sem renumerar). Em erro, nada fica pela metade.
pub(crate) fn write_atomic(
    doc: &PdfDocument,
    dest: &Path,
    mut opts: PdfWriteOptions,
) -> Result<()> {
    let tmp = temp_path(dest)?;
    let tmp_str = utf8_path(&tmp)?;
    opts.set_garbage_level(1);
    let written = doc
        .save_with_options(tmp_str, opts)
        .map_err(PdfError::from)
        // os dados precisam estar no disco antes do rename: numa queda de energia o original
        // nunca pode ser trocado por um arquivo truncado
        .and_then(|()| {
            std::fs::File::open(&tmp)?
                .sync_all()
                .map_err(PdfError::from)
        })
        .and_then(|()| std::fs::rename(&tmp, dest).map_err(PdfError::from));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

fn temp_path(dest: &Path) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .ok_or_else(|| PdfError::Engine("caminho de destino sem nome de arquivo".into()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".gera-tmp");
    Ok(dest.with_file_name(tmp_name))
}

impl PdfDoc {
    pub fn annotations(&self, page: i32) -> Result<Vec<AnnotInfo>> {
        self.check_page(page)?;
        let page_ref = self.inner.load_pdf_page(page)?;
        let mut out = Vec::new();
        for mut annot in page_ref.annotations() {
            // campos de formulário não são comentários (ficam em `fields`)
            if annot.r#type()? == PdfAnnotationType::Widget {
                continue;
            }
            if self.history.hidden.contains(&(page, annot_id(&annot)?)) {
                continue; // apagada por nós (sai do arquivo ao salvar)
            }
            let flags = annot.flags()?;
            if reply_of(&annot)?.is_some() {
                // resposta/status: vai para o painel Comentários; na página não aparece (o Acrobat
                // as esconde pelo /IRT; o MuPDF só respeita as flags)
                if !flags.intersects(AnnotationFlags::IS_HIDDEN | AnnotationFlags::NO_VIEW) {
                    annot.set_flags(flags | AnnotationFlags::NO_VIEW)?;
                }
            } else if flags.contains(AnnotationFlags::IS_HIDDEN) {
                continue; // escondida de fábrica: invisível, fora da lista
            }
            out.push(annot_info(&annot, page)?);
        }
        Ok(out)
    }

    pub fn all_annotations(&self) -> Result<Vec<AnnotInfo>> {
        let mut all = Vec::new();
        for p in 0..self.page_count()? {
            all.extend(self.annotations(p)?);
        }
        Ok(all)
    }

    /// Cria as anotações `pending` e grava (atalho dos testes). Em qualquer erro descarta as
    /// mudanças em memória (relê do disco) e devolve o erro.
    pub fn save_with(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()> {
        let result = pending
            .iter()
            .try_for_each(|a| self.apply(AnnotOp::Create(a.clone())).map(drop))
            .and_then(|()| self.save(dest));
        if result.is_err() {
            self.discard_changes()?;
        }
        result
    }

    /// Grava o documento em memória em `dest`: as anotações escondidas por nós (apagadas) saem de
    /// verdade; o histórico recomeça. Em erro, as edições continuam em memória (só o histórico se
    /// perde, porque as apagadas já saíram) e o documento segue com alterações a salvar.
    pub fn save(&mut self, dest: &Path) -> Result<()> {
        let result = self.purge_hidden().and_then(|()| self.write(dest));
        self.unsaved = result.is_err() && (self.unsaved || self.history.can_undo());
        self.history = History::default();
        result
    }

    /// Volta ao arquivo do disco (com a mesma senha), sem as edições em memória.
    fn discard_changes(&mut self) -> Result<()> {
        self.inner = open_authenticated(&self.path, self.password.as_deref())?;
        self.history = History::default();
        self.unsaved = false;
        Ok(())
    }

    pub(crate) fn purge_hidden(&mut self) -> Result<()> {
        purge(&self.inner, &self.history.hidden)
    }

    fn write(&mut self, dest: &Path) -> Result<()> {
        let mut opts = PdfWriteOptions::default();
        opts.set_encryption(Encryption::Keep);
        write_atomic(&self.inner, dest, opts)?;
        self.path = dest.to_path_buf();
        Ok(())
    }

    /// Aplica uma operação do usuário no documento em memória; devolve as páginas mudadas.
    pub fn apply(&mut self, op: AnnotOp) -> Result<Vec<i32>> {
        let edit = match op {
            AnnotOp::Create(a) => {
                let page = a.page();
                self.check_page(page)?;
                if a.is_empty() {
                    return Err(PdfError::InvalidAnnotation("anotação vazia".into()));
                }
                let mut page_ref = self.inner.load_pdf_page(page)?;
                let mut annot = add_one(&mut self.inner, &mut page_ref, &a)?;
                self.sign(&mut annot)?;
                annot.update()?;
                page_ref.update()?;
                let id = annot_id(&annot)?;
                self.history.record(Edit::Visibility {
                    page,
                    id,
                    hidden: true,
                });
                return Ok(vec![page]);
            }
            AnnotOp::Move { page, id, dx, dy } => {
                if let Some((a, b)) = annot_edit::line_of(&self.inner, page, id)? {
                    let shift = |p: (f32, f32)| (p.0 + dx, p.1 + dy);
                    Edit::LinePoints {
                        page,
                        id,
                        a: shift(a),
                        b: shift(b),
                    }
                } else {
                    let (rect, ink) = annot_edit::geometry_of(&self.inner, page, id)?;
                    let (rect, ink) = annot_edit::moved(rect, ink, dx, dy);
                    Edit::Geometry {
                        page,
                        id,
                        rect,
                        ink,
                    }
                }
            }
            AnnotOp::MoveLinePoint { page, id, end, to } => {
                let (a, b) = annot_edit::line_of(&self.inner, page, id)?
                    .ok_or_else(|| PdfError::InvalidAnnotation("não é uma linha".into()))?;
                let (a, b) = match end {
                    LineEnd::Start => (to, b),
                    LineEnd::End => (a, to),
                };
                Edit::LinePoints { page, id, a, b }
            }
            AnnotOp::Fill { page, id, color } => Edit::Fill {
                page,
                id,
                color: rgb_edit(color, ColorEdit::Clear),
            },
            AnnotOp::Font {
                page,
                id,
                size,
                color,
            } => {
                let size = size.clamp(4.0, 144.0);
                let fit = match annot_edit::text_box_of(&self.inner, page, id)? {
                    Some((rect, text, _, border)) => {
                        Some((rect, text_fit::fitted(rect, &text, size, border)?))
                    }
                    None => None,
                };
                with_fit(
                    Edit::Font {
                        page,
                        id,
                        size,
                        color,
                    },
                    page,
                    id,
                    fit,
                )
            }
            AnnotOp::Resize { page, id, rect } => {
                let (old, ink) = annot_edit::geometry_of(&self.inner, page, id)?;
                let ink = annot_edit::scaled(old, rect, ink);
                Edit::Geometry {
                    page,
                    id,
                    rect,
                    ink,
                }
            }
            AnnotOp::Style {
                page,
                id,
                color,
                opacity,
                width,
            } => Edit::Style {
                page,
                id,
                color: rgb_edit(color, ColorEdit::Keep),
                opacity,
                width,
            },
            AnnotOp::SetContents { page, id, text } => {
                let fit = match annot_edit::text_box_of(&self.inner, page, id)? {
                    Some((rect, _, size, border)) => {
                        Some((rect, text_fit::fitted(rect, &text, size, border)?))
                    }
                    None => None,
                };
                let contents = Edit::Contents {
                    page,
                    id,
                    text,
                    modified: Some(pdf_now()),
                };
                with_fit(contents, page, id, fit)
            }
            AnnotOp::Delete { page, id } => {
                // como no Acrobat, as respostas e os status vão junto com o comentário
                let hide = |id| Edit::Visibility {
                    page,
                    id,
                    hidden: true,
                };
                let children: Vec<Edit> = descendants(&self.annotations(page)?, id)
                    .into_iter()
                    .map(hide)
                    .collect();
                if children.is_empty() {
                    hide(id)
                } else {
                    Edit::Batch(std::iter::once(hide(id)).chain(children).collect())
                }
            }
            AnnotOp::Reply { page, parent, text } => {
                if text.trim().is_empty() {
                    return Err(PdfError::InvalidAnnotation("resposta vazia".into()));
                }
                return self.add_child(page, parent, &text, None);
            }
            AnnotOp::SetState {
                page,
                parent,
                model,
                state,
            } => return self.add_child(page, parent, "", Some((model, &state))),
        };
        let page = edit.page();
        let inverse = annot_edit::apply_edit(&self.inner, &mut self.history.hidden, edit)?;
        self.history.record(inverse);
        Ok(vec![page])
    }

    /// Desfaz a última operação; devolve as páginas mudadas (vazio se não havia o que desfazer).
    pub fn undo(&mut self) -> Result<Changed> {
        self.step(true)
    }

    pub fn redo(&mut self) -> Result<Changed> {
        self.step(false)
    }

    /// Desfaz (`back`) ou refaz um passo; o passo contrário vai para a outra pilha. Em erro,
    /// nada muda.
    fn step(&mut self, back: bool) -> Result<Changed> {
        let popped = if back {
            self.history.undo.pop()
        } else {
            self.history.redo.pop()
        };
        let Some(step) = popped else {
            return Ok(Changed::Pages(Vec::new()));
        };
        let result = match &step {
            Step::Annot(edit) => {
                let page = edit.page();
                annot_edit::apply_edit(&self.inner, &mut self.history.hidden, edit.clone())
                    .map(|inverse| (Step::Annot(inverse), Changed::Pages(vec![page])))
            }
            Step::Snapshot(snap) => self.snapshot().and_then(|now| {
                self.restore(snap)?;
                Ok((Step::Snapshot(now), Changed::Document))
            }),
            Step::Field { page, id, value } => {
                let (page, id) = (*page, *id);
                self.write_field(page, id, value).map(|old| {
                    (
                        Step::Field {
                            page,
                            id,
                            value: old,
                        },
                        Changed::Fields,
                    )
                })
            }
        };
        let (from, to) = if back {
            (&mut self.history.undo, &mut self.history.redo)
        } else {
            (&mut self.history.redo, &mut self.history.undo)
        };
        match result {
            Ok((other, changed)) => {
                to.push(other);
                Ok(changed)
            }
            Err(e) => {
                from.push(step);
                Err(e)
            }
        }
    }

    /// O documento em memória como está agora (com as apagadas pendentes), para desfazer.
    pub(crate) fn snapshot(&self) -> Result<Snapshot> {
        let mut bytes = Vec::new();
        let mut keep = PdfWriteOptions::default();
        keep.set_encryption(Encryption::Keep);
        self.inner.write_to_with_options(&mut bytes, keep)?;
        Ok(Snapshot {
            bytes,
            hidden: self.history.hidden.clone(),
        })
    }

    /// Volta o documento ao instantâneo (com a mesma senha).
    pub(crate) fn restore(&mut self, snap: &Snapshot) -> Result<()> {
        self.inner = reopen(&snap.bytes, self.password.as_deref())?;
        self.history.hidden = snap.hidden.clone();
        Ok(())
    }

    /// Texto de uma página.
    pub fn page_text(&self, page: i32) -> Result<String> {
        Ok(self
            .load(page)?
            .to_text_page(TextPageFlags::empty())?
            .to_text()?)
    }

    /// Rotação gravada na página (/Rotate), em graus.
    pub fn page_rotation(&self, page: i32) -> Result<i32> {
        self.check_page(page)?;
        Ok(self.inner.load_pdf_page(page)?.rotation()?)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Autor e datas de criação/modificação de uma anotação nova.
    fn sign(&self, annot: &mut PdfAnnotation) -> Result<()> {
        if let Some(name) = &self.author {
            annot.set_author(name)?;
        }
        let now = pdf_now();
        let mut obj = annot.object();
        obj.dict_put("CreationDate", PdfObject::new_string(&now)?)?;
        obj.dict_put("M", PdfObject::new_string(&now)?)?;
        Ok(())
    }

    /// Resposta (sem `state`) ou status do comentário `parent`: anotação Text oculta na página
    /// (NoView), ligada ao pai por /IRT, como o Acrobat grava. Desfazer a esconde.
    fn add_child(
        &mut self,
        page: i32,
        parent: i32,
        text: &str,
        state: Option<(StateModel, &str)>,
    ) -> Result<Vec<i32>> {
        self.check_page(page)?;
        let (_, parent_annot) = annot_edit::find(&self.inner, page, parent)?;
        let rect = parent_annot.rect()?;
        let mut page_ref = self.inner.load_pdf_page(page)?;
        let mut child = page_ref.add_text_annotation(rect, text)?;
        let mut obj = child.object();
        obj.dict_put("IRT", parent_annot.object())?;
        match state {
            None => obj.dict_put("RT", PdfObject::new_name("R")?)?,
            Some((model, name)) => {
                obj.dict_put("StateModel", PdfObject::new_name(model.pdf_name())?)?;
                obj.dict_put("State", PdfObject::new_name(name)?)?;
            }
        }
        // fora da tela e da impressão: a conversa fica no painel Comentários
        let flags = (child.flags()? | AnnotationFlags::NO_VIEW) - AnnotationFlags::IS_PRINT;
        child.set_flags(flags)?;
        self.sign(&mut child)?;
        child.update()?;
        page_ref.update()?;
        let id = annot_id(&child)?;
        self.history.record(Edit::Visibility {
            page,
            id,
            hidden: true,
        });
        Ok(vec![page])
    }

    /// Há o que salvar mesmo sem passos para desfazer (ex.: o desfazer não alcança mais o salvo).
    pub(crate) fn mark_unsaved(&mut self) {
        self.unsaved = true;
    }

    /// Autor das anotações criadas daqui em diante (`None`: sem /T).
    pub fn set_author(&mut self, author: Option<String>) {
        self.author = author.filter(|a| !a.trim().is_empty());
    }

    pub fn history_state(&self) -> HistoryState {
        HistoryState {
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            dirty: self.is_dirty(),
        }
    }

    /// Há edições desde a última gravação?
    pub fn is_dirty(&self) -> bool {
        self.unsaved || self.history.can_undo()
    }

    pub fn outline(&self) -> Result<Vec<OutlineItem>> {
        let mut out = Vec::new();
        flatten(&self.inner.outlines()?, 0, &mut out);
        Ok(out)
    }

    /// Índice e anotações corrompidos não impedem abrir o documento; por isso `unwrap_or_default`.
    pub fn summary(&self) -> Result<DocSummary> {
        let sizes = (0..self.page_count()?)
            .map(|p| self.page_size(p))
            .collect::<Result<Vec<_>>>()?;
        Ok(DocSummary {
            path: self.path.clone(),
            sizes,
            outline: self.outline().unwrap_or_default(),
            annots: self.all_annotations().unwrap_or_default(),
            fields: self.all_fields().unwrap_or_default(),
            form_notes: self.form_notes().unwrap_or_default(),
        })
    }
}

/// Destinos externos (URI) viram `page: None`; `OutlineItem` não guarda o URI.
fn flatten(items: &[mupdf::Outline], depth: usize, out: &mut Vec<OutlineItem>) {
    for it in items {
        out.push(OutlineItem {
            title: it.title.clone(),
            // destino que não resolve para página (ex.: página excluída): sem página
            page: it
                .dest
                .as_ref()
                .map(|d| d.loc.page_number as i32)
                .filter(|p| (0..i32::MAX).contains(p)),
            depth,
        });
        flatten(&it.down, depth + 1, out);
    }
}
