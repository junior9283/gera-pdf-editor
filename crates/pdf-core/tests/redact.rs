use pdf_core::{
    AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect, RedactLook, RedactPattern, RedactQuery,
    SanitizeOptions,
};

/// Página com texto sensível, uma imagem cinza (100×100 pt em x=300, y=500 do PDF), um link e
/// metadados, anexo, script de abertura e um comentário.
fn sensitive(dir: &std::path::Path) -> std::path::PathBuf {
    let content = "BT /F1 12 Tf 72 700 Td (Nome: Ana Souza CPF 123.456.789-09) Tj ET \
        BT /F1 12 Tf 72 680 Td (Contato: ana@exemplo.com.br tel. (11) 98765-4321) Tj ET \
        BT /F1 12 Tf 72 660 Td (CNPJ 12.345.678/0001-95 CEP 01310-100 em 10/10/2026) Tj ET \
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
    RedactLook::default()
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
    assert_eq!(found(RedactPattern::Cnpj), ["12.345.678/0001-95"]);
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

/// Página com o segredo repetido onde a redação do MuPDF não chega sozinha: campo de formulário,
/// destaque com popup, /ActualText, miniatura (/Thumb) e dados privados (/PieceInfo).
fn hidden_copies(dir: &std::path::Path) -> std::path::PathBuf {
    let content = "BT /F1 12 Tf 72 700 Td (Nome: Ana Souza) Tj ET \
        /Span <</ActualText (ACTUALSECRET)>> BDC BT /F1 12 Tf 72 680 Td (visivel) Tj ET EMC \
        BT /F1 12 Tf 72 600 Td (Texto que fica) Tj ET";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[6 0 R]>>>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R/Annots[6 0 R 7 0 R 8 0 R]/Thumb 9 0 R/PieceInfo<</X<</Private(PIECESECRET)>>>>>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        "6 0 obj<</Type/Annot/Subtype/Widget/FT/Tx/T(campo)/V(FIELDSECRET)/Rect[72 650 250 668]/P 3 0 R>>endobj\n".into(),
        "7 0 obj<</Type/Annot/Subtype/Highlight/Rect[100 695 170 712]/QuadPoints[100 712 170 712 100 695 170 695]/Contents(HLSECRET)/Popup 8 0 R>>endobj\n".into(),
        "8 0 obj<</Type/Annot/Subtype/Popup/Rect[300 600 400 700]/Parent 7 0 R>>endobj\n".into(),
        "9 0 obj<</Width 1/Height 1/ColorSpace/DeviceGray/BitsPerComponent 8/Length 11>>stream\nTHUMBSECRET\nendstream endobj\n".into(),
    ];
    let path = dir.join("copias.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    path
}

/// Aplica uma marca sobre a página toda (menos a última linha) e devolve os bytes salvos.
fn redact_top_and_save(path: &std::path::Path) -> String {
    let mut d = PdfDoc::open(path, None).unwrap();
    // tela: y para baixo; a linha "Texto que fica" (y 600 do PDF) fica fora
    mark(&mut d, vec![PdfRect::new(50.0, 70.0, 560.0, 160.0)]);
    d.apply_redactions(&look()).unwrap();
    let text = d.page_text(0).unwrap();
    assert!(text.contains("Texto que fica"), "{text}");
    let out = path.with_file_name("saida.pdf");
    d.save(&out).unwrap();
    String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned()
}

#[test]
fn copies_of_the_secret_outside_the_text_layer_go_too() {
    let dir = tempfile::tempdir().unwrap();
    let raw = redact_top_and_save(&hidden_copies(dir.path()));
    for secret in [
        "FIELDSECRET",
        "HLSECRET",
        "ACTUALSECRET",
        "THUMBSECRET",
        "PIECESECRET",
    ] {
        assert!(!raw.contains(secret), "{secret} ficou no arquivo");
    }
}

#[test]
fn an_undone_mark_is_not_applied() {
    let (_d, mut d) = open();
    let r = find_rect(&d, "Ana Souza");
    mark(&mut d, vec![r]);
    d.undo().unwrap();
    let cpf = find_rect(&d, "123.456.789-09");
    mark(&mut d, vec![cpf]);
    d.apply_redactions(&look()).unwrap();
    let text = d.page_text(0).unwrap();
    assert!(
        text.contains("Ana Souza"),
        "a marca desfeita não vale: {text}"
    );
    assert!(!text.contains("123.456.789-09"));
}

/// Página com os casos de borda dos padrões.
fn pattern_page(dir: &std::path::Path, lines: &[&str]) -> PdfDoc {
    let mut content = String::new();
    for (i, l) in lines.iter().enumerate() {
        content.push_str(&format!(
            "BT /F1 12 Tf 72 {} Td ({l}) Tj ET ",
            700 - 20 * i as i32
        ));
    }
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
    ];
    let path = dir.join("padroes.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    PdfDoc::open(&path, None).unwrap()
}

#[test]
fn patterns_check_digits_and_accept_common_variants() {
    let dir = tempfile::tempdir().unwrap();
    let d = pattern_page(
        dir.path(),
        &[
            "CPF errado 123.456.789-00 e certo 123 456 789 09",
            "CNPJ errado 12.345.678/0001-91",
            "anos 2019-2023 e fone 11 98765-4321",
            "CEP 01.310-100",
            "datas 2026-10-10 e 10.10.2026",
            "Amex 3782 822463 10005",
        ],
    );
    let found = |p: RedactPattern| -> Vec<String> {
        d.find_redactions(&RedactQuery::Patterns(vec![p]))
            .unwrap()
            .into_iter()
            .map(|h| h.text)
            .collect()
    };
    assert_eq!(found(RedactPattern::Cpf), ["123 456 789 09"]);
    assert!(found(RedactPattern::Cnpj).is_empty(), "dígito errado");
    assert_eq!(found(RedactPattern::Phone), ["11 98765-4321"]);
    assert_eq!(found(RedactPattern::Cep), ["01.310-100"]);
    assert_eq!(found(RedactPattern::Date), ["2026-10-10", "10.10.2026"]);
    assert_eq!(found(RedactPattern::CreditCard), ["3782 822463 10005"]);
}

#[test]
fn a_phrase_is_found_every_time_on_a_crowded_page() {
    let dir = tempfile::tempdir().unwrap();
    let line = "x ".repeat(60);
    let lines: Vec<&str> = std::iter::repeat_n(line.as_str(), 12).collect();
    let d = pattern_page(dir.path(), &lines);
    let hits = d.find_redactions(&RedactQuery::Phrase("x".into())).unwrap();
    assert_eq!(hits.len(), 720, "sem o limite de 512 por página");
}

#[test]
fn sanitizing_metadata_also_clears_the_pages_private_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = hidden_copies(dir.path());
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sanitize(&SanitizeOptions {
        metadata: true,
        attachments: false,
        scripts: false,
        comments: false,
    })
    .unwrap();
    let out = dir.path().join("limpo.pdf");
    d.save(&out).unwrap();
    let raw = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(!raw.contains("PIECESECRET") && !raw.contains("THUMBSECRET"));
}

