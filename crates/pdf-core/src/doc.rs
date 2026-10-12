use crate::annot_edit::{
    self, reply_of, ColorEdit, Edit, History, Snapshot, Step, SNAPSHOT_BUDGET,
};
use crate::error::{refused, PdfError, Result};
use crate::stamp;
use crate::text_fit;
use crate::types::{
    AnnotInfo, AnnotKind, AnnotOp, Changed, DocProperties, DocSummary, HistoryState, LineEnd, Mark,
    NewAnnot, OutlineItem, PageSize, PdfRect, RenderedPage, Rotation, StateModel, TextSelection,
    Word, COMMENT_RED, TEXT_EDIT_BLUE,
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
    /// O arquivo do disco tem assinaturas: gravar e tirar instantâneos só de forma incremental
    /// (a memória pode ter perdido o /V ou a página assinada, o arquivo não).
    pub(crate) signed_on_disk: bool,
    /// A senha de permissões foi informada (todas as permissões liberadas).
    pub(crate) owner_unlocked: bool,
    /// Proteção a aplicar no próximo Salvar (`Some(None)`: remover a segurança).
    pub(crate) pending_protection: Option<Option<crate::security::Protection>>,
    /// Números dos objetos das fontes que o Editar texto acrescentou nesta sessão: só elas são
    /// reaproveitadas; nada do arquivo é confiado como "fonte do app".
    pub(crate) added_fonts: Vec<i32>,
    /// Olho do painel Camadas: as camadas que o usuário ligou ou desligou (só a vista).
    pub(crate) layer_view: std::collections::BTreeMap<i32, bool>,
    /// O `/D` das camadas do arquivo, guardado enquanto o da memória mostra o olho.
    pub(crate) layer_default: Option<crate::layers::LayerDefault>,
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

type AnnotsLinksFiles = (
    Vec<AnnotInfo>,
    Vec<crate::LinkInfo>,
    Vec<crate::AttachmentInfo>,
);

impl PdfDoc {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let inner = open_authenticated(path, password)?;
        let mut doc = Self {
            inner,
            path: path.to_path_buf(),
            password: password.map(str::to_owned),
            history: History::default(),
            unsaved: false,
            author: None,
            signed_on_disk: false,
            owner_unlocked: false,
            pending_protection: None,
            added_fonts: Vec::new(),
            layer_view: Default::default(),
            layer_default: None,
        };
        doc.signed_on_disk = doc.has_signatures().unwrap_or(false);
        // códigos de barras: o MuPDF os mostraria como texto (não marca o documento alterado)
        let _ = doc.draw_barcodes();
        // abrir digitando a senha de permissões libera tudo (como no Acrobat)
        if let Some(p) = password.filter(|p| !p.is_empty()) {
            doc.owner_unlocked = doc.unlock(p).is_ok();
        }
        Ok(doc)
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

    /// Executa `f` com instantâneo no histórico (desfazer volta tudo); se falhar, o documento volta
    /// como estava.
    pub(crate) fn with_snapshot<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let before = self.snapshot()?;
        match f(self) {
            Ok(v) => {
                if self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
                    self.mark_unsaved();
                }
                Ok(v)
            }
            Err(e) => {
                self.restore(&before)?;
                Err(e)
            }
        }
    }

    /// Roda `edit` na página com as recusas de alteração; em erro o documento volta como
    /// estava, e em sucesso o desfazer ganha o instantâneo de antes.
    pub(crate) fn guarded_page_edit(
        &mut self,
        page: i32,
        action: &str,
        edit: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        self.require_modifiable(action)?;
        self.check_page(page)?;
        self.with_snapshot(edit)
    }

    /// Recusa alterar o conteúdo de um documento assinado (`action` invalidaria as assinaturas)
    /// ou que não permite alterações.
    pub(crate) fn require_modifiable(&self, action: &str) -> Result<()> {
        if self.signed_on_disk || self.has_signatures()? {
            return Err(refused(format!(
                "o documento está assinado: {action} alteraria as assinaturas"
            )));
        }
        if !self.security()?.permissions.modify {
            return Err(refused("o documento não permite alterações"));
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
        Ok(packed_rgb(&pix, invert))
    }

    /// Ladrilho `tile` (coluna, linha) de lado `size` px da página renderizada em `scale`, com a
    /// vista girada. Serve para zooms acima do teto da página inteira: só o recorte é alocado. Os da
    /// borda saem cortados no tamanho da página.
    pub fn render_tile(
        &self,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        tile: (u32, u32),
        size: u32,
    ) -> Result<RenderedPage> {
        let page = self.load(page)?;
        if !scale.is_finite()
            || scale <= 0.0
            || scale > MAX_TILE_SCALE
            || !(1..=4096).contains(&size)
        {
            return Err(PdfError::InvalidScale(scale));
        }
        let mut ctm = Matrix::new_rotate(f32::from(rotation.degrees()));
        ctm.scale(scale, scale);
        let full = page.bounds()?.transform(&ctm).round();
        let x0 = i64::from(full.x0) + i64::from(tile.0) * i64::from(size);
        let y0 = i64::from(full.y0) + i64::from(tile.1) * i64::from(size);
        let x1 = (x0 + i64::from(size)).min(i64::from(full.x1));
        let y1 = (y0 + i64::from(size)).min(i64::from(full.y1));
        if x0 >= x1 || y0 >= y1 {
            return Err(PdfError::InvalidScale(scale)); // fora da página
        }
        let rect = mupdf::IRect::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32);
        let mut pix = mupdf::Pixmap::new_with_rect(&Colorspace::device_rgb(), rect, false)?;
        pix.clear_with(255)?;
        {
            let device = mupdf::Device::from_pixmap(&pix)?;
            page.run(&device, &ctm)?;
        }
        Ok(packed_rgb(&pix, invert))
    }

    /// Caixa da tinta da página (sem anotações), em coordenadas do MuPDF, para "Ajustar ao
    /// conteúdo visível". `None` numa página em branco.
    pub fn content_box(&self, page: i32) -> Result<Option<PdfRect>> {
        let page = self.load(page)?;
        let bounds = page.bounds()?;
        // 0,25, mas o lado maior nunca passa de 2048 px (página gigante, /UserUnit)
        let side = (bounds.x1 - bounds.x0).max(bounds.y1 - bounds.y0).max(1.0);
        let scale = 0.25f32.min(2048.0 / side);
        if !scale.is_finite() || scale <= 0.0 {
            return Ok(None);
        }
        let ctm = Matrix::new_scale(scale, scale);
        let pix = page.to_pixmap(&ctm, &Colorspace::device_rgb(), false, false)?;
        let (w, h, stride) = (
            pix.width() as usize,
            pix.height() as usize,
            pix.stride() as usize,
        );
        // direto nas amostras (sem copiar o pixmap): limites da tinta
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for (y, row) in pix.samples().chunks(stride).take(h).enumerate() {
            for (x, px) in row[..w * 3].chunks(3).enumerate() {
                if px.iter().any(|&c| c < 250) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        if x0 >= x1 {
            return Ok(None);
        }
        let to_pt = |v: usize, origin: f32| v as f32 / scale + origin;
        Ok(Some(PdfRect::new(
            to_pt(x0, bounds.x0),
            to_pt(y0, bounds.y0),
            to_pt(x1, bounds.x0).min(bounds.x1),
            to_pt(y1, bounds.y0).min(bounds.y1),
        )))
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
        Ok(page_words(&self.load(page)?)?
            .into_iter()
            .filter(|w| w.rect.intersects(&area))
            .collect())
    }
}

/// As palavras da página, com o retângulo de cada uma.
pub(crate) fn page_words(page: &mupdf::Page) -> Result<Vec<Word>> {
    Ok(page
        .words(TextExtractOptions::default())?
        .into_iter()
        .map(|w| Word {
            text: w.text,
            rect: w.bounds.into(),
        })
        .collect())
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
            custom: self.custom_properties(),
            initial: self.initial_view(),
            window: self.window_options(),
            lang: self.language(),
            print: self.print_prefs(),
            fonts: self.fonts(),
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
        NewAnnot::Redact { rects, .. } => page.add_redact_annotation(quads(rects)?)?,
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
            let rect = text_fit::snug(*at, text, *size)?;
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
        NewAnnot::CustomStamp {
            rect, image, name, ..
        } => {
            let img = stamp::custom_image(image, page.rotation()?)?;
            add_stamp(doc, page, *rect, name, &img)?
        }
        NewAnnot::Caret { rect, text, .. } => {
            let mut a = page.add_caret_annotation(Rect::from(*rect))?;
            a.set_contents(text)?;
            a.set_color(rgb(TEXT_EDIT_BLUE))?;
            a
        }
        NewAnnot::Callout {
            rect,
            knee,
            tip,
            text,
            size,
            color,
            ..
        } => {
            let mut a = page.add_free_text_annotation(Rect::from(*rect), text)?;
            a.set_default_appearance("Helv", *size, Some(rgb(*color)))?;
            a.set_border_width(1.0)?;
            let ctm = page.ctm()?;
            annot_edit::set_callout(doc, &mut a, &ctm, *rect, *tip, *knee)?;
            a
        }
        NewAnnot::FileAttachment {
            at,
            path,
            description,
            ..
        } => {
            let (name, len) = crate::attachments::attachment_file(path)?;
            // lido direto para o buffer do MuPDF (sem cópia intermediária do arquivo inteiro)
            let mut buf = mupdf::Buffer::with_capacity(len as usize);
            std::io::copy(
                &mut std::io::Read::take(std::fs::File::open(path)?, len),
                &mut buf,
            )?;
            // o arquivo vai no comentário (/FS), não na lista de anexos do documento
            let mut params = doc.new_dict()?;
            params.dict_put(
                "Size",
                PdfObject::new_int(buf.len().min(i32::MAX as usize) as i32)?,
            )?;
            let mut ef_dict = doc.new_dict()?;
            ef_dict.dict_put("Type", PdfObject::new_name("EmbeddedFile")?)?;
            ef_dict.dict_put("Params", params)?;
            let stream = doc.add_stream(&buf, Some(&ef_dict), false)?;
            let mut ef = doc.new_dict()?;
            ef.dict_put("F", stream.clone())?;
            ef.dict_put("UF", stream)?;
            let mut fs = doc.new_dict()?;
            fs.dict_put("Type", PdfObject::new_name("Filespec")?)?;
            fs.dict_put("F", PdfObject::new_string(&name)?)?;
            fs.dict_put("UF", PdfObject::new_string(&name)?)?;
            fs.dict_put("EF", ef)?;
            let fs = doc.add_object(&fs)?;
            let mut a = page.create_annotation(PdfAnnotationType::FileAttachment)?;
            a.set_rect(Rect::new(at.0, at.1, at.0 + 14.0, at.1 + 20.0))?;
            a.object().dict_put("FS", fs)?;
            a.set_icon_name("Paperclip")?;
            a.set_contents(description)?;
            a.set_color(rgb(TEXT_EDIT_BLUE))?;
            a
        }
        NewAnnot::ReplaceText { .. } => {
            return Err(PdfError::InvalidAnnotation(
                "substituir texto cria duas anotações (replace_text)".into(),
            ))
        }
        NewAnnot::Polygon {
            vertices,
            closed,
            cloud,
            ..
        } => {
            let points = vertices
                .iter()
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .take(annot_edit::MAX_VERTICES)
                .map(|&(x, y)| Point::new(x, y));
            let mut a = outlined(if *closed || *cloud {
                page.add_polygon_annotation(points)?
            } else {
                page.add_polyline_annotation(points)?
            })?;
            if *cloud {
                // nuvem do Acrobat: polígono com borda de nuvem (/BE /S /C), intensidade 2
                a.set_border_effect(mupdf::pdf::AnnotationBorderEffect::Cloudy)?;
                a.set_border_effect_intensity(2.0)?;
            }
            a
        }
    })
}

