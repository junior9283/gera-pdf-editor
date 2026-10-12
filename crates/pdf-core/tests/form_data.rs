mod common;
use common::write_sample;
use pdf_core::{DataFormat, NewFieldKind, PdfDoc, PdfRect};

fn r(y: f32) -> PdfRect {
    PdfRect::new(100.0, y, 250.0, y + 22.0)
}

/// Formulário com texto (acentos), caixa, lista e um nome hierárquico.
fn form(dir: &tempfile::TempDir) -> (std::path::PathBuf, PdfDoc) {
    let path = write_sample(dir, "f.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.add_field(0, r(140.0), NewFieldKind::Checkbox, "Aceito")
        .unwrap();
    d.add_field(0, r(180.0), NewFieldKind::Text, "Cidade")
        .unwrap();
    d.add_field(0, r(220.0), NewFieldKind::Signature, "Assinatura")
        .unwrap();
    d.save(&path).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let f = d.fields(0).unwrap();
    let id = |n: &str| f.iter().find(|x| x.name == n).unwrap().id;
    d.set_field(0, id("Nome"), "José Ação & <Cia>").unwrap();
    d.set_field(0, id("Aceito"), "Sim").unwrap();
    d.set_field(0, id("Cidade"), "Recife").unwrap();
    (path, d)
}

fn value(d: &PdfDoc, name: &str) -> String {
    d.fields(0)
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
        .value
}

#[test]
fn export_import_round_trip_in_every_format() {
    for format in DataFormat::ALL {
        let dir = tempfile::tempdir().unwrap();
        let (_p, mut d) = form(&dir);
        let data = dir.path().join(format!("dados.{}", format.extension()));
        assert_eq!(d.export_form_data(&data, format).unwrap(), 3, "{format:?}");
        d.reset_form().unwrap();
        assert_eq!(value(&d, "Nome"), "");
        let report = d.import_form_data(&data).unwrap();
        assert_eq!(report.filled, 3, "{format:?}");
        assert_eq!(value(&d, "Nome"), "José Ação & <Cia>", "{format:?}");
        assert_eq!(value(&d, "Aceito"), "Sim", "{format:?}");
        assert_eq!(value(&d, "Cidade"), "Recife", "{format:?}");
    }
}

#[test]
fn import_is_one_undo_step_and_skips_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = form(&dir);
    let data = dir.path().join("x.xfdf");
    std::fs::write(
        &data,
        r#"<?xml version="1.0"?><xfdf xmlns="http://ns.adobe.com/xfdf/"><fields>
           <field name="Nome"><value>Maria</value></field>
           <field name="Inexistente"><value>x</value></field>
           <field name="Assinatura"><value>falsa</value></field>
           </fields></xfdf>"#,
    )
    .unwrap();
    let report = d.import_form_data(&data).unwrap();
    assert_eq!(
        (report.filled, report.skipped),
        (1, 2),
        "assinatura não é preenchida"
    );
    assert_eq!(value(&d, "Nome"), "Maria");
    d.undo().unwrap();
    assert_eq!(value(&d, "Nome"), "José Ação & <Cia>");
}

