mod common;
use common::write_sample;
use pdf_core::{
    AnnotOp, CancelToken, NewAnnot, PdfError, PdfRect, Request, Response, Rotation, Worker,
};
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
fn save_and_failed_open() {
    let (dir, w, _) = opened(1);
    let path = dir.path().join("a.pdf");
    w.send(Request::Annot(AnnotOp::Create(NewAnnot::Highlight {
        page: 0,
        rects: vec![PdfRect::new(70.0, 90.0, 200.0, 110.0)],
    })));
    assert!(matches!(recv(&w), Response::AnnotsChanged { .. }));
    w.send(Request::Save { dest: path });
    match recv(&w) {
        Response::Saved { annots, history } => {
            assert_eq!(annots.len(), 1);
            assert!(!history.dirty && !history.can_undo);
        }
        other => panic!("{other:?}"),
    }
    w.send(Request::Save {
        dest: dir.path().join("nada").join("x.pdf"),
    });
    assert!(matches!(recv(&w), Response::SaveFailed { .. }));
    w.send(Request::Open {
        path: dir.path().join("nao_existe.pdf"),
        password: None,
    });
    assert!(matches!(recv(&w), Response::OpenFailed(PdfError::Open(_))));
}

#[test]
fn annotation_edits_through_the_worker() {
    let (_d, w, _) = opened(1);
    w.send(Request::Annot(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (50.0, 50.0),
        text: "x".into(),
    })));
    let id = match recv(&w) {
        Response::AnnotsChanged {
            annots,
            pages,
            history,
        } => {
            assert_eq!(pages, vec![0]);
            assert!(history.can_undo && history.dirty);
            annots[0].id
        }
        other => panic!("{other:?}"),
    };
    w.send(Request::Undo);
    assert!(
        matches!(recv(&w), Response::AnnotsChanged { ref annots, history, .. } if annots.is_empty() && history.can_redo)
    );
    w.send(Request::Annot(AnnotOp::Delete {
        page: 0,
        id: id + 1000,
    }));
    assert!(matches!(recv(&w), Response::AnnotFailed(_)));
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
        dest: dest.clone(),
        with_annots: true,
    });
    match recv(&w) {
        Response::Exported { dest: d } => assert_eq!(d, dest),
        other => panic!("{other:?}"),
    }
    assert!(dest.exists());
}

#[test]
fn a_burst_of_selection_requests_is_answered_by_the_latest() {
    // 50 pedidos chegam enquanto o worker ainda abre o documento: só os últimos importam
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 3, 0);
    let w = Worker::spawn(|| {});
    w.send(Request::Open {
        path,
        password: None,
    });
    for tag in 1..=50 {
        w.send(Request::SelectText {
            tag,
            page: 0,
            a: (0.0, 0.0),
            b: (10_000.0, 10_000.0),
        });
    }
    let mut tags = Vec::new();
    loop {
        match recv(&w) {
            Response::Selection { tag, .. } => {
                tags.push(tag);
                if tag == 50 {
                    break;
                }
            }
            Response::Opened(_) => {}
            other => panic!("{other:?}"),
        }
    }
    assert!(
        tags.len() < 50,
        "pedidos antigos foram juntados: {}",
        tags.len()
    );
}

#[test]
fn filling_a_field_reports_every_field_of_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.pdf");
    std::fs::write(&path, pdf_core::sample::form_pdf()).unwrap();
    let w = Worker::spawn(|| {});
    w.send(Request::Open {
        path: path.clone(),
        password: None,
    });
    assert!(matches!(recv(&w), Response::Opened(_)));
    let all = pdf_core::PdfDoc::open(&path, None)
        .unwrap()
        .all_fields()
        .unwrap();
    let nome = all.iter().find(|f| f.name == "nome").unwrap();
    w.send(Request::SetField {
        page: 0,
        id: nome.id,
        value: "Bia".into(),
    });
    match recv(&w) {
        Response::FieldsChanged { fields, .. } => {
            assert_eq!(fields.len(), all.len());
            assert!(fields.iter().any(|f| f.name == "nome" && f.value == "Bia"));
        }
        other => panic!("esperava FieldsChanged, veio {other:?}"),
    }
}
