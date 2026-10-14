use pdf_core::{FieldCalc, FieldFormat, FieldInfo, FieldValidate, PdfDoc};

/// O valor como o Formatar do campo o mostra.
fn shown(f: &pdf_core::FieldInfo) -> String {
    pdf_core::format_value(&f.format, &f.value).map_or_else(|_| f.value.clone(), |f| f.text)
}

/// Formulário como o Acrobat grava: Preço (moeda, padrão 1500), Quantidade (1 a 10), Total =
/// Preço × Qtd (somente leitura, na ordem de cálculo) e Desconto = Total × 0,1.
fn order_form(dir: &tempfile::TempDir, total: &str) -> std::path::PathBuf {
    let field = |n: usize, name: &str, y: i32, aa: &str, more: &str| {
        format!(
            "<</Type/Annot/Subtype/Widget/FT/Tx/T({name})/Rect[100 {y} 300 {}]/P 3 0 R\
             /DA(/Helv 10 Tf 0 g)/F 4{more}/AA<<{aa}>>>>",
            y + 20
        )
        .replace("{n}", &n.to_string())
    };
    let js = |key: &str, code: &str| format!("/{key}<</S/JavaScript/JS({code})>>");
    let money = format!(
        "{}{}",
        js("F", r#"AFNumber_Format(2, 2, 0, 0, "R$ ", true);"#),
        js("K", r#"AFNumber_Keystroke(2, 2, 0, 0, "R$ ", true);"#)
    );
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R 7 0 R]/DA(/Helv 0 Tf 0 g)\
         /DR<</Font<</Helv 8 0 R>>>>/CO[6 0 R 7 0 R]>>>>"
            .to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R 5 0 R 6 0 R 7 0 R]>>"
            .to_string(),
        field(4, "Preco", 700, &money, "/V(1500)/DV(1500)"),
        field(
            5,
            "Qtd",
            650,
            &js("V", "AFRange_Validate(true, 1, true, 10);"),
            "",
        ),
        field(
            6,
            "Total",
            600,
            &format!(
                "{money}{}",
                js("C", &pdf_core::calc_script(&FieldCalc::Sfn(total.into())))
            ),
            "/Ff 1",
        ),
        field(
            7,
            "Desconto",
            550,
            &js(
                "C",
                &pdf_core::calc_script(&FieldCalc::Sfn("Total * 0.1".into())),
            ),
            "",
        ),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>".to_string(),
    ];
    let path = dir.path().join("pedido.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    path
}

fn field(d: &PdfDoc, name: &str) -> FieldInfo {
    d.fields(0)
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
}

/// O que a aparência (/AP /N) do widget `name` desenha.
fn appearance(path: &std::path::Path, name: &str) -> String {
    let doc = mupdf::pdf::PdfDocument::from_bytes(&std::fs::read(path).unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    let w = page
        .widgets()
        .find(|w| w.name().unwrap().as_deref() == Some(name))
        .unwrap();
    let ap = w.annotation().object().get_dict("AP").unwrap().unwrap();
    let n = ap.get_dict("N").unwrap().unwrap();
    String::from_utf8_lossy(&n.read_stream().unwrap()).into_owned()
}

#[test]
fn acrobat_made_form_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    let preco = field(&d, "Preco");
    assert!(matches!(
        preco.format,
        FieldFormat::Number { dec: 2, sep: 2, .. }
    ));
    assert_eq!(shown(&preco), "R$ 1.500,00");
    assert_eq!(preco.value, "1500");
    assert!(matches!(
        field(&d, "Qtd").validate,
        FieldValidate::Range {
            min: Some(_),
            max: Some(_)
        }
    ));
    assert_eq!(
        field(&d, "Total").calc,
        FieldCalc::Sfn("Preco * Qtd".into())
    );
}

#[test]
fn formatted_appearance_keeps_raw_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = order_form(&dir, "Preco * Qtd");
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = field(&d, "Preco").id;
    d.set_field(0, id, "1.234,5").unwrap();
    let preco = field(&d, "Preco");
    assert_eq!(preco.value, "1234.5", "/V cru");
    assert_eq!(shown(&preco), "R$ 1.234,50");
    d.save(&path).unwrap();
    assert!(
        appearance(&path, "Preco").contains("(R$ 1.234,50)"),
        "a página mostra o formatado"
    );
}