pub(crate) fn utf8_path(path: &Path) -> Result<&str> {
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
pub(crate) fn pdf_now() -> String {
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

/// O grupo só vale quando aponta para outra anotação da página que não está, ela mesma, num
/// grupo: grupo para si mesmo, para fora da página ou em ciclo (de outros programas) vira `None`.
/// Assim o painel, a seleção e o núcleo leem `group` sem conferir de novo.
fn normalize_groups(annots: &mut [AnnotInfo]) {
    let raw: std::collections::HashMap<i32, Option<i32>> =
        annots.iter().map(|a| (a.id, a.group)).collect();
    for a in annots.iter_mut() {
        a.group = a
            .group
            .filter(|g| *g != a.id && raw.get(g).is_some_and(|pg| pg.is_none_or(|pg| pg == *g)));
    }
}

/// Principal do grupo de `id` (ou o próprio `id`).
fn group_root(annots: &[AnnotInfo], id: i32) -> i32 {
    annots
        .iter()
        .find(|a| a.id == id)
        .and_then(|a| a.group)
        .unwrap_or(id)
}

/// O principal e os membros do grupo.
fn group_members(annots: &[AnnotInfo], root: i32) -> Vec<i32> {
    std::iter::once(root)
        .chain(
            annots
                .iter()
                .filter(|a| a.group == Some(root) && a.id != root)
                .map(|a| a.id),
        )
        .collect()
}

/// Cada um de `ids` com o grupo e, como no Acrobat, as respostas e os status (o que sai ao
/// excluir e o que vai junto ao exportar).
pub(crate) fn with_group_and_replies(annots: &[AnnotInfo], ids: &[i32]) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::new();
    for &id in ids {
        for root in group_members(annots, group_root(annots, id)) {
            for k in std::iter::once(root).chain(descendants(annots, root)) {
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
    }
    out
}

fn hide_all(page: i32, ids: Vec<i32>) -> Edit {
    let mut edits: Vec<Edit> = ids
        .into_iter()
        .map(|id| Edit::Visibility {
            page,
            id,
            hidden: true,
        })
        .collect();
    if edits.len() == 1 {
        edits.remove(0)
    } else {
        Edit::Batch(edits)
    }
}

/// Junta `ids` num grupo cujo principal é o (grupo do) primeiro. Respostas e status não entram.
fn group_edit(page: i32, annots: &[AnnotInfo], ids: &[i32]) -> Result<Edit> {
    let mut all = Vec::new();
    for &id in ids {
        let a = annots
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| PdfError::InvalidAnnotation("anotação não encontrada".into()))?;
        if a.is_reply() {
            return Err(refused("respostas e status não entram em grupos"));
        }
        for m in group_members(annots, group_root(annots, id)) {
            if !all.contains(&m) {
                all.push(m);
            }
        }
    }
    let Some((&root, rest)) = all.split_first().filter(|(_, rest)| !rest.is_empty()) else {
        return Err(refused("escolha dois ou mais comentários para agrupar"));
    };
    let edits: Vec<Edit> = rest
        .iter()
        .filter(|&&m| annots.iter().any(|a| a.id == m && a.group != Some(root)))
        .map(|&m| Edit::Group {
            page,
            id: m,
            to: Some(root),
        })
        .collect();
    if edits.is_empty() {
        return Err(refused("os comentários já estão no mesmo grupo"));
    }
    Ok(Edit::Batch(edits))
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

/// Contorno padrão das formas e linhas: vermelho, 1 pt (como no Acrobat).
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
        PdfAnnotationType::Redact => AnnotKind::Redact,
        PdfAnnotationType::Polygon => AnnotKind::Polygon,
        PdfAnnotationType::Caret => AnnotKind::Caret,
        PdfAnnotationType::FileAttachment => AnnotKind::FileAttachment,
        PdfAnnotationType::PolyLine => AnnotKind::PolyLine,
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

pub(crate) fn annot_info(annot: &PdfAnnotation, page: i32, ctm: &Matrix) -> Result<AnnotInfo> {
    let kind = match kind_of(annot.r#type()?) {
        // a nuvem é um polígono com borda de nuvem (/BE /S /C)
        AnnotKind::Polygon if annot_edit::is_cloudy(annot)? => AnnotKind::Cloud,
        AnnotKind::TextBox if annot_edit::is_callout(annot)? => AnnotKind::Callout,
        k => k,
    };
    let callout = match kind {
        AnnotKind::Callout => annot_edit::callout(annot, ctm)?,
        _ => None,
    };
    let font = matches!(kind, AnnotKind::TextBox | AnnotKind::Callout)
        .then(|| annot.default_appearance())
        .transpose()?
        .flatten()
        .map(|da| (da.size, da.color.map(rgb_of).unwrap_or([0.0; 3])));
    let own_color = annot.color()?.map(rgb_of);
    Ok(AnnotInfo {
        id: annot_id(annot)?,
        kind,
        page,
        // texto explicativo: a caixa (a linha de chamada vem à parte)
        rect: match callout {
            Some((boxed, _)) => boxed,
            None => annot.bounds()?.into(),
        },
        contents: annot.contents()?.unwrap_or_default().to_owned(),
        // caixa de texto: a cor é a do texto e da borda; /C é o fundo
        color: match kind {
            AnnotKind::TextBox | AnnotKind::Callout => font.map(|f| f.1),
            _ => own_color,
        },
        opacity: annot.opacity()?,
        width: kind
            .has_border()
            .then(|| annot.border_width())
            .transpose()?,
        fill: match kind {
            AnnotKind::TextBox | AnnotKind::Callout => own_color,
            k if k.has_fill() => annot.interior_color()?.map(rgb_of),
            _ => None,
        },
        vertices: annot_edit::vertices(annot, ctm)?,
        cloud: (kind == AnnotKind::Cloud)
            .then(|| annot.border_effect_intensity())
            .transpose()?,
        callout: callout.map(|(_, line)| line),
        group: annot_edit::group_of(annot)?,
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

/// Grava `bytes` em `dest` como `write_atomic` (temporário, fsync, rename).
pub(crate) fn write_bytes_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_path(dest)?;
    let written = std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::File::open(&tmp)?.sync_all())
        .and_then(|()| std::fs::rename(&tmp, dest))
        .map_err(PdfError::from);
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
        self.annotations_in(&mut self.inner.load_pdf_page(page)?, page)
    }

    /// Anotações da página já carregada (a abertura carrega cada página uma vez só).
    fn annotations_in(&self, page_ref: &mut PdfPage, page: i32) -> Result<Vec<AnnotInfo>> {
        let mut out = Vec::new();
        let ctm = page_ref.ctm()?;
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
            out.push(annot_info(&annot, page, &ctm)?);
        }
        normalize_groups(&mut out);
        Ok(out)
    }

    /// Anotações e links de todas as páginas, numa passada (cada página é carregada uma vez).
    /// Uma página ilegível perde só as próprias anotações e links.
    /// Anotações, links e anexos (os de comentário somam-se aos do documento).
    fn annotations_links_attachments(&self) -> Result<AnnotsLinksFiles> {
        let (mut annots, mut links) = (Vec::new(), Vec::new());
        let mut files = self.document_attachments();
        let count = self.page_count()?;
        for p in 0..count {
            // página ilegível: perde só o que é dela
            let Ok(mut page_ref) = self.inner.load_pdf_page(p) else {
                continue;
            };
            annots.extend(self.annotations_in(&mut page_ref, p).unwrap_or_default());
            links.extend(self.links_in(&page_ref, p, count).unwrap_or_default());
            crate::attachments::comment_attachments(&page_ref, p, &mut files);
        }
        Ok((annots, links, files))
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
        result?;
        // proteção nova: reabre com a senha nova (a gravação seguinte mantém a do arquivo)
        self.after_protection_saved()
    }

    /// Arquivo → Reverter: também a proteção pedida e as fontes embutidas desde o último Salvar
    /// deixam de valer (o Salvar seguinte grava o arquivo como ele está no disco).
    pub fn revert(&mut self) -> Result<()> {
        self.discard_changes()?;
        self.pending_protection = None;
        self.added_fonts.clear();
        Ok(())
    }

    /// Volta ao arquivo do disco (com a mesma senha), sem as edições em memória.
    pub(crate) fn discard_changes(&mut self) -> Result<()> {
        self.inner = open_authenticated(&self.path, self.password.as_deref())?;
        self.history = History::default();
        self.unsaved = false;
        self.signed_on_disk = self.has_signatures().unwrap_or(false);
        self.reapply_layer_view()
    }

    pub(crate) fn purge_hidden(&mut self) -> Result<()> {
        purge(&self.inner, &self.history.hidden)
    }

    fn write(&mut self, dest: &Path) -> Result<()> {
        let mut opts = PdfWriteOptions::default();
        self.encryption_options(&mut opts);
        if self.signed_on_disk || self.has_signatures()? {
            // documento assinado: só gravação incremental (como o Acrobat), senão a reescrita
            // mudaria os bytes assinados e quebraria as assinaturas
            opts.set_incremental(true);
            let bytes = self.with_file_layers(|d| {
                let mut bytes = Vec::new();
                d.inner.write_to_with_options(&mut bytes, opts)?;
                Ok(bytes)
            })?;
            write_bytes_atomic(dest, &bytes)?;
            self.path = dest.to_path_buf();
            return Ok(());
        }
        self.with_file_layers(|d| write_atomic(&d.inner, dest, opts))?;
        self.path = dest.to_path_buf();
        Ok(())
    }

    /// Caixa de texto `id` com `text` (ou o atual) em `size`: (retângulo atual, novo). O texto solto
    /// do Preencher e assinar fica justo ao texto; a caixa comum só cresce para baixo.
    fn refit(
        &self,
        page: i32,
        id: i32,
        text: Option<&str>,
        size: f32,
    ) -> Result<Option<(PdfRect, PdfRect)>> {
        let Some((rect, current, _, border)) = annot_edit::text_box_of(&self.inner, page, id)?
        else {
            return Ok(None);
        };
        let text = text.unwrap_or(&current);
        let new = if annot_edit::is_typewriter(&self.inner, page, id)? {
            text_fit::snug((rect.x0, rect.y0), text, size)?
        } else {
            text_fit::fitted(rect, text, size, border)?
        };
        Ok(Some((rect, new)))
    }

    /// A mudança que leva a anotação `(page, id)` por (`dx`, `dy`).
    fn move_edit(&self, page: i32, id: i32, dx: f32, dy: f32) -> Result<Edit> {
        use annot_edit::Shape;
        let shift = |p: (f32, f32)| (p.0 + dx, p.1 + dy);
        Ok(match annot_edit::shape_of(&self.inner, page, id)? {
            Shape::Callout { rect, tip, knee } => Edit::Callout {
                page,
                id,
                rect: rect.translated(dx, dy),
                tip: shift(tip),
                knee: shift(knee),
            },
            Shape::Vertices { points, .. } => Edit::Vertices {
                page,
                id,
                points: points.into_iter().map(shift).collect(),
            },
            Shape::Line((a, b)) => Edit::LinePoints {
                page,
                id,
                a: shift(a),
                b: shift(b),
            },
            Shape::Box(rect, ink) => {
                let (rect, ink) = annot_edit::moved(rect, ink, dx, dy);
                Edit::Geometry {
                    page,
                    id,
                    rect,
                    ink,
                }
            }
        })
    }

    /// Colar: cópia da anotação `(page, id)` em `to_page`, deslocada (`dx`, `dy`, coordenadas do
    /// MuPDF). A aparência é copiada (mexer numa não muda a outra). Uma etapa do desfazer; devolve
    /// o id da cópia.
    pub fn duplicate_annot(
        &mut self,
        page: i32,
        id: i32,
        to_page: i32,
        dx: f32,
        dy: f32,
    ) -> Result<i32> {
        self.check_page(to_page)?;
        // o original só é lido aqui: a página dele fecha antes de a de destino ser relida (o MuPDF
        // guarda a lista de anotações de uma página aberta)
        let mut copy = {
            let (_page, src) = annot_edit::find(&self.inner, page, id)?;
            let dict = src
                .object()
                .resolve()?
                .ok_or_else(|| PdfError::InvalidAnnotation("anotação sem dicionário".into()))?;
            let mut budget = MAX_COPIED_OBJECTS;
            deep_copy(&self.inner, &dict, 0, &mut budget)?
        };
        // ligações com o original e a página de origem saem; o MuPDF dá um nome novo. A chave da
        // estrutura (PDF marcado) também: duas anotações não podem ter a mesma.
        for key in ["P", "Popup", "IRT", "NM", "Parent", "StructParent"] {
            copy.dict_delete(key)?;
        }
        // a cópia é sempre visível: o original pode estar escondido (recortado, apagado até salvar)
        // ou ser uma resposta (invisível na página), e a cópia deixa de ser resposta
        if let Some(flags) = copy.get_dict("F")?.and_then(|f| f.as_int().ok()) {
            const HIDDEN: i32 = 2;
            const NO_VIEW: i32 = 32;
            copy.dict_put("F", PdfObject::new_int(flags & !(HIDDEN | NO_VIEW))?)?;
        }
        if let Some(ap) = copy.get_dict("AP")? {
            let own = own_appearance(&mut self.inner, &ap)?;
            copy.dict_put("AP", own)?;
        }
        let target = self.inner.load_pdf_page(to_page)?;
        let mut page_obj = target.object();
        copy.dict_put("P", page_obj.clone())?;
        let new_ref = self.inner.add_object(&copy)?;
        let new_id = new_ref.as_indirect()?;
        match page_obj.get_dict("Annots")? {
            Some(mut annots) if annots.is_array()? => annots.array_push(new_ref)?,
            _ => {
                let mut annots = self.inner.new_array()?;
                annots.array_push(new_ref)?;
                page_obj.dict_put("Annots", annots)?;
            }
        }
        drop(target); // a página relida vê a anotação nova
        let mut undo = vec![Edit::Visibility {
            page: to_page,
            id: new_id,
            hidden: true,
        }];
        if dx != 0.0 || dy != 0.0 {
            let shift = self.move_edit(to_page, new_id, dx, dy)?;
            undo.insert(
                0,
                annot_edit::apply_edit(&self.inner, &mut self.history.hidden, shift)?,
            );
        }
        self.history.record(Edit::Batch(undo));
        Ok(new_id)
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
                if let NewAnnot::ReplaceText { rects, text, .. } = &a {
                    return self.replace_text(page, rects, text);
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
                // o grupo se move junto
                let annots = self.annotations(page)?;
                // marcações de texto do grupo ficam presas ao texto (como no Acrobat)
                let members: Vec<i32> = group_members(&annots, group_root(&annots, id))
                    .into_iter()
                    .filter(|m| m == &id || annots.iter().any(|a| a.id == *m && a.movable()))
                    .collect();
                if members.len() > 1 {
                    let moves = members
                        .into_iter()
                        .map(|m| self.move_edit(page, m, dx, dy))
                        .collect::<Result<Vec<_>>>()?;
                    Edit::Batch(moves)
                } else {
                    self.move_edit(page, id, dx, dy)?
                }
            }
            AnnotOp::Duplicate {
                page,
                id,
                to_page,
                dx,
                dy,
            } => {
                self.duplicate_annot(page, id, to_page, dx, dy)?;
                return Ok(vec![to_page]);
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
            AnnotOp::Cloud {
                page,
                id,
                intensity,
            } => Edit::Cloud {
                page,
                id,
                intensity: if intensity.is_finite() {
                    intensity.clamp(0.0, 2.0)
                } else {
                    return Err(refused("intensidade inválida"));
                },
            },
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
                let fit = self.refit(page, id, None, size)?;
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
            AnnotOp::Erase {
                page,
                ids,
                path,
                radius,
            } => {
                let mut edits = Vec::new();
                for id in ids {
                    let (rect, ink) = annot_edit::geometry_of(&self.inner, page, id)?;
                    let strokes = ink.ok_or_else(|| {
                        PdfError::InvalidAnnotation("a borracha só apaga desenhos".into())
                    })?;
                    let left = annot_edit::erase_strokes(&strokes, &path, radius.max(0.5));
                    if left == strokes {
                        continue; // nada sob a borracha
                    }
                    edits.push(if left.is_empty() {
                        // o desenho todo sumiu: sai (como Excluir)
                        Edit::Visibility {
                            page,
                            id,
                            hidden: true,
                        }
                    } else {
                        Edit::Geometry {
                            page,
                            id,
                            rect,
                            ink: Some(left),
                        }
                    });
                }
                match edits.len() {
                    0 => return Ok(Vec::new()),
                    1 => edits.remove(0),
                    _ => Edit::Batch(edits),
                }
            }
            AnnotOp::MoveCallout {
                page,
                id,
                knee,
                tip,
            } => {
                let annot_edit::Shape::Callout { rect, .. } =
                    annot_edit::shape_of(&self.inner, page, id)?
                else {
                    return Err(PdfError::InvalidAnnotation(
                        "não é um texto explicativo".into(),
                    ));
                };
                Edit::Callout {
                    page,
                    id,
                    rect,
                    tip,
                    knee,
                }
            }
            AnnotOp::MoveVertex {
                page,
                id,
                index,
                to,
            } => {
                let annot_edit::Shape::Vertices { mut points, .. } =
                    annot_edit::shape_of(&self.inner, page, id)?
                else {
                    return Err(PdfError::InvalidAnnotation("não tem vértices".into()));
                };
                let slot = points
                    .get_mut(index)
                    .ok_or_else(|| PdfError::InvalidAnnotation("vértice inexistente".into()))?;
                *slot = to;
                Edit::Vertices { page, id, points }
            }
            AnnotOp::Resize { page, id, rect } => {
                match annot_edit::shape_of(&self.inner, page, id)? {
                    // texto explicativo: só a caixa muda; a linha continua apontando para o mesmo lugar
                    annot_edit::Shape::Callout { tip, knee, .. } => Edit::Callout {
                        page,
                        id,
                        rect,
                        tip,
                        knee,
                    },
                    // os vértices acompanham a caixa (como os traços do desenho)
                    annot_edit::Shape::Vertices { rect: old, points } => Edit::Vertices {
                        page,
                        id,
                        points: annot_edit::scaled(old, rect, Some(vec![points]))
                            .and_then(|mut s| s.pop())
                            .unwrap_or_default(),
                    },
                    annot_edit::Shape::Box(old, ink) => Edit::Geometry {
                        page,
                        id,
                        rect,
                        ink: annot_edit::scaled(old, rect, ink),
                    },
                    annot_edit::Shape::Line(_) => Edit::Geometry {
                        page,
                        id,
                        rect,
                        ink: None,
                    },
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
                    Some((_, _, size, _)) => self.refit(page, id, Some(&text), size)?,
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
                let annots = self.annotations(page)?;
                hide_all(page, with_group_and_replies(&annots, &[id]))
            }
            AnnotOp::DeleteMany { page, ids } => {
                let annots = self.annotations(page)?;
                if ids.iter().any(|id| !annots.iter().any(|a| a.id == *id)) {
                    return Err(PdfError::InvalidAnnotation(
                        "anotação não encontrada".into(),
                    ));
                }
                hide_all(page, with_group_and_replies(&annots, &ids))
            }
            AnnotOp::Group { page, ids } => {
                let annots = self.annotations(page)?;
                group_edit(page, &annots, &ids)?
            }
            AnnotOp::Ungroup { page, id } => {
                let annots = self.annotations(page)?;
                let root = group_root(&annots, id);
                let edits: Vec<Edit> = annots
                    .iter()
                    .filter(|a| a.group == Some(root))
                    .map(|a| Edit::Group {
                        page,
                        id: a.id,
                        to: None,
                    })
                    .collect();
                if edits.is_empty() {
                    return Err(refused("o comentário não está num grupo"));
                }
                Edit::Batch(edits)
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
            Step::Fields(list) => {
                // de trás para a frente; o contrário, aplicado também de trás para a frente,
                // refaz na ordem original
                list.iter()
                    .rev()
                    .map(|(page, id, value)| {
                        let old = self.write_field(*page, *id, value, true, None)?;
                        Ok((*page, *id, old))
                    })
                    .collect::<Result<Vec<_>>>()
                    .map(|inverse| (Step::Fields(inverse), Changed::Fields))
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

    /// Bytes do documento em memória (criptografia mantida). Arquivo assinado: incremental, para
    /// continuar começando pelos bytes assinados (restaurar e salvar depois não pode regravar o
    /// arquivo).
    pub(crate) fn memory_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut keep = PdfWriteOptions::default();
        keep.set_encryption(Encryption::Keep);
        keep.set_incremental(self.signed_on_disk);
        self.inner.write_to_with_options(&mut bytes, keep)?;
        Ok(bytes)
    }

    /// O documento em memória como está agora (com as apagadas pendentes), para desfazer.
    pub(crate) fn snapshot(&self) -> Result<Snapshot> {
        let bytes = self.with_file_layers(|d| d.memory_bytes())?;
        Ok(Snapshot {
            bytes,
            hidden: self.history.hidden.clone(),
            added_fonts: self.added_fonts.clone(),
        })
    }

    /// Volta o documento ao instantâneo (com a mesma senha).
    pub(crate) fn restore(&mut self, snap: &Snapshot) -> Result<()> {
        self.inner = reopen(&snap.bytes, self.password.as_deref())?;
        self.history.hidden = snap.hidden.clone();
        self.added_fonts = snap.added_fonts.clone();
        self.reapply_layer_view()
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

    /// Substituir texto: riscado e cursor de inserção (com a nota) agrupados, como o Acrobat grava
    /// (o riscado aponta para o cursor com /IRT e /RT /Group). Uma etapa do desfazer.
    fn replace_text(&mut self, page: i32, rects: &[PdfRect], text: &str) -> Result<Vec<i32>> {
        let last = *rects
            .last()
            .ok_or_else(|| PdfError::InvalidAnnotation("sem texto para substituir".into()))?;
        let mut page_ref = self.inner.load_pdf_page(page)?;
        // o cursor fica no fim do texto riscado, logo abaixo da linha
        let h = last.height().max(4.0);
        let caret = PdfRect::new(
            last.x1 - h * 0.3,
            last.y1 - h * 0.4,
            last.x1 + h * 0.3,
            last.y1 + h * 0.1,
        );
        let marks = quads(rects)?; // confere antes de criar qualquer coisa
        let mut caret = add_one(
            &mut self.inner,
            &mut page_ref,
            &NewAnnot::Caret {
                page,
                rect: caret,
                text: text.to_owned(),
            },
        )?;
        let strike = (|| -> Result<PdfAnnotation> {
            let mut strike = page_ref.add_strikeout_annotation(marks)?;
            strike.set_color(rgb(TEXT_EDIT_BLUE))?;
            let mut obj = strike.object();
            obj.dict_put("IRT", caret.object())?;
            obj.dict_put("RT", PdfObject::new_name("Group")?)?;
            obj.dict_put("IT", PdfObject::new_name("StrikeOutTextEdit")?)?;
            Ok(strike)
        })();
        // o riscado não saiu: o cursor sozinho também não fica
        let mut strike = match strike {
            Ok(s) => s,
            Err(e) => {
                let _ = page_ref.delete_annotation(caret);
                return Err(e);
            }
        };
        for a in [&mut caret, &mut strike] {
            self.sign(a)?;
            a.update()?;
        }
        page_ref.update()?;
        let hide = |id| Edit::Visibility {
            page,
            id,
            hidden: true,
        };
        self.history.record(Edit::Batch(vec![
            hide(annot_id(&caret)?),
            hide(annot_id(&strike)?),
        ]));
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
        let (annots, links, attachments) = self.annotations_links_attachments().unwrap_or_default();
        let fields = self.all_fields().unwrap_or_default();
        Ok(DocSummary {
            path: self.path.clone(),
            sizes,
            outline: self.outline_tree().unwrap_or_default(),
            annots,
            links,
            initial_view: self.initial_view(),
            attachments,
            display_title: self.display_title(),
            labels: self.labels(),
            layers: self.layers(),
            form_notes: self.form_notes_of(&fields).unwrap_or_default(),
            page_actions: (0..self.page_count()?)
                .filter_map(|p| Some((p, self.page_actions(p).ok().filter(|a| !a.is_empty())?)))
                .collect(),
            fields,
            security: self.security()?,
            is_comparison: self.is_comparison(),
        })
    }

    /// Se o documento é um resultado do Comparar arquivos.
    fn is_comparison(&self) -> bool {
        self.info_dict()
            .and_then(|i| i.get_dict("GeraCompare").ok().flatten())
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false)
    }
}

/// Teto de objetos copiados ao colar um comentário (um comum usa algumas dezenas).
const MAX_COPIED_OBJECTS: usize = 2000;

/// Chaves cujo valor fica compartilhado na cópia: apontam para outras partes do documento
/// (página, árvore, ação, destino), que a edição da anotação nunca altera.
const SHARED_KEYS: [&str; 7] = ["P", "Parent", "Popup", "IRT", "A", "AA", "Dest"];

/// Cópia de um objeto com os dicionários e arrays (diretos ou indiretos) também copiados: uma
/// cópia rasa compartilharia, por exemplo, a borda `/BS`, e mudar a cópia mudaria o original.
/// Streams continuam compartilhados (a aparência é trocada à parte, em `own_appearance`).
fn deep_copy(
    doc: &PdfDocument,
    obj: &PdfObject,
    depth: u32,
    budget: &mut usize,
) -> Result<PdfObject> {
    if depth > 16 {
        return Ok(obj.clone());
    }
    // objetos compartilhados em vários níveis multiplicariam a cópia (PDF malicioso)
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| crate::error::refused("comentário complexo demais para copiar"))?;
    if obj.is_indirect()? {
        return match obj.resolve()? {
            Some(r) if !r.is_stream()? && (r.is_dict()? || r.is_array()?) => {
                deep_copy(doc, &r, depth + 1, budget)
            }
            _ => Ok(obj.clone()),
        };
    }
    if obj.is_dict()? {
        let mut out = doc.new_dict()?;
        for i in 0..obj.dict_len()? as i32 {
            if let (Some(k), Some(v)) = (obj.get_dict_key(i)?, obj.get_dict_val(i)?) {
                let shared = k
                    .as_name()
                    .is_ok_and(|n| SHARED_KEYS.iter().any(|s| s.as_bytes() == n.as_slice()));
                let v = if shared {
                    v
                } else {
                    deep_copy(doc, &v, depth + 1, budget)?
                };
                out.dict_put(k, v)?;
            }
        }
        return Ok(out);
    }
    if obj.is_array()? {
        let mut out = doc.new_array()?;
        for i in 0..obj.len()? as i32 {
            if let Some(v) = obj.get_array(i)? {
                out.array_push(deep_copy(doc, &v, depth + 1, budget)?)?;
            }
        }
        return Ok(out);
    }
    Ok(obj.clone())
}

/// `/AP` com streams próprios em todos os níveis (`/N`, `/R`, `/D` e os estados dentro deles):
/// ao mexer na cópia, o MuPDF regrava a aparência dela, nunca a do original.
fn own_appearance(doc: &mut PdfDocument, ap: &PdfObject) -> Result<PdfObject> {
    let mut out = doc.new_dict()?;
    for i in 0..ap.dict_len()? as i32 {
        let (Some(k), Some(v)) = (ap.get_dict_key(i)?, ap.get_dict_val(i)?) else {
            continue;
        };
        let v = if v.is_stream()? {
            own_stream(doc, &v)?
        } else if v.is_dict()? {
            // estados (ex.: /On e /Off)
            let mut states = doc.new_dict()?;
            for j in 0..v.dict_len()? as i32 {
                if let (Some(sk), Some(sv)) = (v.get_dict_key(j)?, v.get_dict_val(j)?) {
                    let sv = if sv.is_stream()? {
                        own_stream(doc, &sv)?
                    } else {
                        sv
                    };
                    states.dict_put(sk, sv)?;
                }
            }
            states
        } else {
            v
        };
        out.dict_put(k, v)?;
    }
    Ok(out)
}

fn own_stream(doc: &mut PdfDocument, stream: &PdfObject) -> Result<PdfObject> {
    let raw = mupdf::Buffer::from_bytes(&stream.read_raw_stream()?)?;
    let dict = stream.copy_dict()?;
    Ok(doc.add_stream(&raw, Some(&dict), true)?)
}

/// Escala máxima de um ladrilho: 6400 % numa tela 4× (o ladrilho tem tamanho fixo).
const MAX_TILE_SCALE: f32 = 256.0;

/// Pixels RGB compactos (sem o preenchimento de cada linha) de um pixmap `device_rgb` sem alfa.
fn packed_rgb(pix: &mupdf::Pixmap, invert: bool) -> RenderedPage {
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
    RenderedPage {
        width: w as u32,
        height: h as u32,
        rgb,
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
