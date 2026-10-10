mod common;
use common::write_sample;
use pdf_core::{
    Align, Bates, DateFormat, HeaderFooter, PageSet, Pagination, PaginationConfig, PdfDoc, StdFont,
    TextLook, Watermark, WatermarkSource,
};

fn look(size: f32) -> TextLook {
    TextLook {
        font: StdFont::Helvetica,
        bold: true,
        size,
        color: [200, 0, 0],
    }
}

fn text_mark(pages: PageSet) -> Watermark {
    Watermark {
        source: WatermarkSource::Text {
            text: "RASCUNHO".into(),
            look: look(48.0),
        },
        rotation: 0.0,
        opacity: 0.3,
        scale: None,
        behind: false,
        vertical: (Align::Center, 0.0),
        horizontal: (Align::Center, 0.0),
        pages,
    }
}

fn open(pages: usize, rotate: i32) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", pages, rotate);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, d)
}

fn has(d: &PdfDoc, page: i32, text: &str) -> bool {
    d.page_text(page).unwrap().contains(text)
}

#[test]
fn a_text_watermark_goes_on_every_page() {
    let (_d, mut d) = open(3, 0);
    d.add_watermark(&text_mark(PageSet::All), false).unwrap();
    for p in 0..3 {
        assert!(
            has(&d, p, "RASCUNHO") && has(&d, p, "Hello Gera"),
            "página {p}"
        );
    }
    assert!(d.pagination_state(Pagination::Watermark).unwrap().0);
}

#[test]
fn only_the_chosen_pages_get_it() {
    let (_d, mut d) = open(4, 0);
    d.add_watermark(&text_mark(PageSet::Range("2-3".into())), false)
        .unwrap();
    let marked: Vec<bool> = (0..4).map(|p| has(&d, p, "RASCUNHO")).collect();
    assert_eq!(marked, [false, true, true, false]);
    let (_d, mut d) = open(4, 0);
    d.add_watermark(&text_mark(PageSet::Even), false).unwrap();
    let marked: Vec<bool> = (0..4).map(|p| has(&d, p, "RASCUNHO")).collect();
    assert_eq!(marked, [false, true, false, true], "pares = 2 e 4");
    let (_d, mut d) = open(2, 0);
    assert!(d
        .add_watermark(&text_mark(PageSet::Range("5".into())), false)
        .is_err());
}

#[test]
fn removing_takes_every_mark_and_keeps_the_page() {
    let (_d, mut d) = open(3, 0);
    d.add_watermark(&text_mark(PageSet::All), false).unwrap();
    assert_eq!(d.remove_pagination(Pagination::Watermark).unwrap(), 3);
    for p in 0..3 {
        assert!(!has(&d, p, "RASCUNHO") && has(&d, p, "Hello Gera"));
    }
    assert!(!d.pagination_state(Pagination::Watermark).unwrap().0);
}

#[test]
fn replacing_leaves_only_the_new_mark() {
    let (_d, mut d) = open(2, 0);
    d.add_watermark(&text_mark(PageSet::All), false).unwrap();
    let mut other = text_mark(PageSet::All);
    other.source = WatermarkSource::Text {
        text: "FINAL".into(),
        look: look(30.0),
    };
    d.add_watermark(&other, true).unwrap();
    assert!(has(&d, 0, "FINAL") && !has(&d, 0, "RASCUNHO"));
}

#[test]
fn the_choices_are_read_back_for_update() {
    let (_d, mut d) = open(2, 0);
    let mut w = text_mark(PageSet::Range("2".into()));
    w.rotation = 45.0;
    w.behind = true;
    d.add_watermark(&w, false).unwrap();
    match d.pagination_state(Pagination::Watermark).unwrap().1 {
        Some(PaginationConfig::Watermark(back)) => assert_eq!(back, w),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_acrobat_watermark_is_removed_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acrobat.pdf");
    let content = "BT /F1 12 Tf 72 700 Td (Texto da pagina) Tj ET \
        /Artifact <</Subtype /Watermark /Type /Pagination >>BDC q /OC /X BDC BT /F1 40 Tf 100 300 Td (ACROBAT) Tj ET EMC Q EMC \
        /Span <</MCID 0>> BDC BT /F1 12 Tf 72 600 Td (Marcado proprio) Tj ET EMC";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n".into(),
    ];
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(d.pagination_state(Pagination::Watermark).unwrap().0);
    assert_eq!(d.remove_pagination(Pagination::Watermark).unwrap(), 1);
    let text = d.page_text(0).unwrap();
    assert!(!text.contains("ACROBAT"), "{text}");
    assert!(text.contains("Texto da pagina") && text.contains("Marcado proprio"));
}

#[test]
fn the_mark_is_centered_on_a_rotated_page() {
    let (_d, mut d) = open(1, 90);
    d.add_watermark(&text_mark(PageSet::All), false).unwrap();
    let size = d.page_size(0).unwrap();
    let b = d
        .page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text.contains("RASCUNHO"))
        .unwrap();
    let (cx, cy) = ((b.rect.x0 + b.rect.x1) / 2.0, (b.rect.y0 + b.rect.y1) / 2.0);
    let (w, h) = (size.width, size.height);
    assert!(
        (cx - w / 2.0).abs() < 6.0 && (cy - h / 2.0).abs() < 6.0,
        "centro {cx},{cy} página {w}x{h}"
    );
    assert!(b.rect.width() > b.rect.height(), "horizontal na tela");
}

