use pdf_core::{ImageEdit, PdfDoc, PdfRect};

/// Página Carta com texto, uma foto (Im0, 200×150 pt em x=72, y=500 do PDF) e, se `twice`, a
/// mesma foto desenhada duas vezes. `rotate`: /Rotate da página.
fn page_with_photo(dir: &std::path::Path, twice: bool, rotate: i32) -> std::path::PathBuf {
    use mupdf::pdf::PdfDocument;
    use mupdf::{Buffer, Colorspace, Image, Pixmap, Size};
    let path = dir.join(format!("foto{twice}{rotate}.pdf"));
    let mut doc = PdfDocument::new();
    let mut pix = Pixmap::new_with_w_h(&Colorspace::device_rgb(), 40, 30, false).unwrap();
    pix.clear_with(60).unwrap();
    let imref = doc.add_image(&Image::from_pixmap(&pix).unwrap()).unwrap();
    let page = doc
        .new_page(Size {
            width: 612.0,
            height: 792.0,
        })
        .unwrap();
    let mut o = page.object();
    let mut res = doc
        .new_object_from_str("<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>")
        .unwrap();
    let mut x = doc.new_dict().unwrap();
    x.dict_put("Im0", imref).unwrap();
    res.dict_put("XObject", x).unwrap();
    o.dict_put("Resources", res).unwrap();
    let mut content = String::from(
        "BT /F1 18 Tf 72 720 Td (Texto acima) Tj ET q 200 0 0 150 72 500 cm /Im0 Do Q ",
    );
    if twice {
        content.push_str("q 100 0 0 75 300 100 cm /Im0 Do Q ");
    }
    content.push_str("BT /F1 18 Tf 72 460 Td (Texto abaixo) Tj ET");
    let c = doc
        .add_stream(
            &Buffer::from_bytes(content.as_bytes()).unwrap(),
            None,
            false,
        )
        .unwrap();
    o.dict_put("Contents", c).unwrap();
    if rotate != 0 {
        o.dict_put("Rotate", mupdf::pdf::PdfObject::new_int(rotate).unwrap())
            .unwrap();
    }
    doc.save(path.to_str().unwrap()).unwrap();
    path
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn images_are_listed_with_their_place_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&page_with_photo(dir.path(), false, 0), None).unwrap();
    let imgs = d.page_images(0).unwrap();
    assert_eq!(imgs.len(), 1);
    let r = imgs[0].rect;
    // na tela (y para baixo): topo = 792 - 650 = 142
    assert!(
        close(r.x0, 72.0) && close(r.y0, 142.0) && close(r.x1, 272.0) && close(r.y1, 292.0),
        "{r:?}"
    );
    assert_eq!(imgs[0].name, "Im0");
    assert!(imgs[0].editable);
}

#[test]
fn moving_resizing_rotating_and_deleting_keep_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = page_with_photo(dir.path(), false, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.edit_image(
        0,
        "Im0",
        ImageEdit::Move {
            dx: 100.0,
            dy: 50.0,
        },
    )
    .unwrap();
    let r = d.page_images(0).unwrap()[0].rect;
    assert!(close(r.x0, 172.0) && close(r.y0, 192.0), "{r:?}");
    d.edit_image(
        0,
        "Im0",
        ImageEdit::Resize {
            rect: PdfRect::new(172.0, 192.0, 272.0, 267.0),
        },
    )
    .unwrap();
    let r = d.page_images(0).unwrap()[0].rect;
    assert!(close(r.width(), 100.0) && close(r.height(), 75.0), "{r:?}");
    d.edit_image(0, "Im0", ImageEdit::Rotate { clockwise: true })
        .unwrap();
    let r = d.page_images(0).unwrap()[0].rect;
    assert!(
        close(r.width(), 75.0) && close(r.height(), 100.0),
        "girada: {r:?}"
    );
    let text = d.page_text(0).unwrap();
    assert!(text.contains("Texto acima") && text.contains("Texto abaixo"));
    d.edit_image(0, "Im0", ImageEdit::Delete).unwrap();
    assert!(d.page_images(0).unwrap().is_empty());
    assert!(d.page_text(0).unwrap().contains("Texto abaixo"));
    d.save(&path).unwrap();
    // a imagem excluída não fica escondida no arquivo
    let raw = std::fs::read(&path).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("/Image"),
        "imagem ainda no arquivo"
    );
    assert!(
        !raw.windows(8).any(|w| w == b"/Subtype")
            || !String::from_utf8_lossy(&raw).contains("/Image"),
        "imagem ainda no arquivo"
    );
    let d = PdfDoc::open(&path, None).unwrap();
    assert!(d.page_images(0).unwrap().is_empty());
}

