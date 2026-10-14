mod common;
use pdf_core::{create_pdf_from_images, OcrLanguage, PdfDoc};

/// PDF "digitalizado": a página de amostra renderizada (sem texto, só imagem). `rotate`: /Rotate
/// gravado depois na página.
fn scanned(dir: &tempfile::TempDir, rotate: i32) -> std::path::PathBuf {
    let src = common::write_sample(dir, "fonte.pdf", 1, 0);
    let d = PdfDoc::open(&src, None).unwrap();
    let img = d.render(0, 200.0 / 72.0, false).unwrap();
    let png = dir.path().join("pagina.png");
    let pix = {
        let mut p = mupdf::Pixmap::new_with_w_h(
            &mupdf::Colorspace::device_rgb(),
            img.width as i32,
            img.height as i32,
            false,
        )
        .unwrap();
        p.samples_mut().copy_from_slice(&img.rgb);
        p
    };
    pix.save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    let out = dir.path().join(format!("digitalizado{rotate}.pdf"));
    create_pdf_from_images(&[png], &out).unwrap();
    if rotate != 0 {
        let doc = mupdf::pdf::PdfDocument::from_bytes(&std::fs::read(&out).unwrap()).unwrap();
        let mut page = doc.find_page(0).unwrap();
        page.dict_put("Rotate", mupdf::pdf::PdfObject::new_int(rotate).unwrap())
            .unwrap();
        doc.save(out.to_str().unwrap()).unwrap();
    }
    out
}

#[test]
fn a_scanned_page_becomes_searchable_without_changing_its_look() {
    let dir = tempfile::tempdir().unwrap();
    let path = scanned(&dir, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(d.page_text(0).unwrap().trim().is_empty(), "só imagem antes");
    let before = d.render(0, 0.5, false).unwrap();
    let r = d
        .recognize_text(&[0], OcrLanguage::PortugueseEnglish, |_, _| true)
        .unwrap();
    assert_eq!((r.recognized, r.skipped), (1, 0));
    assert!(r.words >= 3, "{r:?}");
    let text = d.page_text(0).unwrap();
    assert!(text.contains("Hello") && text.contains("Gera"), "{text:?}");
    let hits = d.search(0, "Hello").unwrap();
    assert_eq!(hits.len(), 1);
    // a palavra achada fica onde está na imagem (perto do topo esquerdo, como na amostra)
    let h = &hits[0];
    let size = &d.summary().unwrap().sizes[0];
    let (fx, fy) = (h.x0 / size.width, h.y0 / size.height);
    assert!(
        fx > 0.1 && fx < 0.7 && fy > 0.05 && fy < 0.2,
        "{h:?} em {size:?}"
    );
    let after = d.render(0, 0.5, false).unwrap();
    assert!(before.rgb == after.rgb, "a aparência não muda");
    d.undo().unwrap();
    assert!(
        d.page_text(0).unwrap().trim().is_empty(),
        "desfazer remove a camada"
    );
}

#[test]
fn pages_with_text_are_skipped_and_cancel_stops() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_sample(&dir, "texto.pdf", 3, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = d
        .recognize_text(&[0, 1, 2], OcrLanguage::English, |_, _| true)
        .unwrap();
    assert_eq!((r.recognized, r.skipped), (0, 3));
    let scan = scanned(&dir, 0);
    let mut d = PdfDoc::open(&scan, None).unwrap();
    let r = d
        .recognize_text(&[0], OcrLanguage::English, |_, _| false)
        .unwrap();
    assert_eq!(r.recognized, 0, "cancelado antes da página");
}

#[test]
fn rotated_scans_get_the_text_over_the_words() {
    let dir = tempfile::tempdir().unwrap();
    let path = scanned(&dir, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.recognize_text(&[0], OcrLanguage::English, |_, _| true)
        .unwrap();
    let hits = d.search(0, "Hello").unwrap();
    assert_eq!(hits.len(), 1, "{:?}", d.page_text(0));
    // girada 90° na tela: o texto do topo da imagem fica à direita
    let w = d.summary().unwrap().sizes[0].width;
    assert!(hits[0].x0 > w / 2.0, "{:?} largura {w}", hits[0]);
}

#[test]
fn body_text_on_a_real_size_scan_is_read_correctly() {
    // página Carta com texto de 6 pt, "digitalizada" a 300 ppp (o PNG guarda 300 ppp, então a
    // página do PDF fica do tamanho real)
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("corpo.pdf");
    let content = "BT /F1 6 Tf 72 700 Td (The quick brown fox jumps over the lazy dog) Tj ET";
    let pdf = format!(
        "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n\
         4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n\
         5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R>>\n",
        content.len()
    );
    std::fs::write(&src, pdf).unwrap();
    let png = dir.path().join("corpo.png");
    let ok = std::process::Command::new("magick")
        .args([
            "-density",
            "300",
            src.to_str().unwrap(),
            "-background",
            "white",
            "-alpha",
            "remove",
            "-units",
            "PixelsPerInch",
            png.to_str().unwrap(),
        ])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return;
    }
    let scan = dir.path().join("scan.pdf");
    create_pdf_from_images(&[png], &scan).unwrap();
    let mut d = PdfDoc::open(&scan, None).unwrap();
    assert!(
        (d.summary().unwrap().sizes[0].width - 612.0).abs() < 2.0,
        "página de tamanho real"
    );
    d.recognize_text(&[0], OcrLanguage::English, |_, _| true)
        .unwrap();
    let text = d.page_text(0).unwrap();
    for word in ["quick", "brown", "jumps", "lazy"] {
        assert!(text.contains(word), "{word} em {text:?}");
    }
}
