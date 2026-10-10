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

#[test]
fn giant_images_and_giant_renders_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    // PNG de 30000×30000 (900 Mpx) com poucos KB: decodificar estouraria a memória
    let path = dir.path().join("bomba.png");
    std::process::Command::new("magick")
        .args([
            "-size",
            "30000x30000",
            "xc:white",
            "-depth",
            "1",
            path.to_str().unwrap(),
        ])
        .status()
        .ok();
    if path.exists() {
        let out = dir.path().join("b.pdf");
        assert!(create_pdf_from_images(&[path], &out).is_err());
    }
    // página do tamanho máximo do PDF (200 pol) a 600 ppp = 120000 px de lado: não renderiza
    let big = dir.path().join("grande.pdf");
    let mut doc = mupdf::pdf::PdfDocument::new();
    doc.new_page(mupdf::Size {
        width: 14400.0,
        height: 14400.0,
    })
    .unwrap();
    doc.save(big.to_str().unwrap()).unwrap();
    let d = PdfDoc::open(&big, None).unwrap();
    assert!(d.export(ExportFormat::Png { dpi: 600 }).is_err());
}

#[test]
fn the_word_file_is_a_clean_zip() {
    // flags do cabeçalho local iguais às do diretório central (sem "data descriptor"): o
    // LibreOffice recusa o zip do MuPDF por causa disso
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_sample(&dir, "rel.pdf", 1, 0);
    let docx = PdfDoc::open(&path, None)
        .unwrap()
        .export(ExportFormat::Word)
        .unwrap();
    let bytes = std::fs::read(&docx[0]).unwrap();
    let mut i = 0;
    let mut entries = 0;
    while i + 30 <= bytes.len() && bytes[i..i + 4] == *b"PK\x03\x04" {
        let flags = u16::from_le_bytes([bytes[i + 6], bytes[i + 7]]);
        assert_eq!(flags & 8, 0, "entrada {entries} com data descriptor");
        let size = u32::from_le_bytes(bytes[i + 18..i + 22].try_into().unwrap()) as usize;
        let name = u16::from_le_bytes([bytes[i + 26], bytes[i + 27]]) as usize;
        let extra = u16::from_le_bytes([bytes[i + 28], bytes[i + 29]]) as usize;
        i += 30 + name + extra + size;
        entries += 1;
    }
    assert!(entries >= 3, "{entries} entradas");
}

#[test]
fn phone_photos_follow_their_exif_orientation() {
    // JPEG 200×100 com EXIF orientation=6 (girar 90° horário): a página sai em retrato
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("cru.jpg");
    let ok = std::process::Command::new("magick")
        .args([
            "-size",
            "200x100",
            "gradient:red-blue",
            raw.to_str().unwrap(),
        ])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return;
    }
    // APP1 "Exif" com um IFD de uma entrada: Orientation (0x0112) = 6, logo depois do SOI
    let tiff: Vec<u8> = [
        &b"II*\0"[..],
        &8u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &0x0112u16.to_le_bytes(),
        &3u16.to_le_bytes(),
        &1u32.to_le_bytes(),
        &6u16.to_le_bytes(),
        &[0, 0],
        &0u32.to_le_bytes(),
    ]
    .concat();
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend(((tiff.len() + 8) as u16).to_be_bytes());
    app1.extend(b"Exif\0\0");
    app1.extend(&tiff);
    let jpeg = std::fs::read(&raw).unwrap();
    let path = dir.path().join("celular.jpg");
    std::fs::write(&path, [&jpeg[..2], &app1, &jpeg[2..]].concat()).unwrap();
    let out = dir.path().join("c.pdf");
    create_pdf_from_images(&[path], &out).unwrap();
    let d = PdfDoc::open(&out, None).unwrap();
    let s = &d.summary().unwrap().sizes[0];
    assert!(s.height > s.width, "retrato: {s:?}");
    // o vermelho (topo da imagem crua) fica à direita depois de girar 90° no sentido horário
    let img = d.render(0, 1.0, false).unwrap();
    let px = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 3) as usize;
        (img.rgb[i], img.rgb[i + 2])
    };
    let (r_right, b_right) = px(img.width - 3, img.height / 2);
    let (r_left, b_left) = px(2, img.height / 2);
    assert!(
        r_right > b_right && b_left > r_left,
        "direita {r_right}/{b_right} esquerda {r_left}/{b_left}"
    );
}

#[test]
fn a_failed_image_export_leaves_no_half_folder() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("g.pdf");
    let mut doc = mupdf::pdf::PdfDocument::new();
    doc.new_page(mupdf::Size {
        width: 300.0,
        height: 300.0,
    })
    .unwrap();
    doc.new_page(mupdf::Size {
        width: 14400.0,
        height: 14400.0,
    })
    .unwrap();
    doc.save(big.to_str().unwrap()).unwrap();
    let d = PdfDoc::open(&big, None).unwrap();
    assert!(d.export(ExportFormat::Png { dpi: 600 }).is_err());
    assert!(!dir.path().join("g (imagens)").exists(), "nada pela metade");
}
