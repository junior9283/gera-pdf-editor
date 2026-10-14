#!/usr/bin/env bash
# Gera dist/Gera_PDF_Editor-<versão>-windows-x64-portatil.zip (versão portátil do Windows).
#
# Compilação cruzada no Linux com MinGW-w64 (alvo x86_64-pc-windows-gnu): o .exe não pede DLL
# fora do Windows (o runtime C/C++ vai ligado). Requisitos: `rustup target add
# x86_64-pc-windows-gnu`, o g++ do MinGW-w64 (Fedora: mingw64-gcc-c++ e
# mingw64-winpthreads-static; Ubuntu: g++-mingw-w64-x86-64-posix) e o zip.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release -p gera-ui --target "$TARGET"

STAGE=target/windows-portatil
APP="$STAGE/Gera PDF Editor"
rm -rf "$STAGE" && mkdir -p "$APP/tessdata" "$APP/licencas" dist
cp "target/$TARGET/release/gera-pdf-editor.exe" "$APP/GeraPDFEditor.exe"
# AGPL-3.0: a licença e os avisos acompanham o programa
cp LICENSE "$APP/licencas/LICENSE.txt"
cp THIRD_PARTY_NOTICES.md "$APP/licencas/"
cp crates/pdf-core/fonts/OFL.txt "$APP/licencas/OFL-GreatVibes.txt"
# dados de idioma do OCR (português e inglês): o app os procura em tessdata\ ao lado do .exe
for lang in por eng; do
  for dir in /usr/share/tesseract/tessdata /usr/share/tessdata /usr/share/tesseract-ocr/*/tessdata; do
    if [ -f "$dir/$lang.traineddata" ]; then cp "$dir/$lang.traineddata" "$APP/tessdata/"; break; fi
  done
  if [ ! -f "$APP/tessdata/$lang.traineddata" ]; then
    echo "faltam os dados de OCR '$lang' (instale tesseract-langpack-$lang)" >&2
    exit 1
  fi
done
# instruções curtas em texto do Windows (CRLF)
sed 's/$/\r/' > "$APP/LEIA-ME.txt" <<TXT
Gera PDF Editor $VERSION - versão portátil para Windows 10 e 11 (64 bits)

Para usar, abra GeraPDFEditor.exe. Não há instalação.

- As configurações, os IDs digitais e os carimbos ficam na pasta "dados", ao lado do programa,
  e vão junto se você copiar a pasta para outro computador ou pendrive.
- Se a pasta do programa não permitir gravação (por exemplo, dentro de Arquivos de Programas),
  as configurações vão para %APPDATA%\Gera PDF Editor.
- Para abrir um PDF, arraste-o até a janela ou use "Abrir com" > GeraPDFEditor.exe.

Código-fonte e novidades: https://github.com/junior9283/gera-pdf-editor
Licença: AGPL-3.0 (veja a pasta "licencas").
TXT
ZIP="dist/Gera_PDF_Editor-$VERSION-windows-x64-portatil.zip"
rm -f "$ZIP"
(cd "$STAGE" && zip -qr -9 "$OLDPWD/$ZIP" "Gera PDF Editor")
ls -l "$ZIP"
