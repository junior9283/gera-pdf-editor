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

/// /AS (aparência escolhida) e /V de cada botão do grupo "plano", lidos direto do arquivo.
fn radio_states(path: &std::path::Path) -> Vec<String> {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    page.widgets()
        .filter(|w| w.name().unwrap().as_deref() == Some("plano"))
        .map(|w| {
            let o = w.annotation().object();
            String::from_utf8_lossy(&o.get_dict("AS").unwrap().unwrap().as_name().unwrap())
                .into_owned()
        })
        .collect()
}

#[test]
fn choosing_another_radio_turns_the_previous_off() {
    let (_d, path, mut d) = form();
    let radios: Vec<_> = d
        .fields(0)
        .unwrap()
        .into_iter()
        .filter(|x| x.name == "plano")
        .collect();
    let on = |f: &FieldInfo| match &f.kind {
        FieldKind::Radio { on } => on.clone(),
        _ => panic!(),
    };
    d.set_field(0, radios[0].id, &on(&radios[0])).unwrap();
    d.set_field(0, radios[1].id, &on(&radios[1])).unwrap();
    d.save(&path).unwrap();
    assert_eq!(radio_states(&path), ["Off", "completo"]);
    d.undo().ok(); // histórico zerado ao salvar: nada a desfazer
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_field(0, radios[0].id, &on(&radios[0])).unwrap();
    d.undo().unwrap();
    d.save(&path).unwrap();
    assert_eq!(
        radio_states(&path),
        ["Off", "completo"],
        "desfazer volta o grupo inteiro"
    );
}

#[test]
fn button_values_are_saved_as_pdf_names() {
    let (_d, path, mut d) = form();
    let id = field(&d.fields(0).unwrap(), "aceito").id;
    d.set_field(0, id, "Sim").unwrap();
    d.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes
            .windows(7)
            .any(|w| w == b"/V /Sim" || &w[..6] == b"/V/Sim"),
        "valor como nome"
    );
}

#[test]
fn reset_form_clears_radio_groups() {
    let (_d, _p, mut d) = form();
    let r = d
        .fields(0)
        .unwrap()
        .into_iter()
        .find(|x| x.name == "plano")
        .unwrap();
    let FieldKind::Radio { on } = r.kind.clone() else {
        panic!()
    };
    d.set_field(0, r.id, &on).unwrap();
    d.reset_form().unwrap();
    assert!(d
        .fields(0)
        .unwrap()
        .iter()
        .filter(|x| x.name == "plano")
        .all(|x| x.value == "Off"));
}

#[test]
fn hidden_widgets_are_marked_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.pdf");
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace(
            "/T (nome) /V (Ana) /Rect [100 700 300 720] /DA (/Helv 12 Tf 0 g) /P 3 0 R /F 4 >>",
            "/T (nome) /V (Ana) /Rect [100 700 300 720] /DA (/Helv 12 Tf 0 g) /P 3 0 R /F 6 >>",
        );
    std::fs::write(&path, s).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    // listado (o Preparar mostra e o cálculo usa), mas oculto: o preenchimento o pula
    let f = d.fields(0).unwrap();
    assert!(f
        .iter()
        .find(|f| f.name == "nome")
        .is_some_and(|f| f.hidden));
}

#[test]
fn printing_the_document_only_keeps_current_field_values() {
    let (dir, _p, mut d) = form();
    let id = field(&d.fields(0).unwrap(), "nome").id;
    d.set_field(0, id, "Valor atual").unwrap();
    let dest = dir.path().join("imprimir.pdf");
    d.export_for_print(&dest, false).unwrap();
    let printed = PdfDoc::open(&dest, None).unwrap();
    assert_eq!(
        field(&printed.fields(0).unwrap(), "nome").value,
        "Valor atual"
    );
}

#[test]
fn forms_with_xfa_or_calculation_scripts_are_flagged() {
    let (_d, _p, d) = form();
    assert_eq!(d.form_notes().unwrap(), pdf_core::FormNotes::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.pdf");
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace(
            "/T (nome) /V (Ana)",
            "/T (nome) /AA << /C << /S /JavaScript /JS (1) >> >> /V (Ana)",
        );
    let s = s.replacen("/AcroForm <<", "/AcroForm << /XFA (x)", 1);
    std::fs::write(&path, s).unwrap();
    let notes = PdfDoc::open(&path, None).unwrap().form_notes().unwrap();
    assert!(notes.xfa && notes.scripts);
}

#[test]
fn a_parent_cycle_does_not_hang() {
    // /Parent apontando para si mesmo, sem /T: PDF malicioso não pode travar o app
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ciclo.pdf");
    let s = String::from_utf8(pdf_core::sample::form_pdf())
        .unwrap()
        .replace("/T (plano) /V /Off", "/Parent 6 0 R /V /Off");
    std::fs::write(&path, s).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut d = PdfDoc::open(&path, None).unwrap();
        let _ = d.form_notes();
        let radio = d
            .fields(0)
            .unwrap()
            .into_iter()
            .find(|f| matches!(f.kind, FieldKind::Radio { .. }));
        if let Some(r) = radio {
            let _ = d.set_field(0, r.id, "basico");
        }
        let _ = d.reset_form();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("travou num ciclo de /Parent");
}
