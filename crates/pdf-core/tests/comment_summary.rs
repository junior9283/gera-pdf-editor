mod common;
use common::write_sample;
use pdf_core::{
    summarize_comments, AnnotOp, NewAnnot, PdfDoc, PdfRect, SummaryFont, SummaryLayout,
    SummaryOptions, SummarySort,
};

/// 2 páginas; na 1, uma nota de "Zé" (com resposta de "Ana") e um destaque de "Ana".
fn commented(dir: &tempfile::TempDir, rotate: i32) -> PdfDoc {
    let path = write_sample(dir, "doc.pdf", 2, rotate);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_author(Some("Zé".into()));
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (100.0, 100.0),
        text: "primeira nota".into(),
    }))
    .unwrap();
    let note = d.annotations(0).unwrap()[0].id;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    d.set_author(Some("Ana".into()));
    d.apply(AnnotOp::Reply {
        page: 0,
        parent: note,
        text: "resposta da Ana".into(),
    })
    .unwrap();
    d.apply(AnnotOp::Create(NewAnnot::Highlight {
        page: 0,
        rects: vec![PdfRect::new(100.0, 300.0, 200.0, 314.0)],
    }))
    .unwrap();
    d
}

fn opts(layout: SummaryLayout) -> SummaryOptions {
    SummaryOptions {
        layout,
        sort: SummarySort::Page,
        only: None,
        pages: None,
        font: SummaryFont::Medium,
        include_empty: false,
    }
}

/// Páginas e o texto de cada uma.
fn read(path: &std::path::Path) -> Vec<(f32, f32, String)> {
    let doc = mupdf::Document::open(path.to_str().unwrap()).unwrap();
    (0..doc.page_count().unwrap())
        .map(|k| {
            let page = doc.load_page(k).unwrap();
            let b = page.bounds().unwrap();
            let text = page
                .to_text_page(mupdf::TextPageFlags::empty())
                .unwrap()
                .to_text()
                .unwrap();
            (b.width(), b.height(), text)
        })
        .collect()
}

#[test]
fn each_layout_has_the_expected_pages_and_text() {
    let dir = tempfile::tempdir().unwrap();
    let d = commented(&dir, 0);
    for (layout, pages) in [
        (SummaryLayout::CommentsOnly, 1),
        (SummaryLayout::ConnectorSeparate, 2),
        (SummaryLayout::ConnectorSingle, 1),
        (SummaryLayout::NumbersSeparate, 2),
    ] {
        let out = dir.path().join(format!("{layout:?}.pdf"));
        assert_eq!(summarize_comments(&d, &opts(layout), &out).unwrap(), 2);
        let got = read(&out);
        assert_eq!(got.len(), pages, "{layout:?}");
        let all: String = got.iter().map(|(_, _, t)| t.as_str()).collect();
        for needle in ["primeira nota", "resposta da Ana", "Destaque", "Zé"] {
            assert!(all.contains(needle), "{layout:?} sem {needle}: {all}");
        }
        if layout == SummaryLayout::ConnectorSingle {
            assert!(
                (got[0].0 - 2.0 * 612.0).abs() < 1.0,
                "página e coluna lado a lado"
            );
        }
    }
}

#[test]
fn sort_by_author_and_date() {
    let dir = tempfile::tempdir().unwrap();
    let d = commented(&dir, 0);
    let text = |sort| {
        let out = dir.path().join("s.pdf");
        let o = SummaryOptions {
            sort,
            ..opts(SummaryLayout::CommentsOnly)
        };
        summarize_comments(&d, &o, &out).unwrap();
        read(&out).remove(0).2
    };
    let by_author = text(SummarySort::Author);
    assert!(by_author.find("Destaque").unwrap() < by_author.find("primeira nota").unwrap());
    let by_date = text(SummarySort::Date);
    assert!(by_date.find("primeira nota").unwrap() < by_date.find("Destaque").unwrap());
}

#[test]
fn rotated_pages_keep_their_shape() {
    let dir = tempfile::tempdir().unwrap();
    let d = commented(&dir, 90);
    let out = dir.path().join("r.pdf");
    summarize_comments(&d, &opts(SummaryLayout::ConnectorSingle), &out).unwrap();
    let got = read(&out);
    assert!(
        (got[0].0 - 2.0 * 792.0).abs() < 1.0,
        "{:?}",
        (got[0].0, got[0].1)
    );
    assert!((got[0].1 - 612.0).abs() < 1.0);
}

#[test]
fn empty_pages_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let d = commented(&dir, 0);
    let out = dir.path().join("e.pdf");
    let o = SummaryOptions {
        include_empty: true,
        ..opts(SummaryLayout::ConnectorSeparate)
    };
    summarize_comments(&d, &o, &out).unwrap();
    assert_eq!(read(&out).len(), 3, "a página 2 entra sozinha");
    let o = SummaryOptions {
        pages: Some((1, 1)),
        ..opts(SummaryLayout::CommentsOnly)
    };
    assert!(summarize_comments(&d, &o, &out).is_err(), "nada a resumir");
}

#[test]
fn a_long_thread_stays_inside_the_pages() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = commented(&dir, 0);
    let note = d.annotations(0).unwrap()[0].id;
    for k in 0..120 {
        d.apply(AnnotOp::Reply {
            page: 0,
            parent: note,
            text: format!("resposta numero {k}"),
        })
        .unwrap();
    }
    for layout in [
        SummaryLayout::CommentsOnly,
        SummaryLayout::ConnectorSeparate,
    ] {
        let out = dir.path().join("longo.pdf");
        summarize_comments(&d, &opts(layout), &out).unwrap();
        let doc = mupdf::Document::open(out.to_str().unwrap()).unwrap();
        let mut seen = String::new();
        for k in 0..doc.page_count().unwrap() {
            let page = doc.load_page(k).unwrap();
            let b = page.bounds().unwrap();
            let tp = page.to_text_page(mupdf::TextPageFlags::empty()).unwrap();
            for block in tp.blocks() {
                let r = block.bounds();
                if r.y0 >= b.y0 - 1.0 && r.y1 <= b.y1 + 1.0 {
                    for line in block.lines() {
                        seen.extend(line.chars().filter_map(|c| c.char()));
                        seen.push('\n');
                    }
                }
            }
        }
        assert!(
            seen.contains("resposta numero 119"),
            "{layout:?}: texto fora da página"
        );
    }
}

#[test]
fn documents_that_forbid_copying_only_summarize_comments() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = commented(&dir, 0);
    d.set_protection(Some(pdf_core::Protection {
        open_password: String::new(),
        owner_password: "dono".into(),
        permissions: pdf_core::Permissions {
            copy: false,
            ..pdf_core::Permissions::ALL
        },
    }))
    .unwrap();
    let path = dir.path().join("protegido.pdf");
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let out = dir.path().join("r.pdf");
    assert!(summarize_comments(&d, &opts(SummaryLayout::ConnectorSingle), &out).is_err());
    assert!(summarize_comments(&d, &opts(SummaryLayout::CommentsOnly), &out).is_ok());
}
