mod common;
use common::write_sample;
use pdf_core::{sample::pdf_from_objects, PageLabelRange, PageLabelStyle, PdfDoc};

fn range(start_page: i32, style: PageLabelStyle, prefix: &str, first: i32) -> PageLabelRange {
    PageLabelRange {
        start_page,
        style,
        prefix: prefix.into(),
        first,
    }
}

#[test]
fn labels_round_trip_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&write_sample(&dir, "a.pdf", 6, 0), None).unwrap();
    assert!(d.labels().is_empty(), "sem /PageLabels, sem rótulos");
    d.set_page_labels(vec![
        range(0, PageLabelStyle::LowerRoman, "", 1),
        range(2, PageLabelStyle::Decimal, "P-", 5),
    ])
    .unwrap();
    assert_eq!(d.labels(), ["i", "ii", "P-5", "P-6", "P-7", "P-8"]);
    let out = dir.path().join("b.pdf");
    d.save(&out).unwrap();
    let mut back = PdfDoc::open(&out, None).unwrap();
    assert_eq!(back.labels(), ["i", "ii", "P-5", "P-6", "P-7", "P-8"]);
    assert_eq!(back.page_labels().len(), 2);
    back.set_page_labels(Vec::new()).unwrap();
    assert!(back.labels().is_empty());
    back.undo().unwrap();
    assert_eq!(back.labels()[2], "P-5", "desfazer volta os rótulos");
}

#[test]
fn nums_out_of_order_and_kids_cycle_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut objs = vec![
        "<</Type/Catalog/Pages 2 0 R/PageLabels 8 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R 6 0 R 7 0 R]/Count 5>>".to_string(),
    ];
    for _ in 0..5 {
        objs.push("<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".into());
    }
    // a raiz aponta para um filho que aponta de volta para ela; os números vêm fora de ordem
    objs.push("<</Kids[9 0 R]>>".into());
    objs.push("<</Nums[3 <</S/A>> 0 <</S/r/St 2>>]/Kids[8 0 R]>>".into());
    let path = dir.path().join("rot.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.labels(), ["ii", "iii", "iv", "A", "B"]);
}

#[test]
fn a_new_section_keeps_the_other_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&write_sample(&dir, "a.pdf", 8, 0), None).unwrap();
    d.set_page_labels(vec![
        range(0, PageLabelStyle::LowerRoman, "", 1),
        range(3, PageLabelStyle::Decimal, "", 1),
    ])
    .unwrap();
    let new = pdf_core::apply_labels(
        &d.page_labels(),
        1,
        1,
        Some(range(0, PageLabelStyle::UpperAlpha, "", 1)),
        8,
    );
    d.set_page_labels(new).unwrap();
    assert_eq!(d.labels(), ["i", "A", "iii", "1", "2", "3", "4", "5"]);
}

#[test]
fn page_operations_do_not_invent_or_shift_labels() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&write_sample(&dir, "a.pdf", 5, 0), None).unwrap();
    d.apply_pages(pdf_core::PageOp::Move {
        pages: vec![1, 2],
        to: 0,
    })
    .unwrap();
    assert!(
        d.labels().is_empty(),
        "mover não cria rótulos: {:?}",
        d.labels()
    );
    d.set_page_labels(vec![
        range(0, PageLabelStyle::LowerRoman, "", 1),
        range(2, PageLabelStyle::Decimal, "", 1),
    ])
    .unwrap();
    d.apply_pages(pdf_core::PageOp::Delete { pages: vec![0] })
        .unwrap();
    assert_eq!(
        d.labels(),
        ["i", "ii", "1", "2"],
        "as faixas ficam nos índices"
    );
}
