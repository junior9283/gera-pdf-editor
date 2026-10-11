mod common;
use common::write_sample;
use pdf_core::{FieldInfo, FieldKind, FieldProps, NewFieldKind, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn r(y: f32) -> PdfRect {
    PdfRect::new(100.0, y, 250.0, y + 22.0)
}

fn by_name<'a>(f: &'a [FieldInfo], name: &str) -> &'a FieldInfo {
    f.iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("{name} em {f:?}"))
}

#[test]
fn every_kind_is_created_saved_and_read_back_as_a_form_field() {
    let (_d, path, mut d) = doc();
    let kinds = [
        ("Texto1", NewFieldKind::Text),
        ("Caixa1", NewFieldKind::Checkbox),
        ("Grupo1", NewFieldKind::Radio),
        ("Lista suspensa1", NewFieldKind::Combo),
        ("Lista1", NewFieldKind::List),
        ("Assinatura1", NewFieldKind::Signature),
    ];
    for (i, (name, kind)) in kinds.iter().enumerate() {
        d.add_field(0, r(100.0 + 40.0 * i as f32), *kind, name)
            .unwrap();
    }
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let f = d.fields(0).unwrap();
    assert_eq!(f.len(), 6, "{f:?}");
    assert!(matches!(by_name(&f, "Texto1").kind, FieldKind::Text { .. }));
    assert!(matches!(
        by_name(&f, "Caixa1").kind,
        FieldKind::Checkbox { .. }
    ));
    assert!(matches!(&by_name(&f, "Grupo1").kind, FieldKind::Radio { on } if on == "Opção1"));
    assert!(matches!(
        by_name(&f, "Lista suspensa1").kind,
        FieldKind::Combo { .. }
    ));
    assert!(matches!(by_name(&f, "Lista1").kind, FieldKind::List { .. }));
    assert!(matches!(
        by_name(&f, "Assinatura1").kind,
        FieldKind::Signature
    ));
    let t = by_name(&f, "Texto1");
    assert!((t.rect.x0 - 100.0).abs() < 0.5 && (t.rect.y0 - 100.0).abs() < 0.5);
}

#[test]
fn created_fields_can_be_filled_and_checked() {
    let (_d, path, mut d) = doc();
    let t = d
        .add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    let c = d
        .add_field(
            0,
            PdfRect::new(100.0, 150.0, 114.0, 164.0),
            NewFieldKind::Checkbox,
            "Aceito",
        )
        .unwrap();
    d.set_field(0, t, "Maria").unwrap();
    let FieldKind::Checkbox { on } = by_name(&d.fields(0).unwrap(), "Aceito").kind.clone() else {
        panic!()
    };
    d.set_field(0, c, &on).unwrap();
    d.save(&path).unwrap();
    let f = PdfDoc::open(&path, None).unwrap().fields(0).unwrap();
    assert_eq!(by_name(&f, "Nome").value, "Maria");
    assert_eq!(by_name(&f, "Aceito").value, on);
}

