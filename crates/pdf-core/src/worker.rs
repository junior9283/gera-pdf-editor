use crate::error::PdfError;
use crate::types::{
    AnnotInfo, AnnotOp, Changed, DocProperties, DocSummary, HistoryState, PageOp, PdfRect,
    RenderedPage, Rotation, TextSelection,
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
    Search {
        id: u64,
        needle: String,
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
    AnnotFailed(String),
    /// As páginas mudaram (organizar, ou desfazer/refazer de uma operação de página): o resumo
    /// novo do documento inteiro.
    PagesChanged {
        summary: DocSummary,
        history: HistoryState,
    },
    PagesFailed(String),
    Extracted(Vec<PathBuf>),
    Split(Vec<PathBuf>),
    SplitFailed(String),
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
    needle: String,
    cancel: CancelToken,
    next: i32,
    pages: i32,
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
            Request::Search { id, needle, cancel } => {
                let pages = self.doc.as_ref().and_then(|d| d.page_count().ok());
                match pages {
                    Some(pages) => {
                        self.search = Some(SearchJob {
                            id,
                            needle,
                            cancel,
                            next: 0,
                            pages,
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
            send(Response::AnnotFailed(NO_DOC.into()));
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
            Ok(Changed::Pages(pages)) => send(Response::AnnotsChanged {
                annots: pages
                    .iter()
                    .flat_map(|&p| d.annotations(p).unwrap_or_default())
                    .collect(),
                pages,
                history: d.history_state(),
            }),
            Err(e) => send(Response::AnnotFailed(e.to_string())),
        }
    }

    /// Roda `f` no documento aberto; erros (e a falta de documento) viram texto para a UI.
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
            if let Ok(rects) = doc.search(page, &job.needle) {
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
