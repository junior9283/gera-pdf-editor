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
    assert!(matches!(recv(&w), Response::EditFailed(_)));
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

#[test]
fn preparing_a_form_reports_the_new_fields() {
    let (_d, w, _) = opened(1);
    w.send(Request::Prepare(pdf_core::PrepareOp::Add {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 250.0, 122.0),
        kind: pdf_core::NewFieldKind::Text,
        name: "Nome".into(),
    }));
    match recv(&w) {
        Response::FieldsChanged { fields, history } => {
            assert!(fields.iter().any(|f| f.name == "Nome"));
            assert!(history.can_undo);
        }
        other => panic!("esperava FieldsChanged, veio {other:?}"),
    }
    w.send(Request::Prepare(pdf_core::PrepareOp::Add {
        page: 0,
        rect: PdfRect::new(100.0, 200.0, 250.0, 222.0),
        kind: pdf_core::NewFieldKind::Text,
        name: "Nome".into(),
    }));
    assert!(matches!(recv(&w), Response::EditFailed(_)), "nome repetido");
}

#[test]
fn field_properties_come_from_the_worker() {
    let (_d, w, _) = opened(1);
    w.send(Request::Prepare(pdf_core::PrepareOp::Add {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 250.0, 122.0),
        kind: pdf_core::NewFieldKind::Text,
        name: "Nome".into(),
    }));
    let Response::FieldsChanged { fields, .. } = recv(&w) else {
        panic!()
    };
    let id = fields[0].id;
    w.send(Request::FieldProps { page: 0, id });
    match recv(&w) {
        Response::FieldProps {
            page: 0,
            id: got,
            props: Ok(p),
        } => {
            assert_eq!((got, p.name.as_str()), (id, "Nome"));
        }
        other => panic!("esperava FieldProps, veio {other:?}"),
    }
}

#[test]
fn signing_through_the_worker_reports_the_new_signature() {
    let (dir, w, _) = opened(1);
    let id = pdf_core::DigitalId::create_self_signed("Bia", "", "", "x", &dir.path().join("b.pfx"))
        .unwrap();
    w.send(Request::SignDigitally {
        place: pdf_core::SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        id: Box::new(id),
        reason: "Ok".into(),
        location: String::new(),
    });
    assert!(matches!(recv(&w), Response::Signed { .. }));
    w.send(Request::Signatures);
    match recv(&w) {
        Response::Signatures(s) => {
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].integrity, pdf_core::Integrity::Intact);
        }
        other => panic!("esperava Signatures, veio {other:?}"),
    }
}

#[test]
fn protection_and_unlock_through_the_worker() {
    let (_dir, w, _) = opened(1);
    w.send(Request::SetProtection(Some(pdf_core::Protection {
        open_password: String::new(),
        owner_password: "dono".into(),
        permissions: pdf_core::Permissions {
            print: false,
            print_hq: false,
            ..pdf_core::Permissions::ALL
        },
    })));
    match recv(&w) {
        Response::SecurityChanged {
            pending, history, ..
        } => assert!(pending && history.dirty),
        other => panic!("esperava SecurityChanged, veio {other:?}"),
    }
    // a proteção vale ao salvar; depois disso a senha errada não desbloqueia
    w.send(Request::Save {
        dest: _dir.path().join("a.pdf"),
    });
    assert!(matches!(recv(&w), Response::Saved { .. }));
    w.send(Request::Unlock {
        password: "x".into(),
    });
    assert!(matches!(recv(&w), Response::SecurityFailed(_)));
    w.send(Request::Unlock {
        password: "dono".into(),
    });
    assert!(matches!(recv(&w), Response::SecurityChanged { security, .. } if security.unlocked));
}

#[test]
fn compressing_through_the_worker_refuses_a_file_that_would_not_shrink() {
    // a amostra de texto já é mínima: o worker avisa em vez de gravar um arquivo maior
    let (dir, w, _) = opened(2);
    w.send(Request::Compress {
        level: pdf_core::CompressLevel::Low,
    });
    match recv(&w) {
        Response::CompressFailed(msg) => {
            assert!(msg.starts_with("o arquivo já está otimizado"), "{msg}");
            assert!(!dir.path().join("a (comprimido).pdf").exists());
        }
        other => panic!("esperava CompressFailed, veio {other:?}"),
    }
}

