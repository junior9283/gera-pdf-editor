use pdf_core::{
    AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect, RedactLook, RedactPattern, RedactQuery,
    SanitizeOptions,
};

/// Página com texto sensível, uma imagem cinza (100×100 pt em x=300, y=500 do PDF), um link e
/// metadados, anexo, script de abertura e um comentário.
fn sensitive(dir: &std::path::Path) -> std::path::PathBuf {
    let content = "BT /F1 12 Tf 72 700 Td (Nome: Ana Souza CPF 123.456.789-09) Tj ET \
        BT /F1 12 Tf 72 680 Td (Contato: ana@exemplo.com.br tel. (11) 98765-4321) Tj ET \
        BT /F1 12 Tf 72 660 Td (CNPJ 12.345.678/0001-90 CEP 01310-100 em 10/10/2026) Tj ET \
        BT /F1 12 Tf 72 640 Td (Cartao 4111 1111 1111 1111 pagina 3 valor 1.234,56) Tj ET \
        q 100 0 0 100 300 400 cm /Im0 Do Q";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R/OpenAction<</S/JavaScript/JS(app.alert\\(1\\))>>/Names<</EmbeddedFiles<</Names[(a.txt) 9 0 R]>>>>/Metadata 10 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>/XObject<</Im0 6 0 R>>>>/Contents 4 0 R/Annots[7 0 R 8 0 R]>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        "6 0 obj<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceGray/BitsPerComponent 8/Length 4>>stream\n\u{80}\u{80}\u{80}\u{80}\nendstream endobj\n".into(),
        "7 0 obj<</Type/Annot/Subtype/Link/Rect[72 695 200 712]/A<</S/URI/URI(https://exemplo.com)>>>>endobj\n".into(),
        "8 0 obj<</Type/Annot/Subtype/Text/Rect[500 700 520 720]/Contents(segredo no comentario)>>endobj\n".into(),
        "9 0 obj<</Type/Filespec/F(a.txt)/EF<</F 11 0 R>>>>endobj\n".into(),
        "10 0 obj<</Type/Metadata/Subtype/XML/Length 9>>stream\n<x:xmp/>\nendstream endobj\n".into(),
        "11 0 obj<</Type/EmbeddedFile/Length 5>>stream\nanexo\nendstream endobj\n".into(),
    ];
    let path = dir.join("sensivel.pdf");
    std::fs::write(
        &path,
        format!(
            "%PDF-1.4\n{}trailer<</Root 1 0 R/Info<</Author(Ana Souza)/Title(Contrato)>>>>\n",
            objs.concat()
        ),
    )
    .unwrap();
    path
}

fn open() -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&sensitive(dir.path()), None).unwrap();
    (dir, d)
}

fn look() -> RedactLook {
    RedactLook {
        fill: [0, 0, 0],
        overlay: String::new(),
        overlay_color: [255, 0, 0],
    }
}

/// Cor do pixel (x, y) da página 0 desenhada a 1:1.
fn px(d: &PdfDoc, x: u32, y: u32) -> [u8; 3] {
    let p = d.render(0, 1.0, false).unwrap();
    let i = ((y * p.width + x) * 3) as usize;
    [p.rgb[i], p.rgb[i + 1], p.rgb[i + 2]]
}

fn mark(d: &mut PdfDoc, rects: Vec<PdfRect>) {
    d.apply(AnnotOp::Create(NewAnnot::Redact { page: 0, rects }))
        .unwrap();
}

fn find_rect(d: &PdfDoc, text: &str) -> PdfRect {
    let hits = d
        .find_redactions(&RedactQuery::Phrase(text.into()))
        .unwrap();
    assert!(!hits.is_empty(), "{text:?} não achado");
    hits[0].rects[0]
}

#[test]
fn marks_are_redact_annotations_until_applied() {
    let (_d, mut d) = open();
    let r = find_rect(&d, "Ana Souza");
    mark(&mut d, vec![r]);
    let annots = d.annotations(0).unwrap();
    assert!(annots.iter().any(|a| a.kind == AnnotKind::Redact));
    assert!(d.page_text(0).unwrap().contains("Ana Souza"), "só marcado");
}

