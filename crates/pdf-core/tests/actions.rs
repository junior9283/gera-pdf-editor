mod common;
use common::write_sample;
use pdf_core::{ActionKind as A, FieldTrigger as T, NewFieldKind, PageTrigger, PdfDoc, PdfRect};

fn doc(pages: usize) -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", pages, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn r(y: f32) -> PdfRect {
    PdfRect::new(100.0, y, 250.0, y + 22.0)
}

#[test]
fn each_action_round_trips() {
    let (_d, path, mut d) = doc(2);
    let id = d.add_field(0, r(100.0), NewFieldKind::Button, "B").unwrap();
    let list = vec![
        (T::MouseUp, A::GoTo { page: 1 }),
        (T::MouseUp, A::Uri("https://exemplo.com".into())),
        (
            T::MouseUp,
            A::ShowHide {
                fields: vec!["Nome".into(), "Idade".into()],
                hide: true,
            },
        ),
        (T::MouseDown, A::Named("NextPage".into())),
        (
            T::MouseEnter,
            A::ShowHide {
                fields: vec!["Dica".into()],
                hide: false,
            },
        ),
        (
            T::MouseExit,
            A::Reset {
                fields: vec!["Nome".into()],
                exclude: true,
            },
        ),
        (T::Focus, A::Launch("outro.pdf".into())),
        (T::Blur, A::JavaScript("app.alert('oi');".into())),
    ];
    let mut p = d.field_props(0, id).unwrap();
    p.actions = list.clone();
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.field_props(0, id).unwrap().actions, list);
    let f = d.fields(0).unwrap();
    assert_eq!(f[0].actions, list, "o preenchimento recebe as ações");
}

/// PDF de uma página com o botão `obj4` (e os objetos extra a partir do 6).
fn button_pdf(dir: &tempfile::TempDir, a: &str, extra: &[&str]) -> std::path::PathBuf {
    let mut objs = vec![
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        format!(
            "<</Type/Annot/Subtype/Widget/FT/Btn/Ff 65536/T(B)/Rect[100 700 200 720]/P 3 0 R/F 4/A {a}>>"
        ),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string(),
    ];
    objs.extend(extra.iter().map(|s| s.to_string()));
    let path = dir.path().join("b.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    path
}

#[test]
fn next_chain_is_read_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = button_pdf(
        &dir,
        "<</S/URI/URI(a)/Next[<</S/URI/URI(b)/Next<</S/URI/URI(c)>>>> <</S/Named/N/NextPage>>]>>",
        &[],
    );
    let d = PdfDoc::open(&path, None).unwrap();
    let f = d.fields(0).unwrap();
    let got: Vec<&A> = f[0].actions.iter().map(|(_, a)| a).collect();
    assert_eq!(
        got,
        [
            &A::Uri("a".into()),
            &A::Uri("b".into()),
            &A::Uri("c".into()),
            &A::Named("NextPage".into())
        ]
    );
}

#[test]
fn next_cycle_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    // 6 → 7 → 6…: termina, com cada ação uma vez
    let path = button_pdf(
        &dir,
        "6 0 R",
        &[
            "<</S/URI/URI(x)/Next 7 0 R>>",
            "<</S/URI/URI(y)/Next 6 0 R>>",
        ],
    );
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.fields(0).unwrap()[0].actions.len(), 2);
    // uma cadeia enorme é cortada no teto
    let big = format!(
        "<</S/URI/URI(a)/Next[{}]>>",
        "<</S/URI/URI(z)>> ".repeat(500)
    );
    let path = button_pdf(&dir, &big, &[]);
    let d = PdfDoc::open(&path, None).unwrap();
    assert!(d.fields(0).unwrap()[0].actions.len() <= 64);
}

#[test]
fn unknown_actions_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = button_pdf(&dir, "<</S/SubmitForm/F(https://x.y/z)/Flags 6>>", &[]);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.fields(0).unwrap()[0].id;
    let mut p = d.field_props(0, id).unwrap();
    assert!(matches!(&p.actions[0].1, A::Other { kind, .. } if kind == "SubmitForm"));
    // mudar outra coisa e acrescentar uma ação não perde a desconhecida
    p.actions.push((T::MouseUp, A::Named("Print".into())));
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let acts = d.field_props(0, id).unwrap().actions;
    assert!(matches!(&acts[0].1, A::Other { kind, .. } if kind == "SubmitForm"));
    assert_eq!(acts[1].1, A::Named("Print".into()));
}

#[test]
fn page_actions_round_trip() {
    let (_d, path, mut d) = doc(2);
    let list = vec![
        (PageTrigger::Open, A::GoTo { page: 1 }),
        (PageTrigger::Close, A::Uri("https://exemplo.com".into())),
    ];
    d.prepare(pdf_core::PrepareOp::SetPageActions {
        page: 0,
        actions: list.clone(),
    })
    .unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.page_actions(0).unwrap(), list);
    assert!(d.page_actions(1).unwrap().is_empty());
}

