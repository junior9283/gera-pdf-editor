#!/usr/bin/env bash
# Gera dist/Gera_PDF_Editor-x86_64.AppImage.
#
# O AppImage leva só o binário: as bibliotecas comuns de desktop Linux vêm do sistema
# (fontconfig, freetype, harfbuzz, glib, libxml2, zlib; OpenGL e X11/Wayland são carregadas
# em tempo de execução). Veja `ldd target/release/gera-pdf-editor`.
set -euo pipefail
cd "$(dirname "$0")/.."

# appimagetool fixado numa versão estável e conferido por hash (SHA-256 publicado pelo GitHub).
TOOL_VERSION=1.9.1
TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
TOOL=target/appimagetool-$TOOL_VERSION-x86_64.AppImage

cargo build --release -p gera-ui
APPDIR=target/AppDir
DOCDIR=$APPDIR/usr/share/doc/gera-pdf-editor
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin" "$DOCDIR" dist
cp target/release/gera-pdf-editor "$APPDIR/usr/bin/"
cp packaging/gera-pdf-editor.desktop packaging/gera-pdf-editor.svg "$APPDIR/"
cp LICENSE THIRD_PARTY_NOTICES.md "$DOCDIR/" # AGPL-3.0: a licença e os avisos acompanham o binário
cp crates/pdf-core/fonts/OFL.txt "$DOCDIR/OFL-GreatVibes.txt"
# dados de idioma do OCR (português e inglês): o app os procura em $APPDIR/usr/share/tessdata
mkdir -p "$APPDIR/usr/share/tessdata"
for lang in por eng; do
  for dir in /usr/share/tesseract/tessdata /usr/share/tessdata /usr/share/tesseract-ocr/5/tessdata; do
    if [ -f "$dir/$lang.traineddata" ]; then cp "$dir/$lang.traineddata" "$APPDIR/usr/share/tessdata/"; break; fi
  done
done
ln -s usr/bin/gera-pdf-editor "$APPDIR/AppRun"

if [ ! -x "$TOOL" ]; then
  curl -sSfL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-x86_64.AppImage"
  echo "$TOOL_SHA256  $TOOL" | sha256sum -c - || { rm -f "$TOOL"; echo "hash do appimagetool não confere" >&2; exit 1; }
  chmod +x "$TOOL"
fi
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "$APPDIR" dist/Gera_PDF_Editor-x86_64.AppImage
ls -l dist/Gera_PDF_Editor-x86_64.AppImage