#[test]
fn radios_with_the_same_group_name_form_one_group() {
    let (_d, path, mut d) = doc();
    let a = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 114.0, 114.0),
            NewFieldKind::Radio,
            "Plano",
        )
        .unwrap();
    let b = d
        .add_field(
            0,
            PdfRect::new(200.0, 100.0, 214.0, 114.0),
            NewFieldKind::Radio,
            "Plano",
        )
        .unwrap();
    let f = d.fields(0).unwrap();
    let ons: Vec<_> = f
        .iter()
        .filter(|x| x.name == "Plano")
        .map(|x| match &x.kind {
            FieldKind::Radio { on } => on.clone(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(ons, ["Opção1", "Opção2"]);
    d.set_field(0, a, "Opção1").unwrap();
    d.set_field(0, b, "Opção2").unwrap();
    d.save(&path).unwrap();
    let f = PdfDoc::open(&path, None).unwrap().fields(0).unwrap();
    assert!(f
        .iter()
        .filter(|x| x.name == "Plano")
        .all(|x| x.value == "Opção2"));
}

#[test]
fn properties_round_trip() {
    let (_d, _p, mut d) = doc();
    let id = d
        .add_field(0, r(100.0), NewFieldKind::Text, "Texto1")
        .unwrap();
    let mut p = d.field_props(0, id).unwrap();
    assert_eq!(p.name, "Texto1");
    p.name = "Endereço".into();
    p.tooltip = "Rua e número".into();
    p.required = true;
    p.multiline = true;
    p.max_len = Some(80);
    p.default = "Rua A".into();
    p.font_size = 10.0;
    d.set_field_props(0, id, &p).unwrap();
    assert_eq!(d.field_props(0, id).unwrap(), p);
    let f = d.fields(0).unwrap();
    let e = by_name(&f, "Endereço");
    assert_eq!(e.value, "Rua A");
    assert!(e.required && e.tooltip == "Rua e número");
    assert!(matches!(
        e.kind,
        FieldKind::Text {
            multiline: true,
            max_len: Some(80),
            ..
        }
    ));
}

#[test]
fn list_options_and_export_values_round_trip() {
    let (_d, _p, mut d) = doc();
    let id = d
        .add_field(0, r(100.0), NewFieldKind::Combo, "Estado")
        .unwrap();
    let mut p = d.field_props(0, id).unwrap();
    p.options = vec![
        ("SP".into(), "São Paulo".into()),
        ("RJ".into(), "Rio de Janeiro".into()),
    ];
    p.editable = true;
    d.set_field_props(0, id, &p).unwrap();
    let f = d.fields(0).unwrap();
    assert!(matches!(&by_name(&f, "Estado").kind,
        FieldKind::Combo { options, editable: true } if options.len() == 2 && options[1].1 == "Rio de Janeiro"));
    let c = d
        .add_field(
            0,
            PdfRect::new(300.0, 100.0, 314.0, 114.0),
            NewFieldKind::Checkbox,
            "Caixa1",
        )
        .unwrap();
    let mut p = d.field_props(0, c).unwrap();
    p.export = "Sim".into();
    p.checked_default = true;
    d.set_field_props(0, c, &p).unwrap();
    let f = d.fields(0).unwrap();
    let cx = by_name(&f, "Caixa1");
    assert!(matches!(&cx.kind, FieldKind::Checkbox { on } if on == "Sim"));
    assert_eq!(cx.value, "Sim", "marcada por padrão");
}

#[test]
fn bad_or_repeated_names_are_refused() {
    let (_d, _p, mut d) = doc();
    d.add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    assert!(d
        .add_field(0, r(140.0), NewFieldKind::Text, "Nome")
        .is_err());
    assert!(d.add_field(0, r(140.0), NewFieldKind::Text, "a.b").is_err());
    let id = d
        .add_field(0, r(180.0), NewFieldKind::Text, "Outro")
        .unwrap();
    let mut p = d.field_props(0, id).unwrap();
    p.name = "Nome".into();
    assert!(d.set_field_props(0, id, &p).is_err());
    assert_eq!(d.fields(0).unwrap().len(), 2, "nada criado nas recusas");
}

#[test]
fn move_delete_and_undo() {
    let (_d, path, mut d) = doc();
    let id = d
        .add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.move_field(0, id, PdfRect::new(50.0, 300.0, 250.0, 330.0))
        .unwrap();
    let f = d.fields(0).unwrap();
    assert!((f[0].rect.y0 - 300.0).abs() < 0.5 && (f[0].rect.width() - 200.0).abs() < 0.5);
    d.undo().unwrap();
    assert!((d.fields(0).unwrap()[0].rect.y0 - 100.0).abs() < 0.5);
    let id = d.fields(0).unwrap()[0].id;
    d.delete_field(0, id).unwrap();
    assert!(d.fields(0).unwrap().is_empty());
    d.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.windows(6).any(|w| w == b"(Nome)"),
        "campo excluído some do arquivo"
    );
}

#[test]
fn deleting_the_last_radio_removes_the_group() {
    let (_d, path, mut d) = doc();
    let a = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 114.0, 114.0),
            NewFieldKind::Radio,
            "G",
        )
        .unwrap();
    let b = d
        .add_field(
            0,
            PdfRect::new(200.0, 100.0, 214.0, 114.0),
            NewFieldKind::Radio,
            "G",
        )
        .unwrap();
    d.delete_field(0, a).unwrap();
    assert_eq!(d.fields(0).unwrap().len(), 1);
    let b = d
        .fields(0)
        .unwrap()
        .iter()
        .find(|f| f.name == "G")
        .map(|f| f.id)
        .unwrap_or(b);
    d.delete_field(0, b).unwrap();
    d.save(&path).unwrap();
    assert!(PdfDoc::open(&path, None)
        .unwrap()
        .fields(0)
        .unwrap()
        .is_empty());
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.windows(3).any(|w| w == b"(G)"),
        "grupo vazio fora de /Fields"
    );
}