#[test]
fn undo_puts_the_image_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&page_with_photo(dir.path(), false, 0), None).unwrap();
    d.edit_image(0, "Im0", ImageEdit::Delete).unwrap();
    d.undo().unwrap();
    assert_eq!(d.page_images(0).unwrap().len(), 1);
}

#[test]
fn an_image_drawn_twice_is_not_editable() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&page_with_photo(dir.path(), true, 0), None).unwrap();
    let imgs = d.page_images(0).unwrap();
    assert_eq!(imgs.len(), 2);
    assert!(imgs.iter().all(|i| !i.editable));
    assert!(d.edit_image(0, "Im0", ImageEdit::Delete).is_err());
}

#[test]
fn rotated_pages_move_in_screen_space() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&page_with_photo(dir.path(), false, 90), None).unwrap();
    let before = d.page_images(0).unwrap()[0].rect;
    d.edit_image(0, "Im0", ImageEdit::Move { dx: 30.0, dy: 10.0 })
        .unwrap();
    let after = d.page_images(0).unwrap()[0].rect;
    assert!(
        close(after.x0 - before.x0, 30.0) && close(after.y0 - before.y0, 10.0),
        "{before:?} → {after:?}"
    );
}

#[test]
fn adding_and_replacing_images() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("nova.png");
    let mut pix =
        mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 80, 40, false).unwrap();
    pix.clear_with(200).unwrap();
    pix.save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    let mut d = PdfDoc::open(&page_with_photo(dir.path(), false, 0), None).unwrap();
    d.add_image(0, &png, (400.0, 600.0)).unwrap();
    let imgs = d.page_images(0).unwrap();
    assert_eq!(imgs.len(), 2);
    let added = imgs.iter().find(|i| i.name != "Im0").unwrap();
    let c = (
        (added.rect.x0 + added.rect.x1) / 2.0,
        (added.rect.y0 + added.rect.y1) / 2.0,
    );
    assert!(
        close(c.0, 400.0) && close(c.1, 600.0),
        "centrada no clique: {:?}",
        added.rect
    );
    assert!(close(added.rect.width() / added.rect.height(), 2.0));
    d.edit_image(0, "Im0", ImageEdit::Replace { path: png.clone() })
        .unwrap();
    let imgs = d.page_images(0).unwrap();
    assert_eq!(imgs.len(), 2);
    assert!(imgs.iter().all(|i| i.name != "Im0"), "a antiga saiu");
}

/// Duas páginas que herdam os recursos de /Pages; a 1ª tem o conteúdo em dois fluxos e a foto
/// também desenhada dentro de um formulário (Fm0) na 2ª.
fn inherited_and_split(dir: &std::path::Path) -> std::path::PathBuf {
    let img = "6 0 obj<</Type/XObject/Subtype/Image/Width 1/Height 1/ColorSpace/DeviceGray/BitsPerComponent 8/Length 1>>stream\n0\nendstream endobj\n";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R 7 0 R]/Count 2/Resources<</XObject<</Im0 6 0 R/Fm0 9 0 R>>>>/MediaBox[0 0 612 792]>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/Contents[4 0 R 5 0 R]>>endobj\n".into(),
        "4 0 obj<</Length 21>>stream\nq 100 0 0 100 50 50 \nendstream endobj\n".into(),
        "5 0 obj<</Length 13>>stream\ncm /Im0 Do Q\nendstream endobj\n".into(),
        img.into(),
        "7 0 obj<</Type/Page/Parent 2 0 R/Contents 8 0 R>>endobj\n".into(),
        "8 0 obj<</Length 9>>stream\n/Fm0 Do\n\nendstream endobj\n".into(),
        "9 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 612 792]/Resources<</XObject<</Im0 6 0 R>>>>/Length 30>>stream\nq 80 0 0 80 300 300 cm /Im0 Do Q\nendstream endobj\n".into(),
    ];
    let path = dir.join("herdado.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    path
}

