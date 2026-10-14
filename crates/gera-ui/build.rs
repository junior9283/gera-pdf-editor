//! No Windows (MinGW):
//! - o C++ do MuPDF e do Tesseract entra no .exe: o `mupdf-sys` pede a `stdc++` como biblioteca
//!   dinâmica, então uma pasta com só a `libstdc++.a` vem antes das do compilador na busca e o
//!   .exe não depende de libstdc++-6.dll;
//! - o ícone e as informações de versão (Propriedades do arquivo) vão como recurso, compilado
//!   pelo `windres`.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icons/gera-pdf-editor.ico");
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
    resource();
}

/// Ícone e VERSIONINFO do .exe.
fn resource() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../packaging/icons/gera-pdf-editor.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let comma = format!("{},0", nums.join(","));
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041604B0"
    BEGIN
      VALUE "CompanyName", "Gera PDF Editor"
      VALUE "FileDescription", "Gera PDF Editor"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "GeraPDFEditor"
      VALUE "LegalCopyright", "AGPL-3.0"
      VALUE "OriginalFilename", "GeraPDFEditor.exe"
      VALUE "ProductName", "Gera PDF Editor"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x416, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let rc_path = out.join("gera.rc");
    std::fs::write(&rc_path, rc).unwrap();
    let obj = out.join("gera-res.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let ok = Command::new(&windres)
        .arg(&rc_path)
        .args(["-O", "coff", "-o"])
        .arg(&obj)
        .status()
        .expect("windres do MinGW não encontrado");
    assert!(ok.success(), "windres falhou");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}
