use pdf_core::sample::sample_pdf;
use std::path::PathBuf;
use tempfile::TempDir;

pub fn write_sample(dir: &TempDir, name: &str, pages: usize, rotate: i32) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, sample_pdf(pages, rotate)).unwrap();
    path
}