#[test]
fn applying_removes_the_text_for_good_and_paints_the_box() {
    let (_d, mut d) = open();
    let r = find_rect(&d, "123.456.789-09");
    mark(&mut d, vec![r]);
    let n = d
        .apply_redactions(&RedactLook {
            overlay: "SIGILOSO".into(),
            ..look()
        })
        .unwrap();
    assert_eq!(n, 1);
    let text = d.page_text(0).unwrap();
    assert!(!text.contains("123.456.789-09"), "{text}");
    assert!(
        text.contains("Nome: Ana Souza") && text.contains("Contato"),
        "{text}"
    );
    assert!(text.contains("SIGILOSO"), "texto sobreposto: {text}");
    assert!(
        !d.annotations(0)
            .unwrap()
            .iter()
            .any(|a| a.kind == AnnotKind::Redact),
        "as marcas somem depois de aplicadas"
    );
    // a caixa é pintada na cor escolhida
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    assert_eq!(px(&d, cx as u32, cy as u32), [0, 0, 0]);
}

#[test]
fn images_under_the_mark_lose_their_pixels() {
    let (_d, mut d) = open();
    // a imagem fica em x 300..400, y (tela) 292..392; a marca cobre a metade de cima
    mark(&mut d, vec![PdfRect::new(290.0, 280.0, 410.0, 340.0)]);
    d.apply_redactions(&RedactLook {
        fill: [255, 255, 255],
        ..look()
    })
    .unwrap();
    assert_eq!(px(&d, 350, 310), [255, 255, 255], "coberta: apagada");
    assert_ne!(px(&d, 350, 370), [255, 255, 255], "fora da marca: continua");
}

#[test]
fn undo_brings_everything_back() {
    let (_d, mut d) = open();
    let r = find_rect(&d, "Ana Souza");
    mark(&mut d, vec![r]);
    d.apply_redactions(&look()).unwrap();
    d.undo().unwrap();
    assert!(d.page_text(0).unwrap().contains("Ana Souza"));
}

#[test]
fn patterns_find_the_sensitive_data() {
    let (_d, d) = open();
    let found = |p: RedactPattern| -> Vec<String> {
        d.find_redactions(&RedactQuery::Patterns(vec![p]))
            .unwrap()
            .into_iter()
            .map(|h| h.text)
            .collect()
    };
    assert_eq!(found(RedactPattern::Cpf), ["123.456.789-09"]);
    assert_eq!(found(RedactPattern::Cnpj), ["12.345.678/0001-90"]);
    assert_eq!(found(RedactPattern::Email), ["ana@exemplo.com.br"]);
    assert_eq!(found(RedactPattern::Phone), ["(11) 98765-4321"]);
    assert_eq!(found(RedactPattern::Cep), ["01310-100"]);
    assert_eq!(found(RedactPattern::Date), ["10/10/2026"]);
    assert_eq!(found(RedactPattern::CreditCard), ["4111 1111 1111 1111"]);
    // e as caixas cobrem só o trecho achado
    let hit = &d
        .find_redactions(&RedactQuery::Patterns(vec![RedactPattern::Cpf]))
        .unwrap()[0];
    let phrase = find_rect(&d, "123.456.789-09");
    assert!((hit.rects[0].x0 - phrase.x0).abs() < 1.0 && (hit.rects[0].x1 - phrase.x1).abs() < 1.0);
}

#[test]
fn phrases_ignore_case() {
    let (_d, d) = open();
    let hits = d
        .find_redactions(&RedactQuery::Phrase("ana souza".into()))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 0);
}

#[test]
fn sanitizing_removes_hidden_information() {
    let dir = tempfile::tempdir().unwrap();
    let path = sensitive(dir.path());
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sanitize(&SanitizeOptions {
        metadata: true,
        attachments: true,
        scripts: true,
        comments: true,
    })
    .unwrap();
    let out = dir.path().join("limpo.pdf");
    d.save(&out).unwrap();
    let raw = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    for gone in [
        "Ana Souza)",
        "JavaScript",
        "EmbeddedFile",
        "x:xmp",
        "segredo no comentario",
    ] {
        assert!(!raw.contains(gone), "{gone} ainda no arquivo");
    }
    let d = PdfDoc::open(&out, None).unwrap();
    assert!(
        d.page_text(0).unwrap().contains("Contato"),
        "o conteúdo fica"
    );
}
