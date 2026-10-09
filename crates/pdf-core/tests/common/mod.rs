use mupdf::pdf::{Encryption, PdfDocument, PdfWriteOptions};
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