#[test]
fn recognizing_text_reports_progress_and_the_result() {
    // páginas com texto: puladas, mas o progresso e o resultado chegam
    let (_dir, w, _) = opened(2);
    w.send(Request::RecognizeText {
        pages: vec![0, 1],
        lang: pdf_core::OcrLanguage::English,
        cancel: CancelToken::new(),
    });
    let mut progress = 0;
    loop {
        match recv(&w) {
            Response::OcrProgress { done, total } => {
                assert_eq!(total, 2);
                assert!(done < 2);
                progress += 1;
            }
            Response::OcrDone { report, .. } => {
                assert_eq!((report.recognized, report.skipped), (0, 2));
                break;
            }
            other => panic!("inesperado: {other:?}"),
        }
    }
    assert_eq!(progress, 2);
}

#[test]
fn text_blocks_are_listed_and_edited_through_the_worker() {
    let (_d, w, _) = opened(1);
    w.send(Request::PageTextBlocks { page: 0 });
    let rect = match recv(&w) {
        Response::PageTextBlocks { page: 0, blocks } => {
            blocks
                .into_iter()
                .find(|b| b.text.contains("Hello Gera"))
                .expect("bloco do texto")
                .rect
        }
        other => panic!("esperava PageTextBlocks, veio {other:?}"),
    };
    w.send(Request::EditTextBlock {
        page: 0,
        rect,
        text: "Trocado".into(),
    });
    assert!(!matches!(recv(&w), Response::EditFailed(_)));
    w.send(Request::AddText {
        page: 0,
        at: (72.0, 300.0),
        text: "Novo".into(),
    });
    assert!(!matches!(recv(&w), Response::EditFailed(_)));
    w.send(Request::PageTextBlocks { page: 0 });
    match recv(&w) {
        Response::PageTextBlocks { blocks, .. } => {
            let all: Vec<_> = blocks.iter().map(|b| b.text.as_str()).collect();
            assert!(all.contains(&"Trocado") && all.contains(&"Novo"), "{all:?}");
        }
        other => panic!("esperava PageTextBlocks, veio {other:?}"),
    }
}

#[test]
fn watermark_through_the_worker() {
    let (_d, w, _) = opened(2);
    w.send(Request::AddPagination {
        config: Box::new(pdf_core::PaginationConfig::Watermark(pdf_core::Watermark {
            source: pdf_core::WatermarkSource::Text {
                text: "CONFIDENCIAL".into(),
                look: pdf_core::TextLook {
                    font: pdf_core::StdFont::Helvetica,
                    bold: false,
                    size: 40.0,
                    color: [0, 0, 0],
                },
            },
            rotation: 45.0,
            opacity: 0.5,
            scale: None,
            behind: false,
            vertical: (pdf_core::Align::Center, 0.0),
            horizontal: (pdf_core::Align::Center, 0.0),
            pages: pdf_core::PageSet::All,
        })),
        replace: false,
    });
    assert!(matches!(recv(&w), Response::PagesChanged { .. }));
    w.send(Request::PaginationState(pdf_core::Pagination::Watermark));
    match recv(&w) {
        Response::PaginationState {
            present: true,
            config: Some(pdf_core::PaginationConfig::Watermark(m)),
            ..
        } => assert_eq!(m.rotation, 45.0),
        other => panic!("esperava PaginationState, veio {other:?}"),
    }
    w.send(Request::RemovePagination(pdf_core::Pagination::Watermark));
    assert!(matches!(recv(&w), Response::PagesChanged { .. }));
    w.send(Request::PaginationState(pdf_core::Pagination::Watermark));
    assert!(matches!(
        recv(&w),
        Response::PaginationState { present: false, .. }
    ));
}
