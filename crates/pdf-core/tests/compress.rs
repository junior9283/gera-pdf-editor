mod common;
use pdf_core::{CompressLevel, PdfDoc};

/// PDF de uma página (carta) com uma imagem RGB de `w`×`h` sem compressão (degradê), e uma
/// máscara 1 bit que deve ficar como está.
fn photo_pdf(w: usize, h: usize) -> Vec<u8> {
    photo_pdf_cs(w, h, "/DeviceRGB")
}

/// Como `photo_pdf`, com o espaço de cor da foto (o objeto 7 é um perfil ICC de 3 componentes).
fn photo_pdf_cs(w: usize, h: usize, cs: &str) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            rgb.extend([
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) % 256) as u8,
            ]);
        }
    }
    let mask = vec![0xAAu8; 64 * 8];
    let content = b"q 612 0 0 792 0 0 cm /Im1 Do Q q 100 0 0 100 50 50 cm /Mk Do Q".to_vec();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Im1 5 0 R /Mk 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
    ];
    let stream = |dict: String, data: &[u8]| {
        let mut o = format!("{dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        o.extend(data);
        o.extend(b"\nendstream");
        o
    };
    objs.push(stream("<<".into(), &content));
    objs.push(stream(
        format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace {cs} /BitsPerComponent 8"),
        &rgb,
    ));
    objs.push(stream(
        "<< /Type /XObject /Subtype /Image /Width 64 /Height 64 /ImageMask true /BitsPerComponent 1".into(),
        &mask,
    ));
    objs.push(stream("<< /N 3 /Alternate /DeviceRGB".into(), b""));
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn image_size(path: &std::path::Path, name: &str) -> (i32, i32) {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let img = doc
        .find_page(0)
        .unwrap()
        .get_dict("Resources")
        .unwrap()
        .unwrap()
        .get_dict("XObject")
        .unwrap()
        .unwrap()
        .get_dict(name)
        .unwrap()
        .unwrap();
    let int = |k: &str| img.get_dict(k).unwrap().unwrap().as_int().unwrap();
    (int("Width"), int("Height"))
}

#[test]
fn each_level_shrinks_more_and_large_images_are_downsampled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foto.pdf");
    std::fs::write(&path, photo_pdf(2400, 3000)).unwrap();
    let original = std::fs::metadata(&path).unwrap().len();
    let d = PdfDoc::open(&path, None).unwrap();
    let low = dir.path().join("baixa.pdf");
    let mid = dir.path().join("media.pdf");
    let high = dir.path().join("alta.pdf");
    let r_low = d.compress_to(&low, CompressLevel::Low).unwrap();
    let r_mid = d.compress_to(&mid, CompressLevel::Medium).unwrap();
    let r_high = d.compress_to(&high, CompressLevel::High).unwrap();
    assert_eq!(r_low.before, original);
    assert!(r_low.after < original, "{r_low:?}");
    assert!(r_mid.after < r_low.after, "{r_mid:?} {r_low:?}");
    assert!(r_high.after < r_mid.after, "{r_high:?} {r_mid:?}");
    assert_eq!(r_low.images, 0);
    assert_eq!(r_mid.images, 1);
    // 2400 px em 8,5 pol = 282 ppp → 150 ppp = 1275 px
    let (w, h) = image_size(&mid, "Im1");
    assert!(
        (1270..=1280).contains(&w) && (1590..=1600).contains(&h),
        "{w}x{h}"
    );
    assert_eq!(image_size(&mid, "Mk"), (64, 64), "máscara fica");
    // o resultado abre e renderiza
    let c = PdfDoc::open(&high, None).unwrap();
    assert!(c.render(0, 0.2, false).is_ok());
    // o documento aberto não mudou
    assert_eq!(std::fs::metadata(&path).unwrap().len(), original);
}

