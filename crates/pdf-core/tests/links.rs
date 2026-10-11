use pdf_core::{InitialLayout, InitialPanel, InitialZoom, LinkTarget, PdfDoc};
use std::path::{Path, PathBuf};

/// Escreve um PDF a partir dos corpos dos objetos (o objeto `i` é `objs[i - 1]`).
fn raw_pdf(dir: &Path, name: &str, objs: &[String]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(objs)).unwrap();
    path
}

/// Duas páginas 612×792; a primeira tem um link por ação de `actions` (cada uma, uma faixa de
/// 20 pt de altura, de cima para baixo). `catalog_extra` entra no catálogo.
fn links_pdf(dir: &Path, actions: &[&str], catalog_extra: &str) -> PathBuf {
    // 1 catálogo, 2 páginas, 3 e 4 páginas, 5 árvore de nomes, 6.. links
    let annots: Vec<String> = (0..actions.len())
        .map(|i| format!("{} 0 R", 6 + i))
        .collect();
    let mut objs = vec![
        format!("<</Type/Catalog/Pages 2 0 R/Names<</Dests 5 0 R>>{catalog_extra}>>"),
        "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_string(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[{}]>>",
            annots.join(" ")
        ),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Names[(cap1)[4 0 R/Fit]]>>".to_string(),
    ];
    for (i, a) in actions.iter().enumerate() {
        let top = 780 - 30 * i as i32;
        objs.push(format!(
            "<</Type/Annot/Subtype/Link/Rect[10 {} 110 {top}]/Border[0 0 0]{a}>>",
            top - 20
        ));
    }
    raw_pdf(dir, "links.pdf", &objs)
}

fn targets(path: &Path) -> Vec<LinkTarget> {
    let doc = PdfDoc::open(path, None).unwrap();
    doc.links(0)
        .unwrap()
        .into_iter()
        .map(|l| l.target)
        .collect()
}

#[test]
fn link_to_page_resolves_point() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(dir.path(), &["/Dest[4 0 R/XYZ 100 700 0]"], "");
    let doc = PdfDoc::open(&path, None).unwrap();
    let links = doc.links(0).unwrap();
    assert_eq!(links.len(), 1);
    // retângulo em coordenadas do MuPDF (y para baixo)
    let r = links[0].rect;
    assert!(
        (r.x0 - 10.0).abs() < 0.5 && (r.y0 - 12.0).abs() < 0.5,
        "{r:?}"
    );
    match &links[0].target {
        LinkTarget::Page {
            page,
            at: Some((x, y)),
        } => {
            assert_eq!(*page, 1);
            assert!(
                (x - 100.0).abs() < 0.5 && (y - 92.0).abs() < 0.5,
                "({x}, {y})"
            );
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn named_destination_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/Dest(cap1)",
            "/A<</S/GoTo/D(cap1)>>",
            "/A<</S/GoTo/D[4 0 R/Fit]>>",
        ],
        "",
    );
    for t in targets(&path) {
        assert!(matches!(t, LinkTarget::Page { page: 1, .. }), "{t:?}");
    }
}

#[test]
fn web_link_is_uri_and_javascript_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/A<</S/URI/URI(https://example.com/a?b=1)>>",
            "/A<</S/URI/URI(mailto:alguem@example.com)>>",
            "/A<</S/URI/URI(javascript:app.alert\\(1\\))>>",
            "/A<</S/URI/URI(smb://servidor/x)>>",
        ],
        "",
    );
    let t = targets(&path);
    assert_eq!(t.len(), 4, "{t:?}");
    assert_eq!(t[0], LinkTarget::Uri("https://example.com/a?b=1".into()));
    assert_eq!(t[1], LinkTarget::Uri("mailto:alguem@example.com".into()));
    assert!(matches!(t[2], LinkTarget::Blocked(_)), "{:?}", t[2]);
    assert!(matches!(t[3], LinkTarget::Blocked(_)), "{:?}", t[3]);
}

#[test]
fn remote_pdf_link_is_absolute_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/A<</S/GoToR/F(outro.pdf)/D[1/Fit]>>",
            "/A<</S/Launch/F<</Type/Filespec/F(sub/doc.pdf)>>>>",
        ],
        "",
    );
    let t = targets(&path);
    assert_eq!(
        t[0],
        LinkTarget::File {
            path: dir.path().join("outro.pdf"),
            page: Some(1)
        }
    );
    assert_eq!(
        t[1],
        LinkTarget::File {
            path: dir.path().join("sub/doc.pdf"),
            page: Some(0)
        }
    );
}

#[test]
fn launch_of_program_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/A<</S/Launch/F(calc.exe)>>",
            "/A<</S/Launch/Win<</F(cmd.exe)>>>>",
        ],
        "",
    );
    for t in targets(&path) {
        assert!(matches!(t, LinkTarget::Blocked(_)), "{t:?}");
    }
}