/// Texto alternativo por lista de propriedades nomeada (/Span /P0 BDC, com o /ActualText em
/// /Resources/Properties) e dentro de um formulário (XObject) desenhado sob a marca.
#[test]
fn alternate_text_in_property_lists_and_forms_goes_too() {
    let dir = tempfile::tempdir().unwrap();
    let content = "/Span /P0 BDC BT /F1 12 Tf 72 700 Td (visivel) Tj ET EMC \
        q 1 0 0 1 72 650 cm /Fm0 Do Q \
        BT /F1 12 Tf 72 600 Td (Texto que fica) Tj ET";
    let form = "/Span <</ActualText (FORMSECRET)>> BDC BT /F1 12 Tf 0 0 Td (forma) Tj ET EMC";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>/XObject<</Fm0 6 0 R>>/Properties<</P0<</ActualText(PROPSECRET)>>>>>>/Contents 4 0 R>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        format!("6 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 300 50]/Resources<</Font<</F1 5 0 R>>>>/Length {}>>stream\n{form}\nendstream endobj\n", form.len() + 1),
    ];
    let path = dir.path().join("alt.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    let raw = redact_top_and_save(&path);
    assert!(!raw.contains("PROPSECRET"), "propriedade nomeada ficou");
    assert!(
        !raw.contains("FORMSECRET"),
        "texto alternativo do formulário ficou"
    );
}

