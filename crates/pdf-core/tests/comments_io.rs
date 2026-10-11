mod common;
use common::write_sample;
use pdf_core::{AnnotInfo, AnnotKind, AnnotOp, CommentFormat, NewAnnot, PdfDoc, PdfRect};

/// Documento de 2 páginas com nota (e resposta), destaque e desenho, de autores diferentes.
fn commented(dir: &tempfile::TempDir) -> PdfDoc {
    let path = write_sample(dir, "fonte.pdf", 2, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_author(Some("Ana".into()));
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (100.0, 100.0),
        text: "revisar este parágrafo".into(),
    }))
    .unwrap();
    let note = d.annotations(0).unwrap()[0].id;
    d.set_author(Some("Bruno".into()));
    d.apply(AnnotOp::Reply {
        page: 0,
        parent: note,
        text: "feito ✓".into(),
    })
    .unwrap();
    d.apply(AnnotOp::Create(NewAnnot::Highlight {
        page: 1,
        rects: vec![PdfRect::new(100.0, 200.0, 220.0, 214.0)],
    }))
    .unwrap();
    d.apply(AnnotOp::Create(NewAnnot::Ink {
        page: 1,
        strokes: vec![vec![(50.0, 300.0), (90.0, 320.0), (130.0, 300.0)]],
    }))
    .unwrap();
    d
}

/// O que importa comparar: tipo, página, texto, autor, cor e se é resposta.
type Seen = (AnnotKind, i32, String, String, Option<[f32; 3]>, bool);

fn summary(d: &PdfDoc) -> Vec<Seen> {
    let mut out: Vec<_> = (0..d.page_count().unwrap())
        .flat_map(|p| d.annotations(p).unwrap())
        .map(|a: AnnotInfo| {
            (
                a.kind,
                a.page,
                a.contents,
                a.author,
                a.color.map(|c| c.map(|v| (v * 100.0).round() / 100.0)),
                a.reply_to.is_some(),
            )
        })
        .collect();
    out.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    out
}

fn blank(dir: &tempfile::TempDir, pages: usize) -> PdfDoc {
    PdfDoc::open(&write_sample(dir, "alvo.pdf", pages, 0), None).unwrap()
}

#[test]
fn fdf_round_trip_keeps_kind_page_text_author_color_and_replies() {
    let dir = tempfile::tempdir().unwrap();
    let src = commented(&dir);
    let fdf = dir.path().join("c.fdf");
    assert_eq!(
        src.export_comments(&fdf, CommentFormat::Fdf, None).unwrap(),
        4
    );
    assert!(std::fs::read(&fdf).unwrap().starts_with(b"%FDF-1.2"));
    let mut dst = blank(&dir, 2);
    let report = dst.import_comments(&fdf).unwrap();
    assert_eq!((report.added, report.skipped), (4, 0));
    assert_eq!(summary(&dst), summary(&src));
    let reply = dst
        .annotations(0)
        .unwrap()
        .into_iter()
        .find(|a| a.reply_to.is_some())
        .unwrap();
    let note = dst.annotations(0).unwrap()[0].id;
    assert_eq!(
        reply.reply_to,
        Some(note),
        "a resposta aponta para a nota importada"
    );
}

#[test]
fn export_only_the_selected_comment_takes_its_replies() {
    let dir = tempfile::tempdir().unwrap();
    let src = commented(&dir);
    let note = src.annotations(0).unwrap()[0].id;
    let fdf = dir.path().join("sel.fdf");
    assert_eq!(
        src.export_comments(&fdf, CommentFormat::Fdf, Some(&[(0, note)]))
            .unwrap(),
        2,
        "a nota e a resposta"
    );
}

#[test]
fn import_from_another_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let mut src = commented(&dir);
    let saved = dir.path().join("comentado.pdf");
    src.save(&saved).unwrap();
    let mut dst = blank(&dir, 2);
    assert_eq!(dst.import_comments(&saved).unwrap().added, 4);
    assert_eq!(summary(&dst), summary(&src));
}

#[test]
fn import_skips_missing_pages_and_counts_them() {
    let dir = tempfile::tempdir().unwrap();
    let src = commented(&dir);
    let fdf = dir.path().join("c.fdf");
    src.export_comments(&fdf, CommentFormat::Fdf, None).unwrap();
    let mut dst = blank(&dir, 1);
    let report = dst.import_comments(&fdf).unwrap();
    assert_eq!((report.added, report.skipped), (2, 2));
}

#[test]
fn import_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = commented(&dir);
    let fdf = dir.path().join("c.fdf");
    src.export_comments(&fdf, CommentFormat::Fdf, None).unwrap();
    let mut dst = blank(&dir, 2);
    dst.import_comments(&fdf).unwrap();
    dst.undo().unwrap();
    assert!(summary(&dst).is_empty());
}

#[test]
fn garbage_is_refused_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("lixo.fdf");
    std::fs::write(&bad, b"%FDF-1.2\nisto nao e um fdf").unwrap();
    let mut dst = blank(&dir, 1);
    assert!(dst.import_comments(&bad).is_err());
    assert!(summary(&dst).is_empty());
}
