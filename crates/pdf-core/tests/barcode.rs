mod common;
use common::write_sample;
use pdf_core::{BarcodeOptions, FieldCalc, NewFieldKind, PdfDoc, PdfRect, Symbology};

fn form() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc, i32) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.add_field(
        0,
        PdfRect::new(100.0, 100.0, 250.0, 122.0),
        NewFieldKind::Text,
        "Nome",
    )
    .unwrap();
    d.add_field(
        0,
        PdfRect::new(100.0, 140.0, 250.0, 162.0),
        NewFieldKind::Text,
        "Cidade",
    )
    .unwrap();
    let bc = d
        .add_field(
            0,
            PdfRect::new(300.0, 100.0, 450.0, 250.0),
            NewFieldKind::Barcode,
            "",
        )
        .unwrap();
    (dir, path, d, bc)
}

fn by(d: &PdfDoc, name: &str) -> pdf_core::FieldInfo {
    d.fields(0)
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
}

#[test]
fn barcode_field_is_written_like_acrobat() {
    let (_d, path, mut d, bc) = form();
    d.save(&path).unwrap();
    let raw = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(0).unwrap();
    let w = page
        .widgets()
        .find(|w| w.name().unwrap().as_deref() == Some("Código de barras1"))
        .unwrap();
    let obj = w.annotation().object();
    let pmd = obj
        .get_dict("PMD")
        .unwrap()
        .unwrap()
        .resolve()
        .unwrap()
        .unwrap()
        .to_string();
    for key in [
        "/PaperMetaData",
        "/QRCode",
        "/DataPrep 0",
        "/Resolution 300",
        "/XSymWidth",
    ] {
        assert!(pmd.contains(key), "{key} em {pmd}");
    }
    let ff = obj.get_dict("Ff").unwrap().unwrap().as_int().unwrap();
    assert_eq!(
        ff & (1 | 4 | 4096),
        1 | 4 | 4096,
        "somente leitura, não exportar, várias linhas"
    );
    let p = d.field_props(0, bc).unwrap();
    assert_eq!(p.barcode, Some(BarcodeOptions::default()));
    assert_eq!(
        by(&d, "Código de barras1").value,
        "Nome\tCidade\n\t",
        "o código novo já tem valor"
    );
    assert_eq!(
        p.calc,
        FieldCalc::Barcode {
            fields: vec!["Nome".into(), "Cidade".into()],
            names: true
        }
    );
}

#[test]
fn filling_updates_the_barcode() {
    let (_d, path, mut d, _) = form();
    d.set_field(0, by(&d, "Nome").id, "Ana").unwrap();
    d.set_field(0, by(&d, "Cidade").id, "Recife").unwrap();
    assert_eq!(
        by(&d, "Código de barras1").value,
        "Nome\tCidade\nAna\tRecife"
    );
    // o desfazer volta o valor do código numa etapa com o campo
    d.undo().unwrap();
    assert_eq!(by(&d, "Código de barras1").value, "Nome\tCidade\nAna\t");
    d.save(&path).unwrap();
}

/// Renderiza a página e lê o código com o decodificador do rxing.
fn decoded(d: &PdfDoc, path: &std::path::Path, format: rxing::BarcodeFormat) -> String {
    let _ = d;
    let doc = mupdf::Document::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_page(0).unwrap();
    let pix = page
        .to_pixmap(
            &mupdf::Matrix::new_scale(4.0, 4.0),
            &mupdf::Colorspace::device_gray(),
            false,
            true,
        )
        .unwrap();
    let (w, h) = (pix.width(), pix.height());
    let luma: Vec<u8> = pix.samples().to_vec();
    rxing::helpers::detect_in_luma(luma, w, h, Some(format))
        .map(|r| r.getText().to_string())
        .unwrap_or_else(|e| format!("não leu: {e:?}"))
}

#[test]
fn barcode_reads_back() {
    for (sym, format) in [
        (Symbology::QrCode, rxing::BarcodeFormat::QR_CODE),
        (Symbology::Pdf417, rxing::BarcodeFormat::PDF_417),
        (Symbology::DataMatrix, rxing::BarcodeFormat::DATA_MATRIX),
    ] {
        let (_d, path, mut d, bc) = form();
        let mut p = d.field_props(0, bc).unwrap();
        p.barcode = Some(BarcodeOptions {
            symbology: sym,
            ecc: 1,
        });
        d.set_field_props(0, bc, &p).unwrap();
        d.set_field(0, by(&d, "Nome").id, "José").unwrap();
        d.save(&path).unwrap();
        assert_eq!(
            decoded(&d, &path, format),
            "Nome\tCidade\nJosé\t",
            "{sym:?}"
        );
    }
}