/// Página com o conteúdo `content` e a imagem 1×1 Im0.
fn raw_page(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        "3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</XObject<</Im0 5 0 R>>>>/Contents 4 0 R>>endobj\n".into(),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/XObject/Subtype/Image/Width 1/Height 1/ColorSpace/DeviceGray/BitsPerComponent 8/Length 1>>stream\n0\nendstream endobj\n".into(),
    ];
    let path = dir.join("cru.pdf");
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    path
}

#[test]
fn compact_content_without_spaces_is_not_corrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = raw_page(
        dir.path(),
        "q 200 0 0 150 72 500 cm/Im0 Do 0 g Q",
    );
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.edit_image(0, "Im0", ImageEdit::Move { dx: 10.0, dy: 0.0 })
        .unwrap();
    let r = d.page_images(0).unwrap()[0].rect;
    assert!(
        close(r.x0, 82.0) && close(r.width(), 200.0) && close(r.height(), 150.0),
        "{r:?}"
    );
}

#[test]
fn repeated_edits_do_not_nest_graphics_states() {
    let dir = tempfile::tempdir().unwrap();
    let path = raw_page(dir.path(), "q 200 0 0 150 72 500 cm /Im0 Do Q");
    let mut d = PdfDoc::open(&path, None).unwrap();
    for _ in 0..40 {
        d.edit_image(0, "Im0", ImageEdit::Move { dx: 1.0, dy: 1.0 })
            .unwrap();
    }
    let r = d.page_images(0).unwrap()[0].rect;
    assert!(close(r.x0, 112.0) && close(r.y0, 182.0), "{r:?}");
    d.save(&path).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let qs = raw.windows(2).filter(|w| w == b"q ").count();
    assert!(qs <= 2, "{qs} níveis de q");
}

#[test]
fn split_content_inherited_resources_and_form_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = inherited_and_split(dir.path());
    let mut d = PdfDoc::open(&path, None).unwrap();
    let imgs = d.page_images(0).unwrap();
    assert_eq!(imgs.len(), 1);
    assert!(imgs[0].editable && close(imgs[0].rect.x0, 50.0), "{imgs:?}");
    // a da 2ª página está dentro de um formulário: contorno, mas não editável
    let form = d.page_images(1).unwrap();
    assert_eq!(form.len(), 1, "{form:?}");
    assert!(!form[0].editable && form[0].name.is_empty());
    d.edit_image(0, "Im0", ImageEdit::Delete).unwrap();
    assert!(d.page_images(0).unwrap().is_empty());
    assert_eq!(
        d.page_images(1).unwrap().len(),
        1,
        "a outra página não muda"
    );
    // acrescentar numa página de recursos herdados não mexe nos da outra
    let png = dir.path().join("p.png");
    let pix = mupdf::Pixmap::new_with_w_h(&mupdf::Colorspace::device_rgb(), 10, 10, false).unwrap();
    pix.save_as(png.to_str().unwrap(), mupdf::ImageFormat::PNG)
        .unwrap();
    d.add_image(0, &png, (300.0, 300.0)).unwrap();
    assert_eq!(d.page_images(0).unwrap().len(), 1);
    assert_eq!(d.page_images(1).unwrap().len(), 1);
}
