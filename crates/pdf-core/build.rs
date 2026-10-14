//! No Windows (MinGW), o C++ do MuPDF e do Tesseract entra no binário: o `mupdf-sys` pede a
//! `stdc++` como biblioteca dinâmica, então uma pasta com só a `libstdc++.a` vem antes das do
//! compilador na busca. A busca vale para todo binário que usa o pdf-core (o app e os testes), e
//! nenhum depende de libstdc++-6.dll.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("windows-gnu") {
        return;
    }
    let gcc = std::env::var("CC_x86_64_pc_windows_gnu")
        .unwrap_or_else(|_| "x86_64-w64-mingw32-gcc".into());
    let out = Command::new(&gcc)
        .arg("-print-file-name=libstdc++.a")
        .output()
        .expect("compilador MinGW não encontrado");
    let lib = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    assert!(lib.is_absolute(), "libstdc++.a do MinGW não encontrada");
    let dir = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("static-cxx");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&lib, dir.join("libstdc++.a")).unwrap();
    println!("cargo:rustc-link-search=native={}", dir.display());
}
