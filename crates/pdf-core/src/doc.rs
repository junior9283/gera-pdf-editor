use crate::error::{PdfError, Result};
use crate::types::{
    AnnotInfo, AnnotKind, DocProperties, DocSummary, NewAnnot, OutlineItem, PageSize, PdfRect,
    RenderedPage, Rotation, TextSelection, Word,
};
use mupdf::pdf::{
    AnnotationFlags, Encryption, PdfAnnotation, PdfAnnotationType, PdfDocument, PdfPage,
    PdfWriteOptions,
};
use mupdf::color::AnnotationColor;
use mupdf::{
    Colorspace, Matrix, MetadataName, Point, Quad, Rect, TextExtractOptions, TextPageFlags,
};
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
    /// Guardada só em memória, para reabrir o arquivo (ex.: descartar mudanças após falha ao salvar).
    pub(crate) password: Option<String>,
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
        })
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

    /// Cópia para imprimir: o arquivo do disco mais as pendentes (ou sem nenhuma anotação),
    /// gravada sem criptografia, porque o CUPS precisa ler. O documento aberto não muda.
    pub fn export_for_print(
        &self,
        pending: &[NewAnnot],
        dest: &Path,
        with_annots: bool,
    ) -> Result<()> {
        let written = self.write_print_copy(pending, dest, with_annots);
        if written.is_err() {
            // nada de cópia sem senha esquecida no disco (nem pela metade, nem de tentativa antiga)
            let _ = std::fs::remove_file(dest);
        }
        written
    }

    fn write_print_copy(&self, pending: &[NewAnnot], dest: &Path, with_annots: bool) -> Result<()> {
        let copy = open_authenticated(&self.path, self.password.as_deref())?;
        if with_annots {
            apply_pending(&copy, pending)?;
        } else {
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

fn add_one(page: &mut PdfPage, a: &NewAnnot) -> Result<()> {
    match a {
        NewAnnot::Highlight { rects, .. } => {
            page.add_highlight_annotation(quads(rects)?)?;
        }
        NewAnnot::Underline { rects, .. } => {
            page.add_underline_annotation(quads(rects)?)?;
        }
        NewAnnot::StrikeOut { rects, .. } => {
            page.add_strikeout_annotation(quads(rects)?)?;
        }
        NewAnnot::Note { at, text, .. } => {
            page.add_text_annotation(Rect::new(at.0, at.1, at.0 + 24.0, at.1 + 24.0), text)?;
        }
        NewAnnot::Ink { strokes, .. } => {
            page.add_ink_annotation(
                strokes
                    .iter()
                    .map(|s| s.iter().map(|&(x, y)| Point::new(x, y))),
            )?;
        }
    }
    Ok(())
}

/// Aplica as anotações pendentes às páginas de `doc` (recusa páginas fora do documento).
fn apply_pending(doc: &PdfDocument, pending: &[NewAnnot]) -> Result<()> {
    let count = doc.page_count()?;
    if let Some(bad) = pending
        .iter()
        .map(NewAnnot::page)
        .find(|p| *p < 0 || *p >= count)
    {
        return Err(PdfError::PageOutOfRange(bad));
    }
    let mut pages: Vec<i32> = pending.iter().map(NewAnnot::page).collect();
    pages.sort_unstable();
    pages.dedup();
    for p in pages {
        let mut page = doc.load_pdf_page(p)?;
        for a in pending.iter().filter(|a| a.page() == p) {
            add_one(&mut page, a)?;
        }
        page.update()?;
    }
    Ok(())
}

fn utf8_path(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))
}

fn kind_of(t: PdfAnnotationType) -> AnnotKind {
    match t {
        PdfAnnotationType::Highlight => AnnotKind::Highlight,
        PdfAnnotationType::Underline => AnnotKind::Underline,
        PdfAnnotationType::Text => AnnotKind::Note,
        PdfAnnotationType::Ink => AnnotKind::Ink,
        PdfAnnotationType::StrikeOut => AnnotKind::StrikeOut,
        _ => AnnotKind::Other,
    }
}

/// Cor da anotação em RGB (cinza e CMYK convertidos de forma simples).
fn rgb_of(c: AnnotationColor) -> [f32; 3] {
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
    Ok(AnnotInfo {
        id: annot_id(annot)?,
        kind,
        page,
        rect: annot.bounds()?.into(),
        contents: annot.contents()?.unwrap_or_default().to_owned(),
        color: annot.color()?.map(rgb_of),
        opacity: annot.opacity()?,
        width: (kind == AnnotKind::Ink)
            .then(|| annot.border_width())
            .transpose()?,
    })
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
        for annot in page_ref.annotations() {
            if annot.flags()?.contains(AnnotationFlags::IS_HIDDEN) {
                continue; // apagada (até salvar) ou escondida de fábrica: invisível, fora da lista
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

    /// Aplica `pending`, grava em arquivo temporário ao lado de `dest` e renomeia.
    /// Em qualquer erro descarta as mudanças em memória (recarrega do disco) e devolve o erro.
    pub fn save_with(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()> {
        let result = self.apply_and_write(pending, dest);
        if result.is_err() {
            // descarta as mudanças em memória reabrindo o arquivo (com a mesma senha, se houver)
            self.inner = open_authenticated(&self.path, self.password.as_deref())?;
        }
        result
    }

    fn apply_and_write(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()> {
        apply_pending(&self.inner, pending)?;
        let tmp = temp_path(dest)?;
        let tmp_str = utf8_path(&tmp)?;
        let mut opts = PdfWriteOptions::default();
        opts.set_encryption(Encryption::Keep);
        let written = self
            .inner
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
        written?;
        self.path = dest.to_path_buf();
        Ok(())
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
        })
    }
}

/// Destinos externos (URI) viram `page: None`; `OutlineItem` não guarda o URI.
fn flatten(items: &[mupdf::Outline], depth: usize, out: &mut Vec<OutlineItem>) {
    for it in items {
        out.push(OutlineItem {
            title: it.title.clone(),
            page: it.dest.as_ref().map(|d| d.loc.page_number as i32),
            depth,
        });
        flatten(&it.down, depth + 1, out);
    }
}