#[test]
fn hierarchical_names_and_hostile_files() {
    let dir = tempfile::tempdir().unwrap();
    // XML com nomes hierárquicos e original
    let xml = dir.path().join("h.xml");
    std::fs::write(
        &xml,
        r#"<?xml version="1.0"?><fields xmlns:xfdf="http://ns.adobe.com/xfdf-transition/">
           <end><rua>Av. Brasil</rua></end><_1x xfdf:original="1x">um</_1x></fields>"#,
    )
    .unwrap();
    let (_p, mut d) = form(&dir);
    let report = d.import_form_data(&xml);
    // nenhum desses existe no formulário: recusado com a mensagem certa, sem pânico
    assert!(report.is_err());
    // XML muito fundo, FDF com ciclo e arquivo enorme
    let deep = dir.path().join("fundo.xml");
    std::fs::write(&deep, format!("<fields>{}</fields>", "<a>".repeat(100))).unwrap();
    assert!(d.import_form_data(&deep).is_err());
    let cycle = dir.path().join("ciclo.fdf");
    let objs = [
        "<</FDF<</Fields[2 0 R]>>>>".to_string(),
        "<</T(Nome)/Kids[2 0 R]/V(x)>>".into(),
    ];
    let mut bytes = pdf_core::sample::pdf_from_objects(&objs);
    bytes[..8].copy_from_slice(b"%FDF-1.2");
    std::fs::write(&cycle, bytes).unwrap();
    let t = std::time::Instant::now();
    let _ = d.import_form_data(&cycle);
    assert!(t.elapsed().as_secs_f32() < 2.0);
    let big = dir.path().join("grande.txt");
    std::fs::write(&big, "a".repeat(3 * 1024 * 1024)).unwrap();
    assert!(d.import_form_data(&big).is_err());
}

#[test]
fn merge_three_files_into_csv() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut d) = form(&dir);
    d.save(&path).unwrap();
    let xfdf = dir.path().join("b.xfdf");
    std::fs::write(
        &xfdf,
        r#"<xfdf><fields><field name="Nome"><value>Ana, "A"</value></field>
           <field name="Extra"><value>1</value></field></fields></xfdf>"#,
    )
    .unwrap();
    let txt = dir.path().join("c.txt");
    std::fs::write(&txt, "Cidade\tNome\nOlinda\tBia\n").unwrap();
    let csv = dir.path().join("planilha.csv");
    assert_eq!(
        pdf_core::merge_form_data(&[path, xfdf, txt], &csv).unwrap(),
        3
    );
    let text = std::fs::read_to_string(&csv).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Nome,Aceito,Cidade,Extra");
    assert_eq!(lines[1], "José Ação & <Cia>,Sim,Recife,");
    assert_eq!(lines[2], "\"Ana, \"\"A\"\"\",,,1");
    assert_eq!(lines[3], "Bia,,Olinda,");
}

#[test]
fn flatten_bakes_fields_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut d) = form(&dir);
    d.flatten(false).unwrap();
    assert!(d.fields(0).unwrap().is_empty(), "os campos viram conteúdo");
    assert!(
        d.page_text(0).unwrap().contains("Recife"),
        "o valor ficou na página"
    );
    d.undo().unwrap();
    assert_eq!(value(&d, "Cidade"), "Recife", "desfazer volta o formulário");
    d.redo().unwrap();
    d.save(&path).unwrap();
    let saved = PdfDoc::open(&path, None).unwrap();
    assert!(saved.fields(0).unwrap().is_empty());
    assert!(saved.page_text(0).unwrap().contains("Recife"));
}

#[test]
fn spreadsheet_formulas_are_neutralized() {
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("f.txt");
    std::fs::write(
        &txt,
        "A\tB\tC\tD\tE\tF\tG\n=1+1\t+cmd\t-2\t@SUM(A1)\tnormal\t =2\t|cmd\n",
    )
    .unwrap();
    let csv = dir.path().join("p.csv");
    pdf_core::merge_form_data(&[txt], &csv).unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(
        text.lines().nth(1).unwrap(),
        "'=1+1,'+cmd,'-2,'@SUM(A1),normal,' =2,'|cmd"
    );
}

#[test]
fn merging_many_wide_files_is_fast() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for k in 0..150 {
        let names: Vec<String> = (0..3000).map(|j| format!("c{}_{j}", k % 4)).collect();
        let values: Vec<&str> = (0..3000).map(|_| "v").collect();
        let p = dir.path().join(format!("{k}.txt"));
        std::fs::write(&p, format!("{}\n{}\n", names.join("\t"), values.join("\t"))).unwrap();
        files.push(p);
    }
    let t = std::time::Instant::now();
    pdf_core::merge_form_data(&files, &dir.path().join("p.csv")).unwrap();
    assert!(t.elapsed().as_secs_f32() < 3.0, "{:?}", t.elapsed());
}