#[test]
fn small_images_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.pdf");
    std::fs::write(&path, photo_pdf(600, 700)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let out = dir.path().join("c.pdf");
    let r = d.compress_to(&out, CompressLevel::Medium).unwrap();
    assert_eq!(r.images, 0, "70 ppp: nada a reduzir");
    assert_eq!(image_size(&out, "Im1"), (600, 700));
}

#[test]
fn protected_stays_protected_and_signed_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    // foto protegida por senha (a amostra de texto já é pequena demais para encolher)
    let path = dir.path().join("protegida.pdf");
    std::fs::write(&path, photo_pdf(2400, 3000)).unwrap();
    let mut p = PdfDoc::open(&path, None).unwrap();
    p.set_protection(Some(pdf_core::Protection {
        open_password: "segredo".into(),
        owner_password: String::new(),
        permissions: pdf_core::Permissions::ALL,
    }))
    .unwrap();
    p.save(&path).unwrap();
    let d = PdfDoc::open(&path, Some("segredo")).unwrap();
    let out = dir.path().join("c.pdf");
    d.compress_to(&out, CompressLevel::High).unwrap();
    assert!(PdfDoc::open(&out, None).is_err(), "continua com senha");
    assert!(PdfDoc::open(&out, Some("segredo")).is_ok());

    let p = common::write_sample(&dir, "s.pdf", 1, 0);
    let id = pdf_core::DigitalId::create_self_signed("A", "", "", "x", &dir.path().join("a.pfx"))
        .unwrap();
    let mut s = PdfDoc::open(&p, None).unwrap();
    s.sign_digitally(
        pdf_core::SignPlace::New {
            page: 0,
            rect: pdf_core::PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        &id,
        "",
        "",
    )
    .unwrap();
    assert!(s
        .compress_to(&dir.path().join("x.pdf"), CompressLevel::Low)
        .is_err());
}

#[test]
fn compressed_photo_looks_like_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foto.pdf");
    std::fs::write(&path, photo_pdf(2400, 3000)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let out = dir.path().join("alta.pdf");
    d.compress_to(&out, CompressLevel::High).unwrap();
    let a = d.render(0, 0.25, false).unwrap();
    let b = PdfDoc::open(&out, None)
        .unwrap()
        .render(0, 0.25, false)
        .unwrap();
    assert_eq!((a.width, a.height), (b.width, b.height));
    // diferença média por canal pequena (JPEG com perda, mas mesmas cores e posições)
    let diff: u64 = a
        .rgb
        .iter()
        .zip(&b.rgb)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    let mean = diff as f64 / a.rgb.len() as f64;
    assert!(mean < 6.0, "diferença média {mean}");
}

#[test]
fn an_already_small_file_is_not_written_bigger() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foto.pdf");
    std::fs::write(&path, photo_pdf(1200, 1500)).unwrap();
    let first = dir.path().join("c1.pdf");
    PdfDoc::open(&path, None)
        .unwrap()
        .compress_to(&first, CompressLevel::Low)
        .unwrap();
    // comprimir de novo o que já está comprimido não pode gravar um arquivo maior (nem igual)
    let again = dir.path().join("c2.pdf");
    let r = PdfDoc::open(&first, None)
        .unwrap()
        .compress_to(&again, CompressLevel::Low);
    assert!(r.is_err(), "{r:?}");
    assert!(!again.exists());
}

#[test]
fn icc_colour_spaces_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("icc.pdf");
    std::fs::write(&path, photo_pdf_cs(2400, 3000, "[/ICCBased 7 0 R]")).unwrap();
    let dest = dir.path().join("c.pdf");
    let r = PdfDoc::open(&path, None)
        .unwrap()
        .compress_to(&dest, CompressLevel::Medium)
        .unwrap();
    assert_eq!(r.images, 1);
    let bytes = std::fs::read(&dest).unwrap();
    assert!(
        bytes.windows(8).any(|w| w == b"ICCBased"),
        "o perfil continua"
    );
}
