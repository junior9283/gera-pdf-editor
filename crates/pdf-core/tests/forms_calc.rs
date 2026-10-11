use pdf_core::{FieldCalc, FieldFormat, FieldInfo, FieldValidate, PdfDoc};

/// Formulário como o Acrobat grava: Preço (moeda), Quantidade (1 a 10), Total = Preço × Qtd
/// (somente leitura, na ordem de cálculo) e Desconto = Total × 0,1.
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
        field(4, "Preco", 700, &money, "/V(1500)"),
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
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
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
    assert_eq!(preco.display, "R$ 1.500,00");
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
    assert_eq!(preco.display, "R$ 1.234,50");
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
    assert_eq!(total.display, "R$ 4.500,00");
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