#[test]
fn range_refusal_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    let id = field(&d, "Qtd").id;
    let err = d.set_field(0, id, "11").unwrap_err().to_string();
    assert!(err.contains("menor ou igual a 10"), "{err}");
    assert_eq!(field(&d, "Qtd").value, "");
    let id = field(&d, "Preco").id;
    assert!(
        d.set_field(0, id, "abc").is_err(),
        "letras num campo Número"
    );
    assert_eq!(field(&d, "Preco").value, "1500");
}

#[test]
fn total_recalculates_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    let qtd = field(&d, "Qtd").id;
    d.set_field(0, qtd, "3").unwrap();
    let total = field(&d, "Total");
    assert_eq!(total.value, "4500");
    assert_eq!(shown(&total), "R$ 4.500,00");
    assert_eq!(
        field(&d, "Desconto").value,
        "450",
        "cadeia: Desconto depende do Total"
    );
    d.undo().unwrap();
    assert_eq!(field(&d, "Qtd").value, "");
    assert_eq!(field(&d, "Total").value, "", "uma etapa volta tudo");
    assert_eq!(field(&d, "Desconto").value, "");
    d.redo().unwrap();
    assert_eq!(field(&d, "Total").value, "4500");
}

#[test]
fn calc_cycle_stops() {
    let dir = tempfile::tempdir().unwrap();
    // Total depende do Desconto, que depende do Total
    let mut d = PdfDoc::open(&order_form(&dir, "Preco * Qtd + Desconto"), None).unwrap();
    let qtd = field(&d, "Qtd").id;
    d.set_field(0, qtd, "2").unwrap();
    assert!(field(&d, "Total")
        .value
        .parse::<f64>()
        .is_ok_and(f64::is_finite));
}

#[test]
fn acrobat_scripts_do_not_raise_the_unsupported_warning() {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    assert!(
        !d.form_notes().unwrap().scripts,
        "as chamadas AF são executadas"
    );
}

/// Formulário com os objetos dados (o campo é o objeto 4, na página).
fn one_field_form(dir: &tempfile::TempDir, field: &str, extra: &[String]) -> std::path::PathBuf {
    let mut objs = vec![
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]/DA(/Helv 0 Tf 0 g)\
         /DR<</Font<</Helv 5 0 R>>>>>>>>"
            .to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        field.to_string(),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>".to_string(),
    ];
    objs.extend(extra.iter().cloned());
    let path = dir.path().join("um.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    path
}

const WIDGET: &str = "/Type/Annot/Subtype/Widget/FT/Tx/T(C)/Rect[100 700 300 720]/P 3 0 R\
                      /DA(/Helv 10 Tf 0 g)/F 4";

#[test]
fn range_refuses_text_that_is_not_a_number() {
    let dir = tempfile::tempdir().unwrap();
    let path = one_field_form(
        &dir,
        &format!("<<{WIDGET}/AA<</V<</S/JavaScript/JS(AFRange_Validate(true, 1, true, 10);)>>>>>>"),
        &[],
    );
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.fields(0).unwrap()[0].id;
    assert!(d.set_field(0, id, "abc").is_err());
    assert_eq!(d.fields(0).unwrap()[0].value, "");
}

#[test]
fn chained_actions_are_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let path = one_field_form(
        &dir,
        &format!(
            "<<{WIDGET}/V(5)/AA<</F<</S/JavaScript/JS(AFNumber_Format(2, 2, 0, 0, \"R$ \", true);)\
             /Next<</S/JavaScript/JS(event.value = 'outro';)>>>>>>>>"
        ),
        &[],
    );
    let d = PdfDoc::open(&path, None).unwrap();
    let f = &d.fields(0).unwrap()[0];
    assert_eq!(
        f.format,
        FieldFormat::Custom,
        "a ação seguinte mudaria o valor"
    );
    assert!(d.form_notes().unwrap().scripts);
}