#[test]
fn show_hide_sets_the_hidden_flag() {
    let (_d, _p, mut d) = doc(1);
    d.add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.add_field(0, r(200.0), NewFieldKind::Text, "Idade")
        .unwrap();
    assert!(d.show_fields(&["Nome".into()], true).unwrap());
    let f = d.fields(0).unwrap();
    let nome = f.iter().find(|x| x.name == "Nome").unwrap();
    assert!(
        nome.hidden,
        "oculto, mas ainda listado (o Preparar o mostra)"
    );
    assert!(!f.iter().find(|x| x.name == "Idade").unwrap().hidden);
    assert!(
        !d.show_fields(&["Nome".into()], true).unwrap(),
        "já estava oculto"
    );
    assert!(d.show_fields(&["Nome".into()], false).unwrap());
    assert!(!d.fields(0).unwrap().iter().any(|x| x.hidden));
}

/// /A de um widget como texto (para ver o que ficou gravado).
fn widget_a(path: &std::path::Path) -> String {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    let w = page.widgets().next().unwrap();
    w.annotation()
        .object()
        .get_dict("A")
        .unwrap()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

#[test]
fn editing_one_trigger_keeps_the_others_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    // GoTo com /XYZ (o modelo do app só sabe /Fit): não pode virar /Fit se não foi mexido
    let path = button_pdf(&dir, "<</S/GoTo/D[3 0 R/XYZ 10 20 0]>>", &[]);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.fields(0).unwrap()[0].id;
    let mut p = d.field_props(0, id).unwrap();
    p.actions.push((T::MouseEnter, A::Named("Print".into())));
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    assert!(widget_a(&path).contains("XYZ"), "{}", widget_a(&path));
}

#[test]
fn actions_of_the_field_node_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let objs = [
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R]>>>>",
        "<</Type/Pages/Kids[3 0 R]/Count 1>>",
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[5 0 R]>>",
        "<</FT/Btn/Ff 65536/T(B)/Kids[5 0 R]/A<</S/URI/URI(https://x.y)>>>>",
        "<</Type/Annot/Subtype/Widget/Parent 4 0 R/Rect[100 700 200 720]/P 3 0 R/F 4>>",
    ];
    let path = dir.path().join("pai.pdf");
    std::fs::write(
        &path,
        pdf_core::sample::pdf_from_objects(&objs.map(String::from)),
    )
    .unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.fields(0).unwrap()[0].id;
    let mut p = d.field_props(0, id).unwrap();
    assert_eq!(p.actions.len(), 1, "a ação do nó do campo é lida");
    p.actions.clear();
    d.set_field_props(0, id, &p).unwrap();
    assert!(
        d.fields(0).unwrap()[0].actions.is_empty(),
        "e pode ser tirada"
    );
}

#[test]
fn a_go_to_outside_the_document_does_not_block_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = button_pdf(&dir, "<</S/GoTo/D[99 /Fit]>>", &[]);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.fields(0).unwrap()[0].id;
    let mut p = d.field_props(0, id).unwrap();
    assert!(
        matches!(p.actions[0].1, A::Other { .. }),
        "página que não existe: preservada"
    );
    p.actions.push((T::MouseEnter, A::Named("Print".into())));
    d.set_field_props(0, id, &p).unwrap();
}

#[test]
fn submit_action_round_trips() {
    use pdf_core::SubmitFormat as F;
    let dir = tempfile::tempdir().unwrap();
    let path = button_pdf(
        &dir,
        "<</S/SubmitForm/F(https://exemplo.com/recebe)/Flags 32>>",
        &[],
    );
    let mut d = PdfDoc::open(&path, None).unwrap();
    let f = d.fields(0).unwrap();
    assert_eq!(
        f[0].actions[0].1,
        A::Submit {
            url: "https://exemplo.com/recebe".into(),
            format: F::Xfdf
        }
    );
    let id = f[0].id;
    let mut p = d.field_props(0, id).unwrap();
    p.actions = vec![(
        T::MouseUp,
        A::Submit {
            url: "mailto:rh@exemplo.com".into(),
            format: F::Html,
        },
    )];
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.field_props(0, id).unwrap().actions, p.actions);
}

#[test]
fn submit_payloads_and_targets() {
    use pdf_core::SubmitFormat as F;
    let (_d, _p, mut d) = doc(1);
    let id = d
        .add_field(0, r(100.0), NewFieldKind::Text, "Nome")
        .unwrap();
    d.set_field(0, id, "Ana & Bia").unwrap();
    let (html, kind) = d.submit_payload(F::Html).unwrap();
    assert_eq!(kind, "application/x-www-form-urlencoded");
    assert_eq!(String::from_utf8(html).unwrap(), "Nome=Ana+%26+Bia");
    let (xfdf, _) = d.submit_payload(F::Xfdf).unwrap();
    assert!(String::from_utf8(xfdf)
        .unwrap()
        .contains("<value>Ana &amp; Bia</value>"));
    let (fdf, kind) = d.submit_payload(F::Fdf).unwrap();
    assert!(fdf.starts_with(b"%FDF-1.2") && kind == "application/vnd.fdf");
    let (pdf, _) = d.submit_payload(F::Pdf).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    // destinos: só http, https e mailto
    assert!(pdf_core::submit_target("https://a.b/c").is_some());
    assert!(pdf_core::submit_target("mailto:x@y.z").is_some());
    for bad in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ftp://a",
        "relativo.php",
        "",
    ] {
        assert!(pdf_core::submit_target(bad).is_none(), "{bad}");
    }
}
