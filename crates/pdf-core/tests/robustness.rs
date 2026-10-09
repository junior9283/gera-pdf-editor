mod common;
use common::write_sample;
use mupdf::pdf::{Encryption, PdfDocument, PdfWriteOptions};
use pdf_core::sample::sample_pdf;
use pdf_core::{NewAnnot, PdfDoc, PdfError};

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn bad_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let good = sample_pdf(2, 0);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("vazio.pdf", vec![]),
        (
            "lixo.pdf",
            (0..4096u32).map(|i| (i * 31 % 251) as u8).collect(),
        ),
        ("so_cabecalho.pdf", b"%PDF-1.4\n".to_vec()),
        ("truncado.pdf", good[..good.len() / 2].to_vec()),
    ];
    for (name, bytes) in cases {
        let path = write(&dir, name, &bytes);
        match PdfDoc::open(&path, None) {
            Err(_) => {}
            Ok(doc) => {
                // se o MuPDF conseguir reparar, nada pode dar panic
                let n = doc.page_count().unwrap_or(0);
                for p in 0..n.min(3) {
                    let _ = doc.page_size(p);
                    let _ = doc.render(p, 0.5, false);
                }
            }
        }
    }
    assert!(PdfDoc::open(&dir.path().join("nao_existe.pdf"), None).is_err());
}

#[test]
fn zero_page_document_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "zero.pdf", 0, 0);
    if let Ok(doc) = PdfDoc::open(&path, None) {
        assert_eq!(doc.page_count().unwrap_or(0), 0);
        assert!(doc.summary().map(|s| s.sizes.is_empty()).unwrap_or(true));
    }
}

fn encrypted(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("senha.pdf");
    let doc = PdfDocument::from_bytes(&sample_pdf(1, 0)).unwrap();
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::Aes256)
        .set_owner_password("dono")
        .set_user_password("segredo");
    doc.save_with_options(path.to_str().unwrap(), opts).unwrap();
    path
}

#[test]
fn password_flow() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted(&dir);
    assert!(matches!(
        PdfDoc::open(&path, None),
        Err(PdfError::PasswordRequired)
    ));
    assert!(matches!(
        PdfDoc::open(&path, Some("errada")),
        Err(PdfError::WrongPassword)
    ));
    assert_eq!(
        PdfDoc::open(&path, Some("segredo"))
            .unwrap()
            .page_count()
            .unwrap(),
        1
    );
}

#[test]
fn saving_keeps_the_file_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted(&dir);
    let mut doc = PdfDoc::open(&path, Some("segredo")).unwrap();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(
        &[NewAnnot::Highlight {
            page: 0,
            rects: vec![hit],
        }],
        &path,
    )
    .unwrap();
    assert!(matches!(
        PdfDoc::open(&path, None),
        Err(PdfError::PasswordRequired)
    ));
    let again = PdfDoc::open(&path, Some("segredo")).unwrap();
    assert_eq!(again.annotations(0).unwrap().len(), 1);
}

#[test]
fn accented_path_with_spaces_opens_and_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "Relatório final (1) — ação.pdf", 2, 0);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    let hit = doc.search(1, "Hello").unwrap()[0];
    doc.save_with(
        &[NewAnnot::Highlight {
            page: 1,
            rects: vec![hit],
        }],
        &path,
    )
    .unwrap();
    assert_eq!(
        PdfDoc::open(&path, None)
            .unwrap()
            .annotations(1)
            .unwrap()
            .len(),
        1
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        leftovers.len(),
        1,
        "sem arquivo temporário sobrando: {leftovers:?}"
    );
}

#[test]
fn outline_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 3, 0), None).unwrap();
    let outline = doc.outline().unwrap();
    assert_eq!(outline.len(), 3);
    assert_eq!(outline[1].title, "Capitulo 2");
    assert_eq!(outline[1].page, Some(1));
    assert_eq!(outline[1].depth, 0);
    let s = doc.summary().unwrap();
    assert_eq!(s.sizes.len(), 3);
    assert_eq!(s.outline.len(), 3);
    assert!(s.annots.is_empty());
}