#[test]
fn link_to_missing_page_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(dir.path(), &["/Dest(naoexiste)", "/Dest[7/Fit]"], "");
    assert_eq!(targets(&path), vec![]);
}

#[test]
fn malformed_link_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/A<<>>", "/A 5", "/Rect 3", "/Dest[4 0 R/Fit]"],
        "",
    );
    let t = targets(&path);
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(matches!(t[0], LinkTarget::Page { page: 1, .. }));
}

#[test]
fn summary_carries_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/Dest[4 0 R/Fit]", "/A<</S/URI/URI(https://a.b)>>"],
        "",
    );
    let s = PdfDoc::open(&path, None).unwrap().summary().unwrap();
    assert_eq!(s.links.len(), 2);
    assert!(s.links.iter().all(|l| l.page == 0));
}

fn initial(catalog_extra: &str) -> pdf_core::InitialView {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(dir.path(), &[], catalog_extra);
    PdfDoc::open(&path, None)
        .unwrap()
        .summary()
        .unwrap()
        .initial_view
}

#[test]
fn initial_view_reads_mode_layout_and_open_action() {
    let v = initial("/PageMode/UseOutlines/PageLayout/TwoPageRight/OpenAction[4 0 R/FitH 500]");
    assert_eq!(v.panel, Some(InitialPanel::Bookmarks));
    assert!(!v.full_screen);
    assert_eq!(v.layout, Some(InitialLayout::TwoPage { cover: true }));
    assert_eq!(v.page, Some(1));
    assert_eq!(v.zoom, Some(InitialZoom::FitWidth));

    let v = initial(
        "/PageMode/FullScreen/PageLayout/OneColumn/OpenAction<</S/GoTo/D[3 0 R/XYZ 0 792 1.5]>>",
    );
    assert!(v.full_screen);
    assert_eq!(v.layout, Some(InitialLayout::OneColumn));
    assert_eq!((v.page, v.zoom), (Some(0), Some(InitialZoom::Factor(1.5))));

    let v = initial("/PageMode/UseThumbs/PageLayout/TwoColumnLeft/OpenAction[3 0 R/Fit]");
    assert_eq!(v.panel, Some(InitialPanel::Thumbnails));
    assert_eq!(v.layout, Some(InitialLayout::TwoColumn { cover: false }));
    assert_eq!(v.zoom, Some(InitialZoom::FitPage));

    let v = initial("/PageLayout/SinglePage/OpenAction(cap1)");
    assert_eq!((v.layout, v.page), (Some(InitialLayout::Single), Some(1)));
}

#[test]
fn initial_view_ignores_junk() {
    let v = initial("/PageMode 3/PageLayout/Nada/OpenAction[99 0 R/XYZ 0 0 -2]");
    assert_eq!(v, pdf_core::InitialView::default());
    assert_eq!(
        initial("/OpenAction<</S/JavaScript/JS(app.alert(1))>>"),
        pdf_core::InitialView::default()
    );
}

#[test]
fn giant_uri_is_blocked_and_not_kept_whole() {
    let dir = tempfile::tempdir().unwrap();
    let huge = format!("https://example.com/{}", "a".repeat(100_000));
    let path = links_pdf(dir.path(), &[&format!("/A<</S/URI/URI({huge})>>")], "");
    match &targets(&path)[..] {
        [LinkTarget::Blocked(shown)] => assert!(shown.len() <= 300, "{}", shown.len()),
        t => panic!(
            "{} alvos, o primeiro com {:?} bytes",
            t.len(),
            t.first().map(|x| format!("{x:?}").len())
        ),
    }
}

#[test]
fn thousands_of_links_open_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let many: Vec<String> = (0..5000)
        .map(|i| format!("/A<</S/URI/URI(https://e.com/{i})>>"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let path = links_pdf(dir.path(), &refs, "");
    let t = std::time::Instant::now();
    let s = PdfDoc::open(&path, None).unwrap().summary().unwrap();
    assert_eq!(s.links.len(), 5000);
    assert!(t.elapsed().as_secs_f32() < 5.0, "{:?}", t.elapsed());
}

#[test]
fn corrupt_annots_do_not_break_opening() {
    // /Annots como dicionário, número ou em ciclo: a abertura segue, sem links
    for annots in ["/Annots<</A 1>>", "/Annots 7", "/Annots 3 0 R"] {
        let dir = tempfile::tempdir().unwrap();
        let objs = vec![
            "<</Type/Catalog/Pages 2 0 R>>".to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]{annots}>>"),
        ];
        let path = raw_pdf(dir.path(), "c.pdf", &objs);
        let s = PdfDoc::open(&path, None).unwrap().summary().unwrap();
        assert!(s.links.is_empty(), "{annots}");
    }
}
