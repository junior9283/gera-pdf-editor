mod common;
use common::write_sample;
use pdf_core::{
    sample::pdf_from_objects, Duplex, InitialLayout, InitialPanel, InitialView, InitialZoom,
    PdfDoc, PrintPrefs, PropertiesEdit, WindowOptions,
};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 3, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn edit_from(d: &PdfDoc) -> PropertiesEdit {
    PropertiesEdit::from(&d.properties().unwrap())
}

fn saved_and_reopened(dir: &tempfile::TempDir, d: &mut PdfDoc) -> PdfDoc {
    let out = dir.path().join("salvo.pdf");
    d.save(&out).unwrap();
    PdfDoc::open(&out, None).unwrap()
}

#[test]
fn description_and_custom_round_trip() {
    let (dir, _p, mut d) = doc();
    let mut e = edit_from(&d);
    e.title = "Relatório anual".into();
    e.author = "Ana Lúcia".into();
    e.subject = "Contas".into();
    e.keywords = "pdf, teste".into();
    e.custom = vec![("Departamento".into(), "Financeiro".into())];
    d.set_properties(&e).unwrap();
    assert!(d.is_dirty());
    let p = saved_and_reopened(&dir, &mut d).properties().unwrap();
    assert_eq!(
        (p.title.as_str(), p.author.as_str()),
        ("Relatório anual", "Ana Lúcia")
    );
    assert_eq!(
        (p.subject.as_str(), p.keywords.as_str()),
        ("Contas", "pdf, teste")
    );
    assert_eq!(
        p.custom,
        vec![("Departamento".to_string(), "Financeiro".to_string())]
    );
    // vazio apaga
    let mut e = PropertiesEdit::from(&p);
    e.subject.clear();
    e.custom.clear();
    let (dir2, _p2, mut d2) = doc();
    d2.set_properties(&e).unwrap();
    let p2 = saved_and_reopened(&dir2, &mut d2).properties().unwrap();
    assert_eq!(p2.subject, "");
    assert!(p2.custom.is_empty());
}

#[test]
fn standard_names_are_refused_as_custom() {
    let (_t, _p, mut d) = doc();
    for name in ["Title", "Producer", "ModDate", "", &"x".repeat(200)] {
        let mut e = edit_from(&d);
        e.custom = vec![(name.to_string(), "v".into())];
        assert!(d.set_properties(&e).is_err(), "{name:?}");
    }
    assert!(!d.is_dirty(), "recusado não muda nada");
}

#[test]
fn initial_view_and_print_prefs_round_trip() {
    let (dir, _p, mut d) = doc();
    let mut e = edit_from(&d);
    e.initial = InitialView {
        panel: Some(InitialPanel::Bookmarks),
        full_screen: false,
        layout: Some(InitialLayout::TwoPage { cover: true }),
        page: Some(2),
        zoom: Some(InitialZoom::Factor(1.5)),
    };
    e.window = WindowOptions {
        fit_window: true,
        center_window: true,
        display_title: true,
        hide_menubar: false,
        hide_toolbar: true,
        hide_window_ui: false,
    };
    e.lang = "pt-BR".into();
    e.print = PrintPrefs {
        scaling_none: true,
        duplex: Some(Duplex::LongEdge),
        copies: Some(3),
        ranges: vec![(1, 2)],
    };
    d.set_properties(&e).unwrap();
    let p = saved_and_reopened(&dir, &mut d).properties().unwrap();
    assert_eq!(p.initial, e.initial);
    assert_eq!(p.window, e.window);
    assert_eq!(p.lang, "pt-BR");
    assert_eq!(p.print, e.print);
}

#[test]
fn xmp_follows_the_title_and_keeps_pdfa_id() {
    let dir = tempfile::tempdir().unwrap();
    let xmp = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"><pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/Metadata 4 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        format!(
            "<</Type/Metadata/Subtype/XML/Length {}>>\nstream\n{xmp}\nendstream",
            xmp.len()
        ),
    ];
    let path = dir.path().join("pdfa.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let mut e = edit_from(&d);
    e.title = "Título <novo> & cia".into();
    d.set_properties(&e).unwrap();
    let out = dir.path().join("depois.pdf");
    d.save(&out).unwrap();
    let x = PdfDoc::open(&out, None).unwrap().xmp().unwrap();
    assert!(x.contains("Título &lt;novo&gt; &amp; cia"), "{x}");
    assert!(x.contains("<pdfaid:part>2</pdfaid:part>"), "{x}");
    assert!(
        x.contains("<pdfaid:conformance>B</pdfaid:conformance>"),
        "{x}"
    );
}

#[test]
fn undo_restores_the_old_properties() {
    let (_t, _p, mut d) = doc();
    let before = d.properties().unwrap();
    let mut e = edit_from(&d);
    e.title = "Novo".into();
    e.lang = "en".into();
    d.set_properties(&e).unwrap();
    d.undo().unwrap();
    let after = d.properties().unwrap();
    assert_eq!((after.title, after.lang), (before.title, before.lang));
}

#[test]
fn fonts_are_listed_with_embedding() {
    let (_t, _p, d) = doc();
    let fonts = d.properties().unwrap().fonts;
    let helv = fonts
        .iter()
        .find(|f| f.name.contains("Helvetica"))
        .expect("Helvetica");
    assert_eq!(helv.kind, "Type1");
    assert!(!helv.embedded && !helv.subset);
    assert_eq!(
        fonts
            .iter()
            .filter(|f| f.name.contains("Helvetica"))
            .count(),
        1,
        "sem repetir"
    );
}
