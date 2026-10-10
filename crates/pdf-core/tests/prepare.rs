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
    assert!(d.add_field(0, r(140.0), NewFieldKind::Text, "  ").is_err());
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
        let r = d.add_field(0, PdfRect::new(300.0, 50.0, 450.0, 72.0), NewFieldKind::Text, "Novo");
        tx.send(r.is_ok()).unwrap();
    });
    let ok = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("travou");
    assert!(ok, "campo novo criado apesar do campo defeituoso");
}
