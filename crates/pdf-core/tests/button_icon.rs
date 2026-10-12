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