#[test]
fn foreign_barcode_draws_its_value() {
    let dir = tempfile::tempdir().unwrap();
    let objs = [
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".into(),
        "<</Type/Annot/Subtype/Widget/FT/Tx/Ff 4101/T(BC)/Rect[100 400 300 600]/P 3 0 R/F 4\
         /V(Gerado por outro programa)/PMD 5 0 R/DA(/Helv 0 Tf 0 g)\
         /AA<</C<</S/JavaScript/JS(event.value = 'x';)>>>>>>"
            .into(),
        "<</Type/PaperMetaData/Version 1/Symbology/PDF417/XSymWidth 9/XSymHeight 27/ECC 2\
         /DataPrep 0/Resolution 300.0>>"
            .into(),
    ];
    let path = dir.path().join("outro.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(!d.is_dirty(), "desenhar ao abrir não conta como alteração");
    d.save(&path).unwrap();
    assert_eq!(
        decoded(&d, &path, rxing::BarcodeFormat::PDF_417),
        "Gerado por outro programa"
    );
}

#[test]
fn too_much_data_warns() {
    let o = BarcodeOptions {
        symbology: Symbology::QrCode,
        ecc: 3,
    };
    assert!(pdf_core::barcode_modules(o, &"x".repeat(5_000)).is_err());
    assert!(pdf_core::barcode_modules(o, &"y".repeat(2_500)).is_err());
    assert!(pdf_core::barcode_modules(o, "ok").is_ok());
}

#[test]
fn many_heavy_barcodes_open_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let n = 400;
    let value = "Z9".repeat(950);
    let mut objs = vec![
        format!(
            "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[{}]>>>>",
            (0..n)
                .map(|k| format!("{} 0 R", k + 5))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[{}]>>",
            (0..n)
                .map(|k| format!("{} 0 R", k + 5))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<</Type/PaperMetaData/Version 1/Symbology/QRCode/XSymWidth 9/ECC 0\
         /DataPrep 0/Resolution 300.0>>"
            .into(),
    ];
    for k in 0..n {
        objs.push(format!(
            "<</Type/Annot/Subtype/Widget/FT/Tx/T(B{k})/Rect[10 10 200 200]/P 3 0 R\
             /V({value})/PMD 4 0 R>>"
        ));
    }
    let path = dir.path().join("muitos.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let t = std::time::Instant::now();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(t.elapsed().as_secs_f32() < 3.0, "abrir: {:?}", t.elapsed());
    // mexer num campo da página redesenha a página: também com teto
    let t = std::time::Instant::now();
    let id = d.fields(0).unwrap()[0].id;
    d.move_field(0, id, pdf_core::PdfRect::new(20.0, 20.0, 210.0, 210.0))
        .unwrap();
    assert!(t.elapsed().as_secs_f32() < 3.0, "mover: {:?}", t.elapsed());
}

#[test]
fn data_matrix_never_reaches_the_encoder_panic() {
    // o rxing entra em pânico com Data Matrix grande; no app (panic = "abort") isso derrubaria
    // tudo: o valor tem de ser recusado antes
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let panicked = Arc::new(AtomicBool::new(false));
    let flag = panicked.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |_| flag.store(true, Ordering::SeqCst)));
    let dm = BarcodeOptions {
        symbology: Symbology::DataMatrix,
        ecc: 0,
    };
    let big = "ação\t".repeat(400);
    let digits = "1".repeat(1999);
    let r1 = pdf_core::barcode_modules(dm, &big);
    let t = std::time::Instant::now();
    let r2 = pdf_core::barcode_modules(dm, &digits);
    let took = t.elapsed();
    std::panic::set_hook(previous);
    assert!(r1.is_err() && r2.is_err());
    assert!(
        !panicked.load(Ordering::SeqCst),
        "o codificador não pode entrar em pânico"
    );
    assert!(
        took.as_millis() < 50,
        "recusado antes de codificar: {took:?}"
    );
}

