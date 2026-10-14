//! No Windows (MinGW), o ícone e as informações de versão (Propriedades do arquivo) vão no .exe
//! como recurso, compilado pelo `windres`. (A libstdc++ estática fica no build.rs do pdf-core.)
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icons/gera-pdf-editor.ico");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("windows-gnu") {
        return;
    }
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
