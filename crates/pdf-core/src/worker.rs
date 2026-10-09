use crate::error::PdfError;
use crate::types::{
    AnnotInfo, DocProperties, DocSummary, NewAnnot, PdfRect, RenderedPage, Rotation, TextSelection,
    Word,
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
    Words {
        tag: u64,
        page: i32,
        area: PdfRect,
    },
    Save {
        pending: Vec<NewAnnot>,
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
    /// Cópia para imprimir em `dest` (com as pendentes ou sem nenhuma anotação).
    ExportForPrint {
        pending: Vec<NewAnnot>,
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
    Words {
        tag: u64,
        page: i32,
        words: Vec<Word>,
    },
    Saved {
        annots: Vec<AnnotInfo>,
    },
    SaveFailed(String),
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
}

impl State {
    fn handle(&mut self, request: Request, send: &dyn Fn(Response)) {
        match request {
            Request::Open { path, password } => {
                self.search = None;
                match PdfDoc::open(&path, password.as_deref())
                    .and_then(|d| d.summary().map(|s| (d, s)))
                {
                    Ok((d, summary)) => {
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
            Request::Words { tag, page, area } => {
                let words = self
                    .doc
                    .as_ref()
                    .and_then(|d| d.words_in_rect(page, area).ok())
                    .unwrap_or_default();
                send(Response::Words { tag, page, words });
            }
            Request::Save { pending, dest } => {
                let Some(d) = self.doc.as_mut() else {
                    send(Response::SaveFailed(NO_DOC.into()));
                    return;
                };
                match d.save_with(&pending, &dest) {
                    Ok(()) => send(Response::Saved {
                        annots: d.all_annotations().unwrap_or_default(),
                    }),
                    Err(e) => send(Response::SaveFailed(e.to_string())),
                }
            }
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
            Request::Properties => match self.doc.as_ref().map(PdfDoc::properties) {
                Some(Ok(p)) => send(Response::Properties(p)),
                Some(Err(e)) => send(Response::PropertiesFailed(e.to_string())),
                None => send(Response::PropertiesFailed(NO_DOC.into())),
            },
            Request::ExportForPrint {
                pending,
                dest,
                with_annots,
            } => match self
                .doc
                .as_ref()
                .map(|d| d.export_for_print(&pending, &dest, with_annots))
            {
                Some(Ok(())) => send(Response::Exported { dest }),
                Some(Err(e)) => send(Response::ExportFailed(e.to_string())),
                None => send(Response::ExportFailed(NO_DOC.into())),
            },
        }
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
    loop {
        // Pedidos pendentes têm prioridade sobre a busca; sem busca ativa, espera bloqueando.
        let next = if state.search.is_some() {
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
            Some(request) => state.handle(request, &send),
            None => state.search_step(&send),
        }
    }
}