#[test]
fn need_appearances_and_revert_keep_the_barcode_drawn() {
    let dir = tempfile::tempdir().unwrap();
    for need in ["/NeedAppearances true", ""] {
        let objs = [
            format!("<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]{need}>>>>"),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".into(),
            "<</Type/Annot/Subtype/Widget/FT/Tx/Ff 4101/T(BC)/Rect[100 400 300 600]/P 3 0 R/F 4\
             /V(Lido)/PMD 5 0 R/DA(/Helv 0 Tf 0 g)>>"
                .into(),
            "<</Type/PaperMetaData/Version 1/Symbology/QRCode/XSymWidth 9/ECC 256\
             /DataPrep 0/Resolution 300.0>>"
                .into(),
        ];
        let path = dir.path().join("na.pdf");
        std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
        let mut d = PdfDoc::open(&path, None).unwrap();
        // /ECC fora do intervalo fica no maior nível (não volta a 0)
        let id = d.fields(0).unwrap()[0].id;
        assert_eq!(d.field_props(0, id).unwrap().barcode.unwrap().ecc, 3);
        d.save(&path).unwrap();
        assert_eq!(
            decoded(&d, &path, rxing::BarcodeFormat::QR_CODE),
            "Lido",
            "{need}"
        );
        // Reverter reabre o arquivo (o original, de novo): o código continua desenhado
        std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
        d.revert().unwrap();
        d.set_field(0, id, "").unwrap_or_default();
        let out = dir.path().join("depois.pdf");
        std::fs::copy(&path, &out).unwrap();
        let mut d2 = PdfDoc::open(&out, None).unwrap();
        d2.revert().unwrap();
        d2.save(&out).unwrap();
        assert_eq!(
            decoded(&d2, &out, rxing::BarcodeFormat::QR_CODE),
            "Lido",
            "reverter {need}"
        );
    }
}

#[test]
fn moving_updates_the_paper_metadata_size() {
    let (_d, path, mut d, bc) = form();
    d.move_field(0, bc, PdfRect::new(300.0, 100.0, 516.0, 316.0))
        .unwrap();
    d.save(&path).unwrap();
    let raw = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(0).unwrap();
    let w = page
        .widgets()
        .find(|w| w.name().unwrap().as_deref() == Some("Código de barras1"))
        .unwrap();
    let pmd = w.annotation().object().get_dict("PMD").unwrap().unwrap();
    let width = pmd.get_dict("Width").unwrap().unwrap().as_float().unwrap();
    assert!(
        (width - 3.0).abs() < 0.01,
        "216 pt = 3 polegadas, veio {width}"
    );
}

#[test]
fn a_huge_field_list_is_still_recognized() {
    let names: Vec<String> = (0..2500)
        .map(|k| format!("um campo de nome comprido {k:04}"))
        .collect();
    let c = FieldCalc::Barcode {
        fields: names,
        names: true,
    };
    assert_eq!(pdf_core::parse_calc(&pdf_core::calc_script(&c)), c);
}

#[test]
fn filling_with_many_barcodes_stays_fast() {
    let dir = tempfile::tempdir().unwrap();
    let n = 200;
    let script = pdf_core::calc_script(&FieldCalc::Barcode {
        fields: vec!["A".into()],
        names: false,
    });
    let js = script
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let mut objs = vec![
        format!(
            "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R {}]>>>>",
            (0..n).map(|k| format!("{} 0 R", k + 6)).collect::<Vec<_>>().join(" ")
        ),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R {}]>>",
            (0..n).map(|k| format!("{} 0 R", k + 6)).collect::<Vec<_>>().join(" ")
        ),
        "<</Type/Annot/Subtype/Widget/FT/Tx/T(A)/Rect[10 700 300 720]/P 3 0 R/V()>>".into(),
        "<</Type/PaperMetaData/Version 1/Symbology/DataMatrix/XSymWidth 9/DataPrep 0/Resolution 300.0>>"
            .into(),
    ];
    for k in 0..n {
        objs.push(format!(
            "<</Type/Annot/Subtype/Widget/FT/Tx/T(B{k})/Rect[10 10 200 200]/P 3 0 R/V()\
             /PMD 5 0 R/AA<</C<</S/JavaScript/JS({js})>>>>>>"
        ));
    }
    let path = dir.path().join("dm.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let a = by(&d, "A").id;
    let t = std::time::Instant::now();
    d.set_field(0, a, &"1".repeat(499)).unwrap();
    assert!(t.elapsed().as_secs_f32() < 5.0, "{:?}", t.elapsed());
    assert_eq!(
        by(&d, "B199").value,
        "1".repeat(499),
        "o valor de todos é gravado"
    );
}
