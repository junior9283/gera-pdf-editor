use mupdf::pdf::{AnnotationFlags, Encryption, PdfDocument, PdfWriteOptions};
use pdf_core::sample::sample_pdf;
use std::path::PathBuf;
use tempfile::TempDir;

pub fn write_sample(dir: &TempDir, name: &str, pages: usize, rotate: i32) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, sample_pdf(pages, rotate)).unwrap();
    path
}

/// PDF de 1 página protegido: senha do usuário "segredo", do dono "dono".
#[allow(dead_code)] // nem todo arquivo de teste usa (cada um compila `common` à parte)
pub fn encrypted_sample(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("senha.pdf");
    let doc = PdfDocument::from_bytes(&sample_pdf(1, 0)).unwrap();
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::Aes256)
        .set_owner_password("dono")
        .set_user_password("segredo");
    doc.save_with_options(path.to_str().unwrap(), opts).unwrap();
    path
}

/// Liga a flag Hidden em todas as anotações do arquivo (anotação "escondida de fábrica").
#[allow(dead_code)]
pub fn hide_all_annotations(path: &std::path::Path) {
    let doc = PdfDocument::open(path.to_str().unwrap()).unwrap();
    for p in 0..doc.page_count().unwrap() {
        let mut page = doc.load_pdf_page(p).unwrap();
        let all: Vec<_> = page.annotations().collect();
        for mut a in all {
            let flags = a.flags().unwrap() | AnnotationFlags::IS_HIDDEN;
            a.set_flags(flags).unwrap();
            a.update().unwrap();
        }
        page.update().unwrap();
    }
    doc.save(path.to_str().unwrap()).unwrap();
}

/// Quantas anotações o arquivo tem, contando as escondidas.
#[allow(dead_code)]
pub fn count_raw_annotations(path: &std::path::Path) -> usize {
    let doc = PdfDocument::open(path.to_str().unwrap()).unwrap();
    (0..doc.page_count().unwrap())
        .map(|p| doc.load_pdf_page(p).unwrap().annotations().count())
        .sum()
}