#[test]
fn a_script_stream_bomb_is_not_inflated() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&vec![b' '; 200 * 1024 * 1024]).unwrap();
    let packed = z.finish().unwrap();
    let path = dir.path().join("bomba.pdf");
    let mut pdf = pdf_core::sample::pdf_from_objects(&[
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        format!("<<{WIDGET}/AA<</F<</S/JavaScript/JS 5 0 R>>>>>>"),
        format!("<</Length {}/Filter/FlateDecode>>", packed.len()),
    ]);
    // o stream do objeto 5 vai à mão (o gerador de amostra só faz dicionários)
    let marker = format!("<</Length {}/Filter/FlateDecode>>", packed.len());
    let at = pdf
        .windows(marker.len())
        .position(|w| w == marker.as_bytes())
        .unwrap()
        + marker.len();
    let mut body = b"\nstream\n".to_vec();
    body.extend_from_slice(&packed);
    body.extend_from_slice(b"\nendstream");
    pdf.splice(at..at, body);
    std::fs::write(&path, &pdf).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let t = std::time::Instant::now();
    let f = d.fields(0).unwrap();
    assert_eq!(f[0].format, FieldFormat::Custom);
    assert!(t.elapsed().as_secs() < 2, "{:?}", t.elapsed());
}

#[test]
fn a_huge_calculation_cycle_finishes_quickly() {
    let dir = tempfile::tempdir().unwrap();
    const N: usize = 300;
    let mut fields = Vec::new();
    let mut annots = Vec::new();
    for k in 0..N {
        let prev = if k == 0 { N - 1 } else { k - 1 };
        let js = pdf_core::calc_script(&FieldCalc::Sfn(format!("F{prev} + 1")));
        fields.push(format!(
            "<</Type/Annot/Subtype/Widget/FT/Tx/T(F{k})/Rect[10 10 20 20]/P 3 0 R\
             /DA(/Helv 10 Tf 0 g)/F 4/AA<</C<</S/JavaScript/JS({js})>>>>>>"
        ));
        annots.push(format!("{} 0 R", 5 + k));
    }
    let mut objs = vec![
        format!(
            "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[{} 4 0 R]/DA(/Helv 0 Tf 0 g)>>>>",
            annots.join(" ")
        ),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[{} 4 0 R]>>",
            annots.join(" ")
        ),
        format!("<<{WIDGET}>>"),
    ];
    objs.extend(fields);
    let path = dir.path().join("ciclo.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d
        .fields(0)
        .unwrap()
        .iter()
        .find(|f| f.name == "C")
        .unwrap()
        .id;
    let t = std::time::Instant::now();
    d.set_field(0, id, "x").unwrap();
    assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
}

