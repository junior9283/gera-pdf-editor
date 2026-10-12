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