#[test]
fn images_and_pdf_pages_can_be_the_mark() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("logo.png");
    let pix = mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 40, 20, false).unwrap();
    pix.save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    let (_d, mut d) = open(2, 0);
    let mut w = text_mark(PageSet::All);
    w.source = WatermarkSource::Image(png);
    w.scale = Some(0.5);
    // opaca: o texto estruturado do MuPDF (que lista as imagens) deixa de fora as semitransparentes
    w.opacity = 1.0;
    d.add_watermark(&w, false).unwrap();
    let imgs = d.page_images(1).unwrap();
    let img = &imgs[0].rect;
    let size = d.page_size(1).unwrap();
    assert!(
        (img.width() - size.width * 0.5).abs() < 2.0,
        "metade da largura: {img:?}"
    );
    let src = write_sample(&dir, "origem.pdf", 2, 0);
    let mut w = text_mark(PageSet::All);
    w.source = WatermarkSource::PdfPage {
        path: src.clone(),
        page: 1,
    };
    d.add_watermark(&w, true).unwrap();
    assert!(has(&d, 0, "Page 2 Hello Gera"), "a página 2 da origem");
    w.source = WatermarkSource::PdfPage { path: src, page: 9 };
    assert!(d.add_watermark(&w, true).is_err());
}

#[test]
fn undo_takes_the_mark_away() {
    let (_d, mut d) = open(2, 0);
    d.add_watermark(&text_mark(PageSet::All), false).unwrap();
    d.undo().unwrap();
    assert!(!has(&d, 0, "RASCUNHO"));
}

fn header_footer() -> HeaderFooter {
    HeaderFooter {
        texts: [
            "Esq <<n>>".into(),
            "Topo central".into(),
            "Pag <<n>> de <<total>>".into(),
            String::new(),
            "Doc <<bates>>".into(),
            "Hoje <<data>>".into(),
        ],
        look: look(10.0),
        margins: [36.0, 36.0, 36.0, 36.0],
        start: 5,
        date_format: DateFormat::DayMonthYear,
        bates: Some(Bates {
            prefix: "ABC".into(),
            suffix: "-X".into(),
            digits: 4,
            start: 1,
        }),
        pages: PageSet::All,
    }
}

#[test]
fn header_and_footer_fill_the_six_places_with_numbers() {
    let (_d, mut d) = open(3, 0);
    d.add_header_footer(&header_footer(), false).unwrap();
    let t0 = d.page_text(0).unwrap();
    assert!(
        t0.contains("Esq 5") && t0.contains("Pag 5 de 3") && t0.contains("Doc ABC0001-X"),
        "{t0}"
    );
    let today = chrono::Local::now().format("%d/%m/%Y").to_string();
    assert!(t0.contains(&format!("Hoje {today}")), "{t0}");
    let t2 = d.page_text(2).unwrap();
    assert!(t2.contains("Esq 7") && t2.contains("ABC0003-X"), "{t2}");
    let blocks = d.page_text_blocks(0).unwrap();
    let at = |s: &str| blocks.iter().find(|b| b.text.contains(s)).unwrap().rect;
    let size = d.page_size(0).unwrap();
    let top = at("Topo central");
    assert!(top.y0 >= 36.0 - 1.0 && top.y1 < 60.0, "{top:?}");
    assert!(((top.x0 + top.x1) / 2.0 - size.width / 2.0).abs() < 2.0);
    let right = at("Pag 5");
    assert!((right.x1 - (size.width - 36.0)).abs() < 2.0, "{right:?}");
    let bottom = at("Doc ABC");
    assert!(bottom.y1 <= size.height - 36.0 + 3.0 && bottom.y0 > size.height - 60.0);
    assert_eq!(d.remove_pagination(Pagination::HeaderFooter).unwrap(), 3);
    assert!(!d.page_text(0).unwrap().contains("Topo central"));
}

#[test]
fn page_ranges_are_parsed_like_the_print_dialog() {
    assert_eq!(
        pdf_core::parse_ranges("1-3, 7", 10).unwrap(),
        [(1, 3), (7, 7)]
    );
    assert!(pdf_core::parse_ranges("4-2", 10).is_err());
    assert!(pdf_core::parse_ranges("11", 10).is_err());
}

/// O caminho do arquivo da marca, lido do documento, não vale: um PDF malicioso poderia apontar
/// um arquivo local para o Atualizar embutir sem o usuário perceber.
#[test]
fn file_paths_read_from_the_document_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("logo.png");
    mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 4, 4, false)
        .unwrap()
        .save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    let (_d, mut d) = open(1, 0);
    let mut w = text_mark(PageSet::All);
    w.source = WatermarkSource::Image(png);
    d.add_watermark(&w, false).unwrap();
    match d.pagination_state(Pagination::Watermark).unwrap().1 {
        Some(PaginationConfig::Watermark(back)) => {
            assert_eq!(back.source, WatermarkSource::Image(Default::default()));
        }
        other => panic!("{other:?}"),
    }
    // e não fica gravado no PDF
    let out = dir.path().join("saida.pdf");
    d.save(&out).unwrap();
    let raw = std::fs::read(&out).unwrap();
    let path_hex: String = png_hex(dir.path().to_string_lossy().as_bytes());
    assert!(!String::from_utf8_lossy(&raw).contains(&path_hex));
}

fn png_hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02X}")).collect()
}
