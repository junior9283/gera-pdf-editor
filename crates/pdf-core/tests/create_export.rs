mod common;
use pdf_core::{create_pdf_from_images, ExportFormat, PdfDoc};

/// PNG `w`×`h` cinza gerado pelo MuPDF (que grava 96 ppp no arquivo).
fn png(dir: &std::path::Path, name: &str, w: i32, h: i32) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut pix =
        mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), w, h, false).unwrap();
    pix.clear_with(128).unwrap();
    pix.save_as(path.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    path
}

#[test]
fn images_become_one_page_each_sized_by_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let a = png(dir.path(), "a.png", 720, 360);
    let b = png(dir.path(), "b.png", 144, 288);
    let out = dir.path().join("novo.pdf");
    create_pdf_from_images(&[a, b], &out).unwrap();
    let d = PdfDoc::open(&out, None).unwrap();
    assert_eq!(d.page_count().unwrap(), 2);
    let s = d.summary().unwrap().sizes;
    // 96 ppp gravados no PNG: 720 px = 540 pt
    assert_eq!((s[0].width.round(), s[0].height.round()), (540.0, 270.0));
    assert_eq!((s[1].width.round(), s[1].height.round()), (108.0, 216.0));
    // a página mostra a imagem (cinza no meio)
    let img = d.render(0, 0.1, false).unwrap();
    let mid = ((img.height / 2 * img.width + img.width / 2) * 3) as usize;
    assert!(img.rgb[mid].abs_diff(128) < 10, "{}", img.rgb[mid]);
}

#[test]
fn something_that_is_not_an_image_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("x.png");
    std::fs::write(&bad, b"nada").unwrap();
    let out = dir.path().join("novo.pdf");
    assert!(create_pdf_from_images(&[bad], &out).is_err());
    assert!(!out.exists());
    assert!(create_pdf_from_images(&[], &out).is_err());
}

#[test]
fn exports_to_word_text_and_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_sample(&dir, "rel.pdf", 2, 0);
    let d = PdfDoc::open(&path, None).unwrap();

    let word = d.export(ExportFormat::Word).unwrap();
    assert_eq!(word, vec![dir.path().join("rel.docx")]);
    let bytes = std::fs::read(&word[0]).unwrap();
    assert!(bytes.starts_with(b"PK"), "docx é um zip");

    let txt = d.export(ExportFormat::Text).unwrap();
    let text = std::fs::read_to_string(&txt[0]).unwrap();
    assert!(text.contains("Page 1 Hello Gera") && text.contains("Page 2 Hello Gera"));

    let pngs = d.export(ExportFormat::Png { dpi: 72 }).unwrap();
    assert_eq!(pngs.len(), 2);
    assert!(
        pngs[0].ends_with("rel (imagens)/página 1.png"),
        "{:?}",
        pngs[0]
    );
    let jpgs = d.export(ExportFormat::Jpeg { dpi: 150 }).unwrap();
    assert!(
        jpgs[1].ends_with("rel (imagens) (2)/página 2.jpg"),
        "{:?}",
        jpgs[1]
    );
    let jpg = std::fs::read(&jpgs[0]).unwrap();
    assert!(jpg.starts_with(&[0xFF, 0xD8]), "JPEG");
    // exportar de novo não sobrescreve
    let again = d.export(ExportFormat::Text).unwrap();
    assert_eq!(again[0], dir.path().join("rel (2).txt"));
}
