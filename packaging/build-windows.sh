#!/usr/bin/env bash
# Gera, para o Windows, dist/Gera_PDF_Editor-<versão>-windows-x64-portatil.zip (versão portátil)
# e dist/Gera_PDF_Editor-<versão>-windows-x64-instalador.exe (instalador, pelo NSIS).
#
# Compilação cruzada no Linux com MinGW-w64 (alvo x86_64-pc-windows-gnu): o .exe não pede DLL
# fora do Windows (o runtime C/C++ vai ligado). Requisitos: `rustup target add
# x86_64-pc-windows-gnu`, o g++ do MinGW-w64 (Fedora: mingw64-gcc-c++ e
# mingw64-winpthreads-static; Ubuntu: g++-mingw-w64-x86-64-posix), o zip e o NSIS (Fedora:
# mingw-nsis-base e mingw32-nsis; Ubuntu: nsis).
set -euo pipefail
cd "$(dirname "$0")/.."
source packaging/lib.sh

TARGET=x86_64-pc-windows-gnu
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release -p gera-ui --target "$TARGET"

STAGE=target/windows-portatil
APP="$STAGE/Gera PDF Editor"
rm -rf "$STAGE" && mkdir -p "$APP" dist
cp "target/$TARGET/release/gera-pdf-editor.exe" "$APP/GeraPDFEditor.exe"
copy_licenses "$APP/licencas" LICENSE.txt
# dados de idioma do OCR: o app os procura em tessdata\ ao lado do .exe
copy_tessdata "$APP/tessdata"
# instalador: a pasta do app, com atalhos, registro para PDF e desinstalador (antes do LEIA-ME,
# que é só da versão portátil)
SETUP="dist/Gera_PDF_Editor-$VERSION-windows-x64-instalador.exe"
makensis -V2 -DVERSION="$VERSION" -DSOURCE="$PWD/$APP" -DOUTFILE="$PWD/$SETUP" \
  packaging/windows/instalador.nsi
ls -l "$SETUP"
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
