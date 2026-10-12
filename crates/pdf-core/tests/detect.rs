use pdf_core::{FieldKind, PdfDoc};

/// Formulário "impresso": rótulos com linhas, uma caixa de seleção e uma célula de tabela.
fn drawn_form(dir: &tempfile::TempDir) -> std::path::PathBuf {
    // conteúdo em coordenadas do PDF (y para cima; página de 792 de altura)
    let content = "BT /F1 11 Tf 50 700 Td (Nome:) Tj ET \
                   0.5 w 100 698 m 400 698 l S \
                   BT /F1 11 Tf 50 650 Td (Assinatura:) Tj ET \
                   130 648 m 400 648 l S \
                   60 600 12 12 re S BT /F1 11 Tf 78 602 Td (Aceito) Tj ET \
                   50 520 200 24 re S BT /F1 11 Tf 50 548 Td (Obs) Tj ET";
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R\
         /Resources<</Font<</F1 5 0 R>>>>>>"
            .into(),
        format!("<</Length {}>>stream\n{content}\nendstream", content.len()),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>".into(),
    ];
    let path = dir.path().join("impresso.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    path
}

#[test]
fn drawn_form_is_detected_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = PdfDoc::open(&drawn_form(&dir), None).unwrap();
    assert_eq!(d.detect_fields(&[0]).unwrap(), 4);
    let f = d.fields(0).unwrap();
    let kind = |n: &str| f.iter().find(|x| x.name == n).map(|x| x.kind.clone());
    assert!(
        matches!(kind("Nome"), Some(FieldKind::Text { .. })),
        "{f:?}"
    );
    assert_eq!(kind("Assinatura"), Some(FieldKind::Signature));
    assert!(matches!(kind("Aceito"), Some(FieldKind::Checkbox { .. })));
    assert!(matches!(kind("Obs"), Some(FieldKind::Text { .. })));
    // de novo: nada se sobrepõe, nada é criado
    assert_eq!(d.detect_fields(&[0]).unwrap(), 0);
    d.undo().unwrap();
    assert!(d.fields(0).unwrap().is_empty(), "uma etapa do desfazer");
}

#[test]
fn huge_page_is_skipped_fast() {
    let dir = tempfile::tempdir().unwrap();
    let mut ops = String::new();
    for k in 0..30_000 {
        ops.push_str(&format!("{} 10 m {} 20 l S ", k % 600, k % 600));
    }
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>".into(),
        format!("<</Length {}>>stream\n{ops}\nendstream", ops.len()),
    ];
    let path = dir.path().join("enorme.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let t = std::time::Instant::now();
    assert_eq!(d.detect_fields(&[0]).unwrap(), 0);
    assert!(t.elapsed().as_secs_f32() < 5.0, "{:?}", t.elapsed());
}

#[test]
fn hostile_pages_finish_quickly() {
    let dir = tempfile::tempdir().unwrap();
    // um único caminho com 200 mil retângulos (o teto não pode ser só por caminho) e 40 mil
    // palavras de sublinhado
    let mut path = String::new();
    for k in 0..200_000 {
        path.push_str(&format!("{} {} 12 12 re ", k % 500, (k / 500) % 700));
    }
    path.push_str("S ");
    let mut text = String::from("BT /F1 4 Tf 0 0 Td ");
    for k in 0..40_000 {
        text.push_str(&format!(
            "({}) Tj 0 {} Td ",
            "_____ ",
            if k % 100 == 0 { 7 } else { 0 }
        ));
    }
    text.push_str("ET");
    // logo abaixo do teto de traços: 19 mil caixas de campo, com 40 mil palavras comuns
    let mut both = String::new();
    for k in 0..19_000 {
        both.push_str(&format!("{} {} 50 20 re S ", (k % 10) * 55, (k / 10) % 700));
    }
    both.push_str("BT /F1 3 Tf 0 0 Td ");
    for k in 0..40_000 {
        both.push_str(&format!(
            "(a ) Tj 0 {} Td ",
            if k % 200 == 0 { 4 } else { 0 }
        ));
    }
    both.push_str("ET");
    for (name, content) in [
        ("caminho.pdf", path),
        ("palavras.pdf", text),
        ("ambos.pdf", both),
    ] {
        let objs = vec![
            "<</Type/Catalog/Pages 2 0 R>>".to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R\
             /Resources<</Font<</F1 5 0 R>>>>>>"
                .into(),
            format!("<</Length {}>>stream\n{content}\nendstream", content.len()),
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".into(),
        ];
        let p = dir.path().join(name);
        std::fs::write(&p, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
        let mut d = PdfDoc::open(&p, None).unwrap();
        let t = std::time::Instant::now();
        let n = d.detect_fields(&[0]).unwrap();
        assert!(n <= 400, "{name}: {n}");
        assert!(t.elapsed().as_secs_f32() < 3.0, "{name}: {:?}", t.elapsed());
    }
}

#[test]
fn comments_are_not_detected_as_fields() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R 5 0 R]>>".into(),
        // comentário Retângulo e uma tinta horizontal, com aparência
        "<</Type/Annot/Subtype/Square/Rect[100 600 300 624]/C[1 0 0]/AP<</N 6 0 R>>>>".into(),
        "<</Type/Annot/Subtype/Ink/Rect[100 500 400 502]/InkList[[100 501 400 501]]/AP<</N 7 0 R>>>>".into(),
        "<</Type/XObject/Subtype/Form/BBox[100 600 300 624]/Length 26>>stream\n100 600 200 24 re S\nendstream".into(),
        "<</Type/XObject/Subtype/Form/BBox[100 500 400 502]/Length 24>>stream\n100 501 m 400 501 l S\nendstream".into(),
    ];
    let path = dir.path().join("comentarios.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(
        d.detect_fields(&[0]).unwrap(),
        0,
        "{:?}",
        d.fields(0).unwrap()
    );
}