#[test]
fn rotated_page_field_lands_where_clicked() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.add_field(0, r(100.0), NewFieldKind::Text, "T").unwrap();
    let f = &d.fields(0).unwrap()[0];
    assert!(
        (f.rect.x0 - 100.0).abs() < 0.5 && (f.rect.y0 - 100.0).abs() < 0.5,
        "{:?}",
        f.rect
    );
}

#[test]
fn encrypted_file_keeps_its_password_with_new_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::encrypted_sample(&dir);
    let mut d = PdfDoc::open(&path, Some("segredo")).unwrap();
    d.add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.save(&path).unwrap();
    assert!(PdfDoc::open(&path, None).is_err(), "continua pedindo senha");
    let d = PdfDoc::open(&path, Some("segredo")).unwrap();
    assert_eq!(d.fields(0).unwrap().len(), 1);
    let _ = FieldProps::default();
}

#[test]
fn a_broken_field_does_not_block_new_ones() {
    // campo com ciclo de /Parent (PDF malicioso): os outros campos continuam editáveis
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ciclo.pdf");
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace("/T (plano) /V /Off", "/Parent 6 0 R /V /Off");
    std::fs::write(&path, s).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut d = PdfDoc::open(&path, None).unwrap();
        let r = d.add_field(
            0,
            PdfRect::new(300.0, 50.0, 450.0, 72.0),
            NewFieldKind::Text,
            "Novo",
        );
        tx.send(r.is_ok()).unwrap();
    });
    let ok = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("travou");
    assert!(ok, "campo novo criado apesar do campo defeituoso");
}

/// Formulário de amostra com o campo "nome" pendurado num nó "endereco" (nome completo
/// "endereco.nome"), como os formulários de outros programas.
fn hierarchical() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.pdf");
    std::fs::write(&path, pdf_core::sample::form_pdf()).unwrap();
    let mut doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let mut fields = doc
        .catalog()
        .unwrap()
        .get_dict("AcroForm")
        .unwrap()
        .unwrap()
        .get_dict("Fields")
        .unwrap()
        .unwrap();
    let nome = fields.get_array(0).unwrap().unwrap();
    let parent = doc
        .add_object(&doc.new_object_from_str("<</T(endereco)/Kids[]>>").unwrap())
        .unwrap();
    parent
        .get_dict("Kids")
        .unwrap()
        .unwrap()
        .array_push(nome.clone())
        .unwrap();
    let mut n = nome.clone();
    n.dict_put("Parent", parent.clone()).unwrap();
    fields.array_put(0, parent).unwrap();
    doc.save(path.to_str().unwrap()).unwrap();
    (dir, path)
}

#[test]
fn hierarchical_fields_keep_their_properties_editable() {
    let (_dir, path) = hierarchical();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let f = by_name(&d.fields(0).unwrap(), "endereco.nome").clone();
    let mut p = d.field_props(0, f.id).unwrap();
    assert_eq!(p.name, "nome", "Propriedades mostram o nome do nó");
    p.tooltip = "Rua".into();
    d.set_field_props(0, f.id, &p).unwrap();
    assert_eq!(
        by_name(&d.fields(0).unwrap(), "endereco.nome").tooltip,
        "Rua"
    );
    assert!(
        d.add_field(0, r(300.0), NewFieldKind::Text, "endereco")
            .is_err(),
        "nome já é um nó"
    );
}

#[test]
fn ok_in_properties_keeps_the_typed_value_and_the_font() {
    let (_d, _p, mut d) = doc();
    let id = d
        .add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.set_field(0, id, "João").unwrap();
    let mut p = d.field_props(0, id).unwrap();
    p.tooltip = "Seu nome".into();
    d.set_field_props(0, id, &p).unwrap();
    assert_eq!(by_name(&d.fields(0).unwrap(), "Nome").value, "João");
    // fonte de outro programa: só o tamanho muda, e só se mudou
    let dir = tempfile::tempdir().unwrap();
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace(
            "/DA (/Helv 12 Tf 0 g) /P 3 0 R /F 4",
            "/DA (/TiRo 9 Tf 1 0 0 rg) /P 3 0 R /F 4",
        );
    assert!(s.contains("/TiRo"));
    let p2 = dir.path().join("tiro.pdf");
    std::fs::write(&p2, s.as_bytes()).unwrap();
    let mut d = PdfDoc::open(&p2, None).unwrap();
    let f = by_name(&d.fields(0).unwrap(), "nome").clone();
    let mut p = d.field_props(0, f.id).unwrap();
    assert_eq!(p.font_size, 9.0);
    p.tooltip = "x".into();
    d.set_field_props(0, f.id, &p).unwrap();
    p.font_size = 11.0;
    d.set_field_props(0, f.id, &p).unwrap();
    d.save(&p2).unwrap();
    let b = std::fs::read(&p2).unwrap();
    assert!(
        b.windows(19).any(|w| w == b"/TiRo 11 Tf 1 0 0 r"),
        "fonte e cor mantidas"
    );
}