/// PDF com estes objetos a partir do 4 (1 a 3: catálogo com /Fields, árvore e página com
/// /Annots); a fonte Helv é o último objeto.
fn form_with(
    dir: &tempfile::TempDir,
    fields: &str,
    annots: &str,
    objs: &[&str],
) -> std::path::PathBuf {
    let font = 4 + objs.len();
    let mut all = vec![
        format!(
            "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[{fields}]/DA(/Helv 0 Tf 0 g)\
             /DR<</Font<</Helv {font} 0 R>>>>>>>>"
        ),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[{annots}]>>"),
    ];
    all.extend(objs.iter().map(|o| o.to_string()));
    all.push("<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>".into());
    let path = dir.path().join("form.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&all)).unwrap();
    path
}

const W: &str = "/Type/Annot/Subtype/Widget/P 3 0 R/DA(/Helv 10 Tf 0 g)/F 4";

#[test]
fn reset_keeps_formatting_and_recalculates() {
    let dir = tempfile::tempdir().unwrap();
    let path = order_form(&dir, "Preco * Qtd");
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_field(0, field(&d, "Qtd").id, "3").unwrap();
    assert_eq!(field(&d, "Total").value, "4500");
    d.reset_form().unwrap();
    assert_eq!(field(&d, "Preco").value, "1500", "volta ao padrão");
    assert_eq!(
        field(&d, "Total").value,
        "0",
        "o Acrobat recalcula depois de limpar"
    );
    d.save(&path).unwrap();
    assert!(appearance(&path, "Preco").contains("(R$ 1.500,00)"));
    assert!(appearance(&path, "Total").contains("(R$ 0,00)"));
}

#[test]
fn moving_or_reformatting_keeps_the_page_right() {
    let dir = tempfile::tempdir().unwrap();
    let path = order_form(&dir, "Preco * Qtd");
    let mut d = PdfDoc::open(&path, None).unwrap();
    let preco = field(&d, "Preco");
    let mut rect = preco.rect;
    rect.x1 += 40.0;
    d.move_field(0, preco.id, rect).unwrap();
    d.save(&path).unwrap();
    assert!(
        appearance(&path, "Preco").contains("(R$ 1.500,00)"),
        "depois de redimensionar"
    );
    let mut props = d.field_props(0, preco.id).unwrap();
    props.format = FieldFormat::None;
    d.set_field_props(0, preco.id, &props).unwrap();
    d.save(&path).unwrap();
    let ap = appearance(&path, "Preco");
    assert!(
        ap.contains("(1500)") && !ap.contains("R$"),
        "sem Formatar: {ap}"
    );
}

#[test]
fn sum_of_a_group_name_adds_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let w = W;
    let path = form_with(
        &dir,
        "4 0 R 7 0 R",
        "5 0 R 6 0 R 7 0 R",
        &[
            "<</FT/Tx/T(Item)/Kids[5 0 R 6 0 R]>>",
            &format!("<<{w}/Parent 4 0 R/T(0)/Rect[100 700 300 720]/V(2)>>"),
            &format!("<<{w}/Parent 4 0 R/T(1)/Rect[100 650 300 670]/V(3)>>"),
            &format!(
                "<<{w}/FT/Tx/T(Total)/Rect[100 600 300 620]\
                 /AA<</C<</S/JavaScript/JS(AFSimple_Calculate(\"SUM\", new Array (\"Item\"));)>>>>>>"
            ),
        ],
    );
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_field(0, field(&d, "Item.0").id, "4").unwrap();
    assert_eq!(
        field(&d, "Total").value,
        "7",
        "Item soma Item.0 e Item.1, como no Acrobat"
    );
}

#[test]
fn reset_button_clears_only_its_fields() {
    for (flags, a, b) in [("", "", "y"), ("/Flags 1", "x", "")] {
        let dir = tempfile::tempdir().unwrap();
        let w = W;
        let path = form_with(
            &dir,
            "4 0 R 5 0 R 6 0 R",
            "4 0 R 5 0 R 6 0 R",
            &[
                &format!("<<{w}/FT/Tx/T(A)/Rect[100 700 300 720]/V(x)>>"),
                &format!("<<{w}/FT/Tx/T(B)/Rect[100 650 300 670]/V(y)>>"),
                &format!(
                    "<<{w}/FT/Btn/Ff 65536/T(Limpar)/Rect[100 600 200 620]\
                     /A<</S/ResetForm/Fields[(A)]{flags}>>>>"
                ),
            ],
        );
        let mut d = PdfDoc::open(&path, None).unwrap();
        // o clique no botão executa a ação Redefinir dele
        let pdf_core::ActionKind::Reset { fields, exclude } = &field(&d, "Limpar").actions[0].1
        else {
            panic!("ação Redefinir");
        };
        d.reset_fields(fields, *exclude).unwrap();
        assert_eq!(
            (field(&d, "A").value.as_str(), field(&d, "B").value.as_str()),
            (a, b),
            "{flags}"
        );
    }
}

#[test]
fn a_huge_result_is_still_a_number() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    d.set_field(0, field(&d, "Preco").id, &format!("1{}", "0".repeat(300)))
        .unwrap();
    d.set_field(0, field(&d, "Qtd").id, "10").unwrap();
    let total = field(&d, "Total").value;
    assert!(total.parse::<f64>().is_ok_and(f64::is_finite), "{total}");
}

#[test]
fn an_invalid_expression_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&order_form(&dir, "Preco * Qtd"), None).unwrap();
    let id = field(&d, "Desconto").id;
    let mut props = d.field_props(0, id).unwrap();
    props.calc = FieldCalc::Sfn("Preco *".into());
    assert!(
        d.set_field_props(0, id, &props).is_err(),
        "o Acrobat recusa a expressão"
    );
}
