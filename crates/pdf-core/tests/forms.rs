use pdf_core::{FieldInfo, FieldKind, PdfDoc};

fn form() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("form.pdf");
    std::fs::write(&path, pdf_core::sample::form_pdf()).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn field<'a>(fields: &'a [FieldInfo], name: &str) -> &'a FieldInfo {
    fields.iter().find(|f| f.name == name).unwrap()
}

#[test]
fn lists_every_kind_of_field() {
    let (_d, _p, d) = form();
    let f = d.fields(0).unwrap();
    assert_eq!(f.len(), 6, "{f:?}");
    assert_eq!(field(&f, "nome").value, "Ana");
    assert!(matches!(
        field(&f, "nome").kind,
        FieldKind::Text {
            multiline: false,
            ..
        }
    ));
    assert!(matches!(&field(&f, "aceito").kind, FieldKind::Checkbox { on } if on == "Sim"));
    let radios: Vec<_> = f.iter().filter(|x| x.name == "plano").collect();
    assert_eq!(radios.len(), 2);
    assert!(
        matches!(&field(&f, "estado").kind, FieldKind::Combo { options, .. } if options.len() == 3)
    );
    match &field(&f, "cores").kind {
        FieldKind::List { options } => {
            assert_eq!(options[1], ("vm".to_owned(), "Vermelho".to_owned()));
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn widgets_are_not_annotations() {
    let (_d, _p, d) = form();
    assert!(d.annotations(0).unwrap().is_empty());
}

#[test]
fn text_values_round_trip() {
    let (_d, path, mut d) = form();
    let id = field(&d.fields(0).unwrap(), "nome").id;
    d.set_field(0, id, "José da Conceição").unwrap();
    d.save(&path).unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    assert_eq!(
        field(&again.fields(0).unwrap(), "nome").value,
        "José da Conceição"
    );
}

#[test]
fn checkbox_combo_and_list_values_are_saved() {
    let (_d, path, mut d) = form();
    let f = d.fields(0).unwrap();
    d.set_field(0, field(&f, "aceito").id, "Sim").unwrap();
    d.set_field(0, field(&f, "estado").id, "MG").unwrap();
    d.set_field(0, field(&f, "cores").id, "vm").unwrap();
    d.save(&path).unwrap();
    let f = PdfDoc::open(&path, None).unwrap().fields(0).unwrap();
    assert_eq!(field(&f, "aceito").value, "Sim");
    assert_eq!(field(&f, "estado").value, "MG");
    assert_eq!(field(&f, "cores").value, "vm");
}

#[test]
fn radio_buttons_pick_one() {
    let (_d, path, mut d) = form();
    let radios: Vec<_> = d
        .fields(0)
        .unwrap()
        .into_iter()
        .filter(|x| x.name == "plano")
        .collect();
    let FieldKind::Radio { on } = &radios[1].kind else {
        panic!()
    };
    d.set_field(0, radios[1].id, on).unwrap();
    d.save(&path).unwrap();
    let f = PdfDoc::open(&path, None).unwrap().fields(0).unwrap();
    let values: Vec<_> = f
        .iter()
        .filter(|x| x.name == "plano")
        .map(|x| x.value.clone())
        .collect();
    assert_eq!(
        values,
        ["completo", "completo"],
        "o valor do grupo é o estado escolhido"
    );
}

#[test]
fn field_undo_restores_the_previous_value() {
    let (_d, _p, mut d) = form();
    let id = field(&d.fields(0).unwrap(), "nome").id;
    d.set_field(0, id, "Outra").unwrap();
    assert!(d.is_dirty());
    d.undo().unwrap();
    assert_eq!(field(&d.fields(0).unwrap(), "nome").value, "Ana");
    d.redo().unwrap();
    assert_eq!(field(&d.fields(0).unwrap(), "nome").value, "Outra");
}

#[test]
fn reset_form_restores_defaults_with_undo() {
    let (_d, _p, mut d) = form();
    let id = field(&d.fields(0).unwrap(), "estado").id;
    d.set_field(0, id, "MG").unwrap();
    d.reset_form().unwrap();
    assert_ne!(field(&d.fields(0).unwrap(), "estado").value, "MG");
    d.undo().unwrap();
    assert_eq!(field(&d.fields(0).unwrap(), "estado").value, "MG");
}

#[test]
fn readonly_fields_refuse_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.pdf");
    let bytes = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace("/T (nome) /V (Ana)", "/T (nome) /Ff 1 /V (Ana)");
    // mantém o tamanho dos objetos: o xref aponta por posição
    let fixed = bytes.replacen("/F 4 >>", "/F 4>>", 1);
    std::fs::write(&path, fixed).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let nome = field(&d.fields(0).unwrap(), "nome").clone();
    assert!(nome.readonly);
    assert!(d.set_field(0, nome.id, "x").is_err());
}