#[test]
fn merged_radio_from_another_program_does_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.pdf");
    // "aceito" vira um botão de opção mesclado (campo e widget no mesmo objeto, sem /Kids)
    let s = String::from_utf8(pdf_core::sample::form_pdf()).unwrap();
    let s = s.replacen("/T (aceito)", "/Ff 49152 /T (aceito)", 1);
    std::fs::write(&path, s).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = d.add_field(
        0,
        PdfRect::new(400.0, 400.0, 414.0, 414.0),
        NewFieldKind::Radio,
        "aceito",
    );
    assert!(r.is_err(), "recusado com aviso, sem derrubar o worker");
}

#[test]
fn xfa_is_dropped_when_the_form_is_edited() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.pdf");
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replacen("/AcroForm <<", "/AcroForm << /XFA (x)", 1);
    std::fs::write(&path, s).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.add_field(0, r(400.0), NewFieldKind::Text, "Novo")
        .unwrap();
    assert!(!d.form_notes().unwrap().xfa);
}

#[test]
fn an_empty_name_gets_the_next_automatic_one() {
    let (_d, _p, mut d) = doc();
    d.add_field(0, r(100.0), NewFieldKind::Text, "").unwrap();
    d.add_field(0, r(140.0), NewFieldKind::Text, "").unwrap();
    let names: Vec<_> = d.fields(0).unwrap().into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["Texto1", "Texto2"]);
}

#[test]
fn editing_one_radio_keeps_the_group_default() {
    let (_d, _p, mut d) = doc();
    let a = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 114.0, 114.0),
            NewFieldKind::Radio,
            "G",
        )
        .unwrap();
    let b = d
        .add_field(
            0,
            PdfRect::new(200.0, 100.0, 214.0, 114.0),
            NewFieldKind::Radio,
            "G",
        )
        .unwrap();
    let mut pa = d.field_props(0, a).unwrap();
    pa.checked_default = true;
    d.set_field_props(0, a, &pa).unwrap();
    let mut pb = d.field_props(0, b).unwrap();
    pb.tooltip = "segunda".into();
    d.set_field_props(0, b, &pb).unwrap();
    assert!(
        d.field_props(0, a).unwrap().checked_default,
        "o padrão do grupo continua"
    );
}

fn props(d: &PdfDoc, name: &str) -> (i32, FieldProps) {
    let f = d.fields(0).unwrap();
    let id = by_name(&f, name).id;
    (id, d.field_props(0, id).unwrap())
}

#[test]
fn button_and_date_fields_are_created() {
    let (_d, path, mut d) = doc();
    d.add_field(0, r(100.0), NewFieldKind::Button, "Botão1")
        .unwrap();
    d.add_field(0, r(150.0), NewFieldKind::Date, "Data1")
        .unwrap();
    let (id, mut p) = props(&d, "Botão1");
    assert_eq!(p.button_label, "Botão1");
    p.button_label = "Limpar".into();
    p.reset_action = true;
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let f = d.fields(0).unwrap();
    assert_eq!(
        by_name(&f, "Botão1").kind,
        FieldKind::Button { reset: true }
    );
    let (_, p) = props(&d, "Botão1");
    assert_eq!((p.button_label.as_str(), p.reset_action), ("Limpar", true));
    let data = by_name(&f, "Data1");
    assert!(matches!(data.kind, FieldKind::Text { .. }));
    assert_eq!(
        data.format,
        pdf_core::FieldFormat::Date("dd/mm/yyyy".into())
    );
}

