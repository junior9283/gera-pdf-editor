mod common;
use common::write_sample;
use pdf_core::{stamp_image_from_file, AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect};

/// Algum pixel escuro dentro de `r` (coordenadas do MuPDF) na página renderizada a 1:1.
fn ink_inside(d: &PdfDoc, r: PdfRect) -> bool {
    let img = d.render(0, 1.0, false).unwrap();
    let (w, h) = (img.width as usize, img.height as usize);
    let (x0, x1) = (
        (r.x0.max(0.0) as usize).min(w),
        (r.x1.max(0.0) as usize).min(w),
    );
    let (y0, y1) = (
        (r.y0.max(0.0) as usize).min(h),
        (r.y1.max(0.0) as usize).min(h),
    );
    (y0..y1).any(|y| (x0..x1).any(|x| img.rgb[(y * w + x) * 3..][..3].iter().all(|&c| c < 128)))
}

#[test]
fn pdf_and_image_become_stamp_images() {
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_sample(&dir, "logo.pdf", 3, 0);
    let png = dir.path().join("a.png");
    let (w, h) = stamp_image_from_file(&pdf, 1, &png).unwrap();
    assert!(png.exists());
    assert!(
        w.max(h) <= 300.0 && (w / h - 612.0 / 792.0).abs() < 0.02,
        "{w} {h}"
    );
    // a imagem gerada serve de fonte (carimbo de imagem)
    let again = dir.path().join("b.png");
    let (w2, h2) = stamp_image_from_file(&png, 0, &again).unwrap();
    assert!((w2 / h2 - w / h).abs() < 0.02);
    assert!(
        stamp_image_from_file(&pdf, 7, &again).is_err(),
        "página inexistente"
    );
    let junk = dir.path().join("lixo.pdf");
    std::fs::write(&junk, b"nada").unwrap();
    assert!(stamp_image_from_file(&junk, 0, &again).is_err());
}

#[test]
fn custom_stamp_is_drawn_and_survives_save_and_move() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_sample(&dir, "logo.pdf", 1, 0);
    let png = dir.path().join("logo.png");
    stamp_image_from_file(&src, 0, &png).unwrap();
    let path = write_sample(&dir, "doc.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    // a amostra tem texto no alto da página: o carimbo vai num canto vazio
    let rect = PdfRect::new(300.0, 500.0, 500.0, 760.0);
    d.apply(AnnotOp::Create(NewAnnot::CustomStamp {
        page: 0,
        rect,
        image: png.clone(),
        name: "Logotipo".into(),
    }))
    .unwrap();
    let a = d.annotations(0).unwrap().remove(0);
    assert_eq!(a.kind, AnnotKind::Stamp);
    assert!(ink_inside(&d, a.rect), "o carimbo aparece");
    d.apply(AnnotOp::Move {
        page: 0,
        id: a.id,
        dx: -250.0,
        dy: 0.0,
    })
    .unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let moved = d.annotations(0).unwrap().remove(0);
    assert!(
        ink_inside(&d, moved.rect),
        "continua desenhado depois de mover e salvar"
    );
}
