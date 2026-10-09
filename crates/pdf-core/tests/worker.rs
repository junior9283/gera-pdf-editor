mod common;
use common::write_sample;
use pdf_core::{CancelToken, NewAnnot, PdfError, PdfRect, Request, Response, Rotation, Worker};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn recv(w: &Worker) -> Response {
    w.rx.recv_timeout(Duration::from_secs(20))
        .expect("o worker deveria responder")
}

fn opened(pages: usize) -> (tempfile::TempDir, Worker, Arc<AtomicUsize>) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", pages, 0);
    let wakes = Arc::new(AtomicUsize::new(0));
    let w2 = wakes.clone();
    let worker = Worker::spawn(move || {
        w2.fetch_add(1, Ordering::SeqCst);
    });
    worker.send(Request::Open {
        path: path.clone(),
        password: None,
    });
    assert!(matches!(recv(&worker), Response::Opened(s) if s.sizes.len() == pages));
    (dir, worker, wakes)
}

#[test]
fn cancelled_render_is_skipped_and_order_is_kept() {
    let (_d, w, wakes) = opened(3);
    let cancelled = CancelToken::new();
    cancelled.cancel();
    w.send(Request::Render {
        page: 0,
        scale: 1.0,
        invert: false,
        rotation: Rotation::R0,
        cancel: cancelled,
    });
    w.send(Request::Render {
        page: 1,
        scale: 1.0,
        invert: false,
        rotation: Rotation::R0,
        cancel: CancelToken::new(),
    });
    match recv(&w) {
        Response::Rendered { page, .. } => assert_eq!(page, 1),
        other => panic!("esperava Rendered da página 1, veio {other:?}"),
    }
    // o worker chama wake() depois de enviar a resposta: espera o contador com prazo
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while wakes.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(wakes.load(Ordering::SeqCst) >= 2);
}

#[test]
fn search_streams_hits_then_done() {
    let (_d, w, _) = opened(3);
    w.send(Request::Search {
        id: 7,
        needle: "Hello".into(),
        cancel: CancelToken::new(),
    });
    let mut pages = vec![];
    loop {
        match recv(&w) {
            Response::SearchHits { id, page, rects } => {
                assert_eq!(id, 7);
                assert_eq!(rects.len(), 1);
                pages.push(page);
            }
            Response::SearchDone { id } => {
                assert_eq!(id, 7);
                break;
            }
            other => panic!("inesperado: {other:?}"),
        }
    }
    assert_eq!(pages, [0, 1, 2]);
}

#[test]
fn words_save_and_failed_open() {
    let (dir, w, _) = opened(1);
    let path = dir.path().join("a.pdf");
    w.send(Request::Words {
        tag: 3,
        page: 0,
        area: PdfRect::new(0.0, 0.0, 612.0, 792.0),
    });
    let rects = match recv(&w) {
        Response::Words { tag, words, .. } => {
            assert_eq!(tag, 3);
            assert_eq!(words.len(), 4);
            words
                .iter()
                .filter(|x| x.text == "Hello")
                .map(|x| x.rect)
                .collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    w.send(Request::Save {
        pending: vec![NewAnnot::Highlight { page: 0, rects }],
        dest: path,
    });
    match recv(&w) {
        Response::Saved { annots } => assert_eq!(annots.len(), 1),
        other => panic!("{other:?}"),
    }
    w.send(Request::Save {
        pending: vec![NewAnnot::Note {
            page: 0,
            at: (1.0, 1.0),
            text: "x".into(),
        }],
        dest: dir.path().join("nada").join("x.pdf"),
    });
    assert!(matches!(recv(&w), Response::SaveFailed(_)));
    w.send(Request::Open {
        path: dir.path().join("nao_existe.pdf"),
        password: None,
    });
    assert!(matches!(recv(&w), Response::OpenFailed(PdfError::Open(_))));
}

#[test]
fn render_is_served_while_a_long_search_is_running() {
    let (_d, w, _) = opened(300);
    w.send(Request::Search {
        id: 1,
        needle: "Hello".into(),
        cancel: CancelToken::new(),
    });
    w.send(Request::Render {
        page: 0,
        scale: 0.5,
        invert: false,
        rotation: Rotation::R0,
        cancel: CancelToken::new(),
    });
    let mut rendered_before_done = false;
    loop {
        match recv(&w) {
            Response::Rendered { .. } => rendered_before_done = true,
            Response::SearchDone { .. } => break,
            Response::SearchHits { .. } => {}
            other => panic!("inesperado: {other:?}"),
        }
    }
    assert!(
        rendered_before_done,
        "o render ficou preso atrás da busca inteira"
    );
}

#[test]
fn selection_text_and_properties_through_the_worker() {
    let (_d, w, _) = opened(2);
    w.send(Request::SelectText {
        tag: 7,
        page: 0,
        a: (0.0, 0.0),
        b: (10_000.0, 10_000.0),
    });
    match recv(&w) {
        Response::Selection {
            tag,
            page,
            selection,
        } => {
            assert_eq!((tag, page), (7, 0));
            assert!(selection.text.contains("Hello"), "{:?}", selection.text);
        }
        other => panic!("{other:?}"),
    }
    w.send(Request::DocumentText { tag: 8 });
    match recv(&w) {
        Response::DocumentText { tag, text } => {
            assert_eq!(tag, 8);
            assert!(text.contains("Page 2"));
        }
        other => panic!("{other:?}"),
    }
    w.send(Request::Properties);
    match recv(&w) {
        Response::Properties(p) => assert_eq!(p.pages, 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn export_for_print_through_the_worker() {
    let (dir, w, _) = opened(1);
    let dest = dir.path().join("imprimir.pdf");
    w.send(Request::ExportForPrint {
        pending: vec![],
        dest: dest.clone(),
        with_annots: true,
    });
    match recv(&w) {
        Response::Exported { dest: d } => assert_eq!(d, dest),
        other => panic!("{other:?}"),
    }
    assert!(dest.exists());
}