#[test]
fn format_validate_calc_props_round_trip() {
    let (_d, path, mut d) = doc();
    for (k, name) in ["A", "B", "Total"].into_iter().enumerate() {
        d.add_field(0, r(100.0 + 40.0 * k as f32), NewFieldKind::Text, name)
            .unwrap();
    }
    let (id, mut p) = props(&d, "Total");
    p.format = pdf_core::FieldFormat::Number {
        dec: 2,
        sep: 2,
        neg: 0,
        currency: "R$ ".into(),
        prepend: true,
    };
    p.validate = pdf_core::FieldValidate::Range {
        min: Some(0.0),
        max: None,
    };
    p.calc = pdf_core::FieldCalc::Simple {
        op: pdf_core::CalcOp::Sum,
        fields: vec!["A".into(), "B".into()],
    };
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let (id, back) = props(&d, "Total");
    assert_eq!(
        (&back.format, &back.validate, &back.calc),
        (&p.format, &p.validate, &p.calc)
    );
    assert_eq!(d.calc_order_names().unwrap(), ["Total"]);
    // preencher A recalcula o Total
    let a = by_name(&d.fields(0).unwrap(), "A").id;
    d.set_field(0, a, "7").unwrap();
    let fields = d.fields(0).unwrap();
    let total = by_name(&fields, "Total");
    assert_eq!(
        pdf_core::format_value(&total.format, &total.value)
            .unwrap()
            .text,
        "R$ 7,00"
    );
    let mut none = back.clone();
    none.calc = pdf_core::FieldCalc::None;
    d.set_field_props(0, id, &none).unwrap();
    assert!(
        d.calc_order_names().unwrap().is_empty(),
        "sai da ordem de cálculo"
    );
}

#[test]
fn calc_order_is_saved() {
    let (_d, _p, mut d) = doc();
    for (k, name) in ["X", "S1", "S2"].into_iter().enumerate() {
        d.add_field(0, r(100.0 + 40.0 * k as f32), NewFieldKind::Text, name)
            .unwrap();
    }
    for name in ["S1", "S2"] {
        let (id, mut p) = props(&d, name);
        p.calc = pdf_core::FieldCalc::Sfn("X * 2".into());
        d.set_field_props(0, id, &p).unwrap();
    }
    assert_eq!(d.calc_order_names().unwrap(), ["S1", "S2"]);
    d.prepare(pdf_core::PrepareOp::SetCalcOrder {
        names: vec!["S2".into(), "S1".into()],
    })
    .unwrap();
    assert_eq!(d.calc_order_names().unwrap(), ["S2", "S1"]);
    d.undo().unwrap();
    assert_eq!(d.calc_order_names().unwrap(), ["S1", "S2"]);
}

#[test]
fn row_column_and_manual_orders() {
    let (_d, path, mut d) = doc();
    // A à direita na primeira linha, B à esquerda dela, C embaixo de B
    for (name, x, y) in [
        ("A", 300.0, 100.0),
        ("B", 100.0, 100.0),
        ("C", 100.0, 300.0),
    ] {
        d.add_field(
            0,
            PdfRect::new(x, y, x + 120.0, y + 22.0),
            NewFieldKind::Text,
            name,
        )
        .unwrap();
    }
    let names =
        |d: &PdfDoc| -> Vec<String> { d.fields(0).unwrap().into_iter().map(|f| f.name).collect() };
    assert_eq!(
        names(&d),
        ["A", "B", "C"],
        "pela estrutura: a ordem de criação"
    );
    let set = |d: &mut PdfDoc, order| {
        d.prepare(pdf_core::PrepareOp::SetTabOrder { page: 0, order })
            .unwrap()
    };
    set(&mut d, pdf_core::TabOrder::Row);
    assert_eq!(names(&d), ["B", "A", "C"]);
    set(&mut d, pdf_core::TabOrder::Column);
    assert_eq!(names(&d), ["B", "C", "A"]);
    let ids: Vec<i32> = d.fields(0).unwrap().iter().map(|f| f.id).collect();
    set(
        &mut d,
        pdf_core::TabOrder::Manual(vec![ids[1], ids[2], ids[0]]),
    );
    assert_eq!(names(&d), ["C", "A", "B"]);
    d.save(&path).unwrap();
    let back = PdfDoc::open(&path, None).unwrap();
    assert_eq!(names(&back), ["C", "A", "B"]);
    assert_eq!(
        back.tab_order_kind(0).unwrap(),
        pdf_core::TabOrder::Structure
    );
    let mut d = back;
    set(&mut d, pdf_core::TabOrder::Row);
    d.undo().unwrap();
    assert_eq!(names(&d), ["C", "A", "B"], "desfazer volta a ordem");
}
