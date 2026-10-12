mod common;
use common::write_sample;
use pdf_core::{ActionKind, FieldKind, FieldTrigger, NewFieldKind, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn png(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let p = dir.path().join("foto.png");
    let mut pix =
        mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 80, 40, false).unwrap();
    pix.clear_with(120).unwrap();
    pix.save_as(p.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    p
}

fn normal_ap(path: &std::path::Path) -> String {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    let w = page.widgets().next().unwrap();
    let ap = w.annotation().object().get_dict("AP").unwrap().unwrap();
    let n = ap.get_dict("N").unwrap().unwrap();
    String::from_utf8_lossy(&n.read_stream().unwrap()).into_owned()
}

#[test]
fn image_field_is_created_like_acrobat() {
    let (_d, _p, mut d) = doc();
    let id = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 220.0, 190.0),
            NewFieldKind::Image,
            "",
        )
        .unwrap();
    let f = d.fields(0).unwrap();
    assert_eq!(f[0].name, "Imagem1");
    assert!(matches!(f[0].kind, FieldKind::Button));
    let p = d.field_props(0, id).unwrap();
    assert_eq!(p.layout, 1, "somente ícone");
    assert_eq!(
        p.actions,
        vec![(FieldTrigger::MouseUp, ActionKind::ImportIcon)]
    );
    assert!(!p.has_icon);
    // a aparência é a do app desde a criação (o redesenho segue o layout)
    d.save(&_p).unwrap();
    let raw = mupdf::pdf::PdfDocument::open(_p.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(0).unwrap();
    let obj = page.widgets().next().unwrap().annotation().object();
    assert!(obj.get_dict("GeraAP").unwrap().is_some());
}

#[test]
fn button_icon_is_drawn() {
    let (dir, path, mut d) = doc();
    let id = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 220.0, 190.0),
            NewFieldKind::Image,
            "",
        )
        .unwrap();
    d.set_button_icon(0, id, &png(&dir)).unwrap();
    assert!(d.field_props(0, id).unwrap().has_icon);
    d.save(&path).unwrap();
    assert!(
        normal_ap(&path).contains("/Icon Do"),
        "a aparência desenha o ícone"
    );
    // mover o campo não perde o desenho (o MuPDF não sabe desenhar /MK /I)
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.move_field(0, id, PdfRect::new(150.0, 150.0, 300.0, 260.0))
        .unwrap();
    d.save(&path).unwrap();
    assert!(normal_ap(&path).contains("/Icon Do"));
    // desfazer volta ao campo sem imagem
    let mut d = PdfDoc::open(&path, None).unwrap();
    let mut p = d.field_props(0, id).unwrap();
    p.has_icon = false;
    p.layout = 0;
    p.button_label = "Foto".into();
    d.set_field_props(0, id, &p).unwrap();
    d.save(&path).unwrap();
    let ap = normal_ap(&path);
    assert!(!ap.contains("/Icon Do") && ap.contains("Foto"), "{ap}");
}

#[test]
fn invalid_image_changes_nothing() {
    let (dir, _p, mut d) = doc();
    let id = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 220.0, 190.0),
            NewFieldKind::Image,
            "",
        )
        .unwrap();
    let bad = dir.path().join("x.png");
    std::fs::write(&bad, b"isto nao e uma imagem").unwrap();
    assert!(d.set_button_icon(0, id, &bad).is_err());
    assert!(!d.field_props(0, id).unwrap().has_icon);
}

#[test]
fn buttons_from_other_programs_keep_their_appearance() {
    let dir = tempfile::tempdir().unwrap();
    let w = "/Type/Annot/Subtype/Widget/P 3 0 R/F 4";
    let objs = [
        "<</Type/Catalog/Pages 2 0 R/AcroForm<</Fields[4 0 R 5 0 R 6 0 R]>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R 5 0 R 6 0 R]>>".into(),
        // botão com ícone e aparência pressionada (/D) de outro programa
        format!("<<{w}/FT/Btn/Ff 65536/T(Foto)/Rect[100 600 200 700]/H/P/MK<</TP 1/I 7 0 R>>/AP<</N 8 0 R/D 9 0 R>>>>"),
        // caixa de seleção com /MK /TP (não é botão de ação)
        format!("<<{w}/FT/Btn/T(Ok)/Rect[300 600 320 620]/V/Off/AS/Off/MK<</TP 1>>/AP<</N<</Yes 8 0 R/Off 9 0 R>>>>>>"),
        format!("<<{w}/FT/Tx/T(Nome)/Rect[100 400 250 420]/DA(/Helv 10 Tf 0 g)>>"),
        "<</Type/XObject/Subtype/Form/BBox[0 0 10 10]/Length 0>>stream\nendstream".into(),
        "<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Length 0>>stream\nendstream".into(),
        "<</Type/XObject/Subtype/Form/BBox[0 0 100 100]/Length 0>>stream\nendstream".into(),
    ];
    let path = dir.path().join("outros.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let nome = d
        .fields(0)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "Nome")
        .unwrap();
    d.move_field(0, nome.id, PdfRect::new(100.0, 300.0, 250.0, 320.0))
        .unwrap();
    d.save(&path).unwrap();
    let raw = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(0).unwrap();
    for w in page.widgets() {
        let ap = w.annotation().object().get_dict("AP").unwrap().unwrap();
        match w.name().unwrap().unwrap().as_str() {
            "Foto" => assert!(ap.get_dict("D").unwrap().is_some(), "aparência pressionada"),
            "Ok" => assert!(
                ap.get_dict("N").unwrap().unwrap().is_dict().unwrap(),
                "estados"
            ),
            _ => {}
        }
    }
}

#[test]
fn icon_buttons_on_rotated_pages_are_drawn_upright() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "g.pdf", 1, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 300.0, 140.0),
            NewFieldKind::Image,
            "",
        )
        .unwrap();
    d.set_button_icon(0, id, &png(&dir)).unwrap();
    d.save(&path).unwrap();
    let raw = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(0).unwrap();
    let w = page.widgets().next().unwrap();
    let obj = w.annotation().object();
    let n = obj
        .get_dict("AP")
        .unwrap()
        .unwrap()
        .get_dict("N")
        .unwrap()
        .unwrap();
    assert!(
        n.get_dict("Matrix").unwrap().is_some(),
        "a aparência gira com /MK /R"
    );
    // a caixa da aparência tem os lados do /Rect trocados (desenho em pé)
    let rect = obj.get_dict("Rect").unwrap().unwrap();
    let side = |o: &mupdf::pdf::PdfObject, a: i32, b: i32| {
        (o.get_array(b).unwrap().unwrap().as_float().unwrap()
            - o.get_array(a).unwrap().unwrap().as_float().unwrap())
        .abs()
    };
    let bbox = n.get_dict("BBox").unwrap().unwrap();
    assert!((side(&bbox, 0, 2) - side(&rect, 1, 3)).abs() < 0.5);
}
