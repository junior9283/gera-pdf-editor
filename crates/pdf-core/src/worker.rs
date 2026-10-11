use crate::error::PdfError;
use crate::types::{
    AnnotInfo, AnnotOp, Changed, DocProperties, DocSummary, HistoryState, PageOp, PdfRect,
    RenderedPage, Rotation, SearchOptions, TextSelection,
};
use crate::PdfDoc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Lado dos ladrilhos de alta resolução, em pixels.
pub const TILE: u32 = 512;

pub enum Request {
    Open {
        path: PathBuf,
        password: Option<String>,
    },
    Render {
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        cancel: CancelToken,
    },
    /// Arquivo → Reverter: relê o arquivo salvo (responde `PagesChanged`).
    Revert,
    /// Anexos (respondem `AttachmentsChanged`).
    AddAttachment {
        path: PathBuf,
    },
    RemoveAttachment {
        name: String,
    },
    /// Olho de uma camada (só a vista).
    LayerView {
        id: i32,
        on: bool,
    },
    /// Redefinir para visibilidade inicial.
    ResetLayers,
    /// Propriedades da camada: nome e estado padrão.
    EditLayer {
        id: i32,
        name: String,
        default_on: bool,
    },
    /// Cria, renomeia, redefine ou exclui um destino nomeado.
    Destination(crate::DestOp),
    /// Grava os rótulos de página (diálogo Rótulos de página).
    PageLabels(Vec<crate::PageLabelRange>),
    DescribeAttachment {
        name: String,
        text: String,
    },
    /// Grava o anexo em `dest` (Salvar, ou a cópia para abrir); responde `AttachmentWritten`.
    WriteAttachment {
        origin: crate::types::AttachmentOrigin,
        name: String,
        dest: PathBuf,
        open: bool,
    },
    /// Edição dos marcadores (responde `OutlineChanged`).
    Outline(crate::types::OutlineOp),
    /// Propriedades → OK (responde `PagesChanged`: o resumo leva a visualização inicial).
    SetProperties(crate::types::PropertiesEdit),
    /// Ladrilho de `TILE` px da página (zoom acima do teto da página inteira).
    RenderTile {
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        tile: (u32, u32),
        cancel: CancelToken,
    },
    /// Caixa da tinta da página, para Ajustar ao conteúdo visível.
    ContentBox {
        page: i32,
    },
    Search {
        id: u64,
        needle: String,
        options: SearchOptions,
        cancel: CancelToken,
    },
    /// Grava o documento em memória (com as edições) em `dest`.
    Save {
        dest: PathBuf,
    },
    /// Edição de anotação, aplicada na hora no documento em memória.
    Annot(AnnotOp),
    Undo,
    Redo,
    /// Autor gravado nas anotações criadas (vale também para documentos abertos depois).
    SetAuthor(Option<String>),
    /// Preenche um campo de formulário (botão de opção: o estado dele).
    SetField {
        page: i32,
        id: i32,
        value: String,
    },
    /// Limpar formulário (valores padrão).
    ResetForm,
    /// Preparar formulário: criar, editar, mover ou excluir um campo.
    Prepare(crate::PrepareOp),
    /// Assina digitalmente e grava o arquivo (Certificados).
    SignDigitally {
        place: crate::SignPlace,
        id: Box<crate::DigitalId>,
        reason: String,
        location: String,
    },
    /// Assinaturas do documento, validadas.
    Signatures,
    /// Proteção a aplicar ao Salvar (`None`: remover a segurança).
    SetProtection(Option<crate::Protection>),
    /// Senha de permissões: libera tudo.
    Unlock {
        password: String,
    },
    /// Situação de segurança (depois de Salvar).
    Security,
    /// Comprimir PDF: grava "<nome> (comprimido).pdf" ao lado do original.
    Compress {
        level: crate::CompressLevel,
    },
    /// Reconhecer texto (OCR) nas páginas; o progresso chega a cada página.
    RecognizeText {
        pages: Vec<i32>,
        lang: crate::OcrLanguage,
        cancel: CancelToken,
    },
    /// Exportar PDF (Word, texto, imagens) ao lado do original.
    Export {
        format: crate::ExportFormat,
    },
    /// Propriedades do campo, para a janela Propriedades.
    FieldProps {
        page: i32,
        id: i32,
    },
    /// Imagens da página (Editar PDF).
    PageImages {
        page: i32,
    },
    /// Editar PDF: mover, redimensionar, girar, excluir ou substituir uma imagem.
    EditImage {
        page: i32,
        name: String,
        edit: crate::ImageEdit,
    },
    /// Editar PDF: acrescenta a imagem do arquivo centrada em `center` (pontos da página).
    AddImage {
        page: i32,
        path: PathBuf,
        center: (f32, f32),
    },
    /// Blocos de texto da página (Editar PDF).
    PageTextBlocks {
        page: i32,
    },
    /// Editar PDF: troca o texto do bloco de caixa `rect` (vazio apaga).
    EditTextBlock {
        page: i32,
        rect: crate::PdfRect,
        text: String,
    },
    /// Editar PDF: acrescenta texto com o canto superior esquerdo em `at` (pontos da página).
    AddText {
        page: i32,
        at: (f32, f32),
        text: String,
    },
    /// Marca d'água ou cabeçalho e rodapé (`replace`: no lugar dos existentes — o Atualizar).
    AddPagination {
        config: Box<crate::PaginationConfig>,
        replace: bool,
    },
    /// Redigir: procura frases ou padrões no documento.
    FindRedactions(crate::RedactQuery),
    /// Redigir: aplica todas as marcas de redação (permanente).
    ApplyRedactions(crate::RedactLook),
    /// Limpar documento (remover informações ocultas).
    Sanitize(crate::SanitizeOptions),
    /// Tira as marcas d'água (ou os cabeçalhos e rodapés) de todas as páginas.
    RemovePagination(crate::Pagination),
    /// Há marca d'água (ou cabeçalho e rodapé)? E as escolhas gravadas, para o Atualizar.
    PaginationState(crate::Pagination),
    /// Organizar páginas (girar, excluir, mover, inserir, substituir, recortar).
    Pages(PageOp),
    /// Divide o documento em arquivos na pasta `dir`; o documento não muda.
    Split {
        rule: crate::SplitRule,
        dir: PathBuf,
    },
    /// Grava as páginas em outro(s) arquivo(s); o documento não muda.
    Extract {
        pages: Vec<i32>,
        dest: PathBuf,
        separate: bool,
    },
    /// Grava as páginas (com as anotações) num arquivo, para copiá-las para outro documento.
    ExportPages {
        pages: Vec<i32>,
        dest: PathBuf,
    },
    /// Seleção pelo fluxo do texto entre `a` e `b` (pontos da página).
    SelectText {
        tag: u64,
        page: i32,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// Texto do documento inteiro (Selecionar tudo).
    DocumentText {
        tag: u64,
    },
    Properties,
    /// Cópia para imprimir em `dest` (o documento em memória, com ou sem as anotações).
    ExportForPrint {
        dest: PathBuf,
        with_annots: bool,
    },
}

#[derive(Debug)]
pub enum Response {
    Opened(DocSummary),
    OpenFailed(PdfError),
    Rendered {
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        image: RenderedPage,
    },
    RenderFailed {
        page: i32,
        error: String,
    },
    /// Ladrilho pronto. Um ladrilho que falha não volta (fica a página ampliada por baixo).
    Tile {
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        tile: (u32, u32),
        image: RenderedPage,
    },
    ContentBox {
        page: i32,
        rect: Option<PdfRect>,
    },
    AttachmentsChanged {
        attachments: Vec<crate::types::AttachmentInfo>,
        history: HistoryState,
    },
    /// As camadas mudaram (olho ou propriedades): a renderização também.
    LayersChanged {
        layers: Vec<crate::LayerNode>,
        history: HistoryState,
    },
    DestinationsChanged {
        destinations: Vec<crate::DestInfo>,
        created: Option<String>,
        history: HistoryState,
    },
    /// O anexo foi gravado em `dest` (`open`: abrir numa aba).
    AttachmentWritten {
        dest: PathBuf,
        open: bool,
    },
    /// Marcadores depois de uma edição (`created`: o novo, para selecionar).
    OutlineChanged {
        outline: Vec<crate::types::OutlineNode>,
        created: Option<i32>,
        history: HistoryState,
    },
    SearchHits {
        id: u64,
        page: i32,
        rects: Vec<PdfRect>,
    },
    SearchDone {
        id: u64,
    },
    Saved {
        annots: Vec<AnnotInfo>,
        history: HistoryState,
    },
    /// Gravação falhou: as edições continuam em memória, o histórico pode ter recomeçado.
    SaveFailed {
        error: String,
        history: HistoryState,
    },
    Selection {
        tag: u64,
        page: i32,
        selection: TextSelection,
    },
    DocumentText {
        tag: u64,
        text: String,
    },
    Properties(DocProperties),
    PropertiesFailed(String),
    Exported {
        dest: PathBuf,
    },
    ExportFailed(String),
    /// As anotações mudaram (edição, desfazer ou refazer): páginas mudadas (a renderizar de
    /// novo), as anotações visíveis delas e o estado do histórico.
    AnnotsChanged {
        pages: Vec<i32>,
        annots: Vec<AnnotInfo>,
        history: HistoryState,
    },
    EditFailed(String),
    /// As páginas mudaram (organizar, ou desfazer/refazer de uma operação de página): o resumo
    /// novo do documento inteiro.
    PagesChanged {
        summary: DocSummary,
        history: HistoryState,
    },
    PagesFailed(String),
    Extracted(Vec<PathBuf>),
    /// As páginas pedidas por `ExportPages` estão em `dest`.
    PagesExported {
        dest: PathBuf,
    },
    Split(Vec<PathBuf>),
    /// Campos mudaram (preencher, desfazer/refazer de um valor): todos, de todas as páginas.
    FieldsChanged {
        fields: Vec<crate::FieldInfo>,
        history: HistoryState,
    },
    SplitFailed(String),
    /// Documento assinado e gravado: o resumo novo (campos, anotações) e o histórico (vazio).
    Signed {
        summary: DocSummary,
        history: HistoryState,
    },
    SignFailed(String),
    Signatures(Vec<crate::SignatureInfo>),
    SecurityChanged {
        security: crate::SecurityInfo,
        /// Há proteção esperando o Salvar.
        pending: bool,
        history: HistoryState,
    },
    SecurityFailed(String),
    Compressed {
        dest: PathBuf,
        report: crate::CompressReport,
    },
    CompressFailed(String),
    /// Reconhecer texto: começando a página `done` de `total`.
    OcrProgress {
        done: usize,
        total: usize,
    },
    OcrDone {
        report: crate::OcrReport,
        history: HistoryState,
    },
    OcrFailed(String),
    /// Arquivos criados pelo Exportar PDF.
    ExportedTo(Vec<PathBuf>),
    ExportToFailed(String),
    /// Imagens de uma página (vazio se não deu para ler).
    PageImages {
        page: i32,
        images: Vec<crate::PageImage>,
    },
    /// Resultado do Aplicar do Redigir (resposta própria: o "Aplicar e salvar" só grava com ela).
    RedactionsApplied {
        result: std::result::Result<(DocSummary, crate::HistoryState), String>,
    },
    /// Trechos achados pelo Localizar do Redigir.
    RedactHits(Vec<crate::RedactHit>),
    /// Resposta ao `PaginationState`.
    PaginationState {
        kind: crate::Pagination,
        present: bool,
        config: Option<crate::PaginationConfig>,
    },
    /// Blocos de texto de uma página (vazio se não deu para ler).
    PageTextBlocks {
        page: i32,
        blocks: Vec<crate::PageTextBlock>,
    },
    /// Propriedades de um campo (Preparar formulário).
    FieldProps {
        page: i32,
        id: i32,
        props: Result<crate::FieldProps, String>,
    },
}

const NO_DOC: &str = "nenhum documento aberto";

/// Thread dedicada ao documento: o `Document` do MuPDF não é `Send`, então só ela o toca.
pub struct Worker {
    tx: Sender<Request>,
    pub rx: Receiver<Response>,
}

impl Worker {
    /// `wake` é chamado depois de cada resposta (a UI usa para pedir repaint).
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, req_rx) = mpsc::channel();
        let (resp_tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("pdf-worker".into())
            .stack_size(16 << 20)
            .spawn(move || run(req_rx, resp_tx, wake))
            .expect("não foi possível criar a thread do PDF");
        Self { tx, rx }
    }

    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }
}