/// Texto alternativo em formulário dentro de formulário (com propriedade nomeada) e na árvore de
/// estrutura (PDF marcado) da página redigida.
#[test]
fn alternate_text_in_nested_forms_and_the_structure_tree_goes_too() {
    let dir = tempfile::tempdir().unwrap();
    let content = "/P <</MCID 0>> BDC BT /F1 12 Tf 72 700 Td (visivel) Tj ET EMC \
        q 1 0 0 1 72 650 cm /Fm0 Do Q \
        BT /F1 12 Tf 72 600 Td (Texto que fica) Tj ET";
    let outer = "/Span /P1 BDC /Fm1 Do EMC";
    let inner = "/Span <</ActualText (NESTEDSECRET)>> BDC BT /F1 12 Tf 0 0 Td (dentro) Tj ET EMC";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R/StructTreeRoot 8 0 R/MarkInfo<</Marked true>>>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>/XObject<</Fm0 6 0 R>>>>/Contents 4 0 R/StructParents 0>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        format!("6 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 300 50]/Resources<</XObject<</Fm1 7 0 R>>/Properties<</P1<</ActualText(FORMPROPSECRET)>>>>>>/Length {}>>stream\n{outer}\nendstream endobj\n", outer.len() + 1),
        format!("7 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 300 50]/Resources<</Font<</F1 5 0 R>>>>/Length {}>>stream\n{inner}\nendstream endobj\n", inner.len() + 1),
        "8 0 obj<</Type/StructTreeRoot/K 9 0 R>>endobj\n".into(),
        "9 0 obj<</Type/StructElem/S/P/P 8 0 R/Pg 3 0 R/ActualText(STRUCTSECRET)/Alt(ALTSECRET)/K 0>>endobj\n".into(),
    ];
    let path = dir.path().join("estrutura.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    let raw = redact_top_and_save(&path);
    for secret in [
        "NESTEDSECRET",
        "FORMPROPSECRET",
        "STRUCTSECRET",
        "ALTSECRET",
    ] {
        assert!(!raw.contains(secret), "{secret} ficou no arquivo");
    }
}

/// Desenho (ex.: texto em curvas, assinatura) que só passa por baixo da marca: por padrão sai
/// inteiro (seguro); com a opção de preservar, fica.
#[test]
fn drawings_touched_by_the_mark_go_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let line = "q 7.77 w 72 300 m 540 300 l S Q";
    let doc = |dir: &std::path::Path| {
        let content = format!("{line} BT /F1 12 Tf 72 700 Td (Texto) Tj ET");
        let objs = [
            "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
            "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
            "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n".into(),
            format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
            "5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        ];
        let path = dir.join("linha.pdf");
        std::fs::write(
            &path,
            format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
        )
        .unwrap();
        path
    };
    // a marca cobre só o meio da linha (y da tela = 792 - 300 = 492)
    let run = |keep: bool| -> bool {
        let path = doc(dir.path());
        let mut d = PdfDoc::open(&path, None).unwrap();
        mark(&mut d, vec![PdfRect::new(250.0, 480.0, 350.0, 505.0)]);
        d.apply_redactions(&RedactLook {
            keep_touched_drawings: keep,
            ..look()
        })
        .unwrap();
        // a linha aparece fora da marca?
        px(&d, 100, 492) != [255, 255, 255]
    };
    assert!(!run(false), "por padrão o desenho tocado sai inteiro");
    assert!(run(true), "com a opção, desenho só tocado fica");
}