/// Busca em andamento, avançada uma página por vez para não prender os outros pedidos.
struct SearchJob {
    id: u64,
    query: crate::search::SearchQuery,
    cancel: CancelToken,
    next: i32,
    pages: i32,
    /// Marcadores que casam (página, topo): entram no lote da página deles, antes do texto.
    bookmarks: Vec<(i32, PdfRect)>,
}

#[derive(Default)]
struct State {
    doc: Option<PdfDoc>,
    search: Option<SearchJob>,
    author: Option<String>,
}

impl State {
    fn handle(&mut self, request: Request, send: &dyn Fn(Response)) {
        match request {
            Request::Open { path, password } => {
                self.search = None;
                match PdfDoc::open(&path, password.as_deref())
                    .and_then(|d| d.summary().map(|s| (d, s)))
                {
                    Ok((mut d, summary)) => {
                        d.set_author(self.author.clone());
                        self.doc = Some(d);
                        send(Response::Opened(summary));
                    }
                    Err(e) => send(Response::OpenFailed(e)),
                }
            }
            Request::Render {
                page,
                scale,
                invert,
                rotation,
                cancel,
            } => {
                let Some(d) = self.doc.as_ref().filter(|_| !cancel.is_cancelled()) else {
                    return;
                };
                match d.render_rotated(page, scale, invert, rotation) {
                    Ok(image) => send(Response::Rendered {
                        page,
                        scale,
                        invert,
                        rotation,
                        image,
                    }),
                    Err(e) => send(Response::RenderFailed {
                        page,
                        error: e.to_string(),
                    }),
                }
            }
            Request::RenderTile {
                page,
                scale,
                invert,
                rotation,
                tile,
                cancel,
            } => {
                let Some(d) = self.doc.as_ref().filter(|_| !cancel.is_cancelled()) else {
                    return;
                };
                if let Ok(image) = d.render_tile(page, scale, invert, rotation, tile, TILE) {
                    send(Response::Tile {
                        page,
                        scale,
                        invert,
                        rotation,
                        tile,
                        image,
                    });
                }
            }
            Request::Outline(op) => self.edit(send, |d| Ok(Changed::Outline(d.edit_outline(op)?))),
            Request::AddAttachment { path } => self.edit(send, |d| {
                d.add_attachment(&path, "")?;
                Ok(Changed::Attachments)
            }),
            Request::LayerView { id, on } => self.edit(send, |d| {
                d.set_layer_view(id, on)?;
                Ok(Changed::Layers)
            }),
            Request::ResetLayers => self.edit(send, |d| {
                d.reset_layer_view()?;
                Ok(Changed::Layers)
            }),
            Request::EditLayer {
                id,
                name,
                default_on,
            } => self.edit(send, |d| {
                d.edit_layer(id, &name, default_on)?;
                Ok(Changed::Layers)
            }),
            Request::Destination(op) => {
                self.edit(send, |d| Ok(Changed::Destinations(d.edit_destination(op)?)))
            }
            Request::PageLabels(ranges) => self.edit(send, |d| {
                d.set_page_labels(ranges)?;
                Ok(Changed::Document)
            }),
            Request::RemoveAttachment { name } => self.edit(send, |d| {
                d.remove_attachment(&name)?;
                Ok(Changed::Attachments)
            }),
            Request::DescribeAttachment { name, text } => self.edit(send, |d| {
                d.set_attachment_description(&name, &text)?;
                Ok(Changed::Attachments)
            }),
            Request::WriteAttachment {
                origin,
                name,
                dest,
                open,
            } => send(
                match self.with_doc(|d| {
                    let bytes = d.attachment_bytes(&origin, &name)?;
                    crate::doc::write_bytes_atomic(&dest, &bytes)
                }) {
                    Ok(()) => Response::AttachmentWritten { dest, open },
                    Err(e) => Response::EditFailed(e),
                },
            ),
            Request::SetProperties(e) => self.edit(send, |d| {
                d.set_properties(&e)?;
                Ok(Changed::Document)
            }),
            Request::Revert => self.edit(send, |d| {
                d.revert()?;
                Ok(Changed::Document)
            }),
            Request::ContentBox { page } => {
                if let Some(d) = self.doc.as_ref() {
                    let rect = d.content_box(page).ok().flatten();
                    send(Response::ContentBox { page, rect });
                }
            }
            Request::Search {
                id,
                needle,
                options,
                cancel,
            } => {
                let doc = self.doc.as_ref();
                match doc.and_then(|d| Some((d, d.page_count().ok()?))) {
                    Some((d, pages)) => {
                        let bookmarks = d.search_bookmarks(&needle, &options);
                        self.search = Some(SearchJob {
                            id,
                            query: crate::search::SearchQuery::new(&needle, options),
                            cancel,
                            next: 0,
                            pages,
                            bookmarks,
                        })
                    }
                    None => send(Response::SearchDone { id }),
                }
            }
            Request::Save { dest } => {
                let Some(d) = self.doc.as_mut() else {
                    send(Response::SaveFailed {
                        error: NO_DOC.into(),
                        history: HistoryState::default(),
                    });
                    return;
                };
                let result = d.save(&dest);
                let history = d.history_state();
                send(match result {
                    Ok(()) => Response::Saved {
                        annots: d.all_annotations().unwrap_or_default(),
                        history,
                    },
                    Err(e) => Response::SaveFailed {
                        error: e.to_string(),
                        history,
                    },
                });
            }
            Request::Annot(op) => self.edit(send, |d| d.apply(op).map(Changed::Pages)),
            Request::SetField { page, id, value } => self.edit(send, |d| {
                d.set_field(page, id, &value).map(|()| Changed::Fields)
            }),
            Request::ResetForm => self.edit(send, |d| d.reset_form().map(|()| Changed::Document)),
            Request::SignDigitally {
                place,
                id,
                reason,
                location,
            } => send(
                match self.with_doc_mut(|d| {
                    d.sign_digitally(place, &id, &reason, &location)?;
                    Ok((d.summary()?, d.history_state()))
                }) {
                    Ok((summary, history)) => Response::Signed { summary, history },
                    Err(e) => Response::SignFailed(e),
                },
            ),
            Request::SetProtection(p) => self.security(send, |d| d.set_protection(p)),
            Request::Unlock { password } => self.security(send, |d| d.unlock(&password)),
            Request::Security => self.security(send, |_| Ok(())),
            Request::RecognizeText {
                pages,
                lang,
                cancel,
            } => {
                let total = pages.len();
                let result = self.with_doc_mut(|d| {
                    let report = d.recognize_text(&pages, lang, |done, _| {
                        send(Response::OcrProgress { done, total });
                        !cancel.is_cancelled()
                    })?;
                    Ok((report, d.history_state()))
                });
                send(match result {
                    Ok((report, history)) => Response::OcrDone { report, history },
                    Err(e) => Response::OcrFailed(e),
                });
            }
            Request::Export { format } => send(match self.with_doc(|d| d.export(format)) {
                Ok(files) => Response::ExportedTo(files),
                Err(e) => Response::ExportToFailed(e),
            }),
            Request::Compress { level } => send(
                match self.with_doc(|d| {
                    let stem = crate::split::stem_of(d.path(), "documento");
                    let dest = crate::pages::free_name(d.path(), &format!("{stem} (comprimido)"));
                    d.compress_to(&dest, level).map(|r| (dest, r))
                }) {
                    Ok((dest, report)) => Response::Compressed { dest, report },
                    Err(e) => Response::CompressFailed(e),
                },
            ),
            Request::Signatures => send(Response::Signatures(
                self.with_doc(|d| d.signatures()).unwrap_or_default(),
            )),
            Request::FieldProps { page, id } => send(Response::FieldProps {
                page,
                id,
                props: self
                    .with_doc(|d| d.field_props(page, id))
                    .map_err(|e| e.to_string()),
            }),
            Request::Prepare(op) => self.edit(send, |d| d.prepare(op).map(|()| Changed::Fields)),
            Request::PageImages { page } => send(Response::PageImages {
                page,
                images: self.with_doc(|d| d.page_images(page)).unwrap_or_default(),
            }),
            Request::EditImage { page, name, edit } => self.edit(send, |d| {
                d.edit_image(page, &name, edit)
                    .map(|()| Changed::Pages(vec![page]))
            }),
            Request::AddImage { page, path, center } => self.edit(send, |d| {
                d.add_image(page, &path, center)
                    .map(|()| Changed::Pages(vec![page]))
            }),
            Request::PageTextBlocks { page } => send(Response::PageTextBlocks {
                page,
                blocks: self
                    .with_doc(|d| d.page_text_blocks(page))
                    .unwrap_or_default(),
            }),
            Request::EditTextBlock { page, rect, text } => self.edit(send, |d| {
                d.edit_text_block(page, rect, &text)
                    .map(|()| Changed::Pages(vec![page]))
            }),
            Request::AddText { page, at, text } => self.edit(send, |d| {
                d.add_text(page, at, &text)
                    .map(|()| Changed::Pages(vec![page]))
            }),
            Request::AddPagination { config, replace } => self.edit(send, |d| {
                d.add_pagination(&config, replace)
                    .map(|()| Changed::Document)
            }),
            Request::FindRedactions(query) => {
                send(match self.with_doc(|d| d.find_redactions(&query)) {
                    Ok(hits) => Response::RedactHits(hits),
                    Err(e) => Response::EditFailed(e.to_string()),
                })
            }
            Request::ApplyRedactions(look) => {
                let result = match self.doc.as_mut() {
                    None => Err(NO_DOC.to_string()),
                    Some(d) => d
                        .apply_redactions(&look)
                        .and_then(|_| Ok((d.summary()?, d.history_state())))
                        .map_err(|e| e.to_string()),
                };
                send(Response::RedactionsApplied { result })
            }
            Request::Sanitize(opts) => {
                self.edit(send, |d| d.sanitize(&opts).map(|()| Changed::Document))
            }
            Request::RemovePagination(kind) => self.edit(send, |d| {
                d.remove_pagination(kind).map(|_| Changed::Document)
            }),
            Request::PaginationState(kind) => {
                let (present, config) = self
                    .with_doc(|d| d.pagination_state(kind))
                    .unwrap_or((false, None));
                send(Response::PaginationState {
                    kind,
                    present,
                    config,
                })
            }
            Request::Pages(op) => {
                self.edit(send, |d| d.apply_pages(op).map(|()| Changed::Document))
            }
            Request::Split { rule, dir } => send(
                match self.with_doc(|d| {
                    d.split(rule, &dir, &crate::split::stem_of(d.path(), "documento"))
                }) {
                    Ok(files) => Response::Split(files),
                    Err(e) => Response::SplitFailed(e),
                },
            ),
            Request::Extract {
                pages,
                dest,
                separate,
            } => send(
                match self.with_doc(|d| d.extract(&pages, &dest, separate)) {
                    Ok(files) => Response::Extracted(files),
                    Err(e) => Response::PagesFailed(e),
                },
            ),
            Request::ExportPages { pages, dest } => {
                send(match self.with_doc(|d| d.extract(&pages, &dest, false)) {
                    Ok(_) => Response::PagesExported { dest },
                    Err(e) => Response::PagesFailed(e),
                })
            }
            Request::Undo => self.edit(send, PdfDoc::undo),
            Request::SetAuthor(author) => {
                if let Some(d) = self.doc.as_mut() {
                    d.set_author(author.clone());
                }
                self.author = author;
            }
            Request::Redo => self.edit(send, PdfDoc::redo),
            Request::SelectText { tag, page, a, b } => {
                let selection = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.select_text(page, a, b).ok())
                    .unwrap_or_default();
                send(Response::Selection {
                    tag,
                    page,
                    selection,
                });
            }
            Request::DocumentText { tag } => {
                let text = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.document_text().ok())
                    .unwrap_or_default();
                send(Response::DocumentText { tag, text });
            }
            Request::Properties => send(
                self.with_doc(PdfDoc::properties)
                    .map_or_else(Response::PropertiesFailed, Response::Properties),
            ),
            Request::ExportForPrint { dest, with_annots } => send(
                match self.with_doc(|d| d.export_for_print(&dest, with_annots)) {
                    Ok(()) => Response::Exported { dest },
                    Err(e) => Response::ExportFailed(e),
                },
            ),
        }
    }

    /// Aplica uma edição de anotação e conta o resultado à interface.
    fn edit(
        &mut self,
        send: &dyn Fn(Response),
        f: impl FnOnce(&mut PdfDoc) -> crate::Result<Changed>,
    ) {
        let Some(d) = self.doc.as_mut() else {
            send(Response::EditFailed(NO_DOC.into()));
            return;
        };
        match f(d) {
            Ok(Changed::Document) => send(match d.summary() {
                Ok(summary) => Response::PagesChanged {
                    summary,
                    history: d.history_state(),
                },
                Err(e) => Response::PagesFailed(e.to_string()),
            }),
            Ok(Changed::Outline(created)) => send(Response::OutlineChanged {
                outline: d.outline_tree().unwrap_or_default(),
                created,
                history: d.history_state(),
            }),
            Ok(Changed::Layers) => send(Response::LayersChanged {
                layers: d.layers(),
                history: d.history_state(),
            }),
            Ok(Changed::Destinations(created)) => send(Response::DestinationsChanged {
                destinations: d.destinations(),
                created,
                history: d.history_state(),
            }),
            Ok(Changed::Attachments) => send(Response::AttachmentsChanged {
                attachments: d.attachments(),
                history: d.history_state(),
            }),
            Ok(Changed::Fields) => send(Response::FieldsChanged {
                fields: d.all_fields().unwrap_or_default(),
                history: d.history_state(),
            }),
            Ok(Changed::Pages(pages)) => send(Response::AnnotsChanged {
                annots: pages
                    .iter()
                    .flat_map(|&p| d.annotations(p).unwrap_or_default())
                    .collect(),
                pages,
                history: d.history_state(),
            }),
            Err(e) => send(Response::EditFailed(e.to_string())),
        }
    }

    /// Muda a segurança e conta a situação nova à interface.
    fn security(
        &mut self,
        send: &dyn Fn(Response),
        f: impl FnOnce(&mut PdfDoc) -> crate::Result<()>,
    ) {
        send(
            match self.with_doc_mut(|d| {
                f(d)?;
                Ok((d.security()?, d.protection_pending(), d.history_state()))
            }) {
                Ok((security, pending, history)) => Response::SecurityChanged {
                    security,
                    pending,
                    history,
                },
                Err(e) => Response::SecurityFailed(e),
            },
        )
    }

    /// Roda `f` no documento aberto; erros (e a falta de documento) viram texto para a UI.
    fn with_doc_mut<T>(
        &mut self,
        f: impl FnOnce(&mut PdfDoc) -> crate::Result<T>,
    ) -> Result<T, String> {
        let doc = self.doc.as_mut().ok_or_else(|| NO_DOC.to_string())?;
        f(doc).map_err(|e| e.to_string())
    }

    fn with_doc<T>(&self, f: impl FnOnce(&PdfDoc) -> crate::Result<T>) -> Result<T, String> {
        let doc = self.doc.as_ref().ok_or_else(|| NO_DOC.to_string())?;
        f(doc).map_err(|e| e.to_string())
    }

    /// Busca a próxima página do `search` ativo; encerra a busca ao terminar ou cancelar.
    fn search_step(&mut self, send: &dyn Fn(Response)) {
        let (Some(job), Some(doc)) = (self.search.as_mut(), self.doc.as_ref()) else {
            self.search = None;
            return;
        };
        if job.cancel.is_cancelled() {
            self.search = None;
        } else if job.next >= job.pages {
            send(Response::SearchDone { id: job.id });
            self.search = None;
        } else {
            let page = job.next;
            job.next += 1;
            if let Ok(found) = doc.search_page(page, &job.query) {
                let rects: Vec<PdfRect> = job
                    .bookmarks
                    .iter()
                    .filter(|(p, _)| *p == page)
                    .map(|&(_, r)| r)
                    .chain(found)
                    .collect();
                if !rects.is_empty() {
                    send(Response::SearchHits {
                        id: job.id,
                        page,
                        rects,
                    });
                }
            }
        }
    }
}

fn run(requests: Receiver<Request>, out: Sender<Response>, wake: impl Fn()) {
    let send = |r: Response| {
        let _ = out.send(r);
        wake();
    };
    let mut state = State::default();
    // Pedido já tirado da fila ao juntar seleções, ainda por atender.
    let mut held: Option<Request> = None;
    loop {
        // Pedidos pendentes têm prioridade sobre a busca; sem busca ativa, espera bloqueando.
        let next = if held.is_some() {
            held.take()
        } else if state.search.is_some() {
            match requests.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            match requests.recv() {
                Ok(r) => Some(r),
                Err(_) => return,
            }
        };
        match next {
            Some(request @ Request::SelectText { .. }) => {
                // Arrastar o mouse gera um pedido por quadro: só o mais novo da fila importa.
                let mut latest = request;
                while let Ok(r) = requests.try_recv() {
                    if matches!(r, Request::SelectText { .. }) {
                        latest = r;
                    } else {
                        held = Some(r);
                        break;
                    }
                }
                state.handle(latest, &send);
            }
            Some(request) => state.handle(request, &send),
            None => state.search_step(&send),
        }
    }
}
