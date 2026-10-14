# Funções comuns dos scripts de pacote (AppImage e Windows). Use com `source`.

# Dados de idioma do OCR (português e inglês) das pastas do sistema para `$1`.
copy_tessdata() {
  mkdir -p "$1"
  for lang in por eng; do
    for dir in /usr/share/tesseract/tessdata /usr/share/tessdata /usr/share/tesseract-ocr/*/tessdata; do
      if [ -f "$dir/$lang.traineddata" ]; then cp "$dir/$lang.traineddata" "$1/"; break; fi
    done
    if [ ! -f "$1/$lang.traineddata" ]; then
      echo "faltam os dados de OCR '$lang' (instale tesseract-langpack-$lang)" >&2
      exit 1
    fi
  done
}

# AGPL-3.0: a licença e os avisos acompanham o programa. `$1` = pasta; `$2` = nome da licença.
copy_licenses() {
  mkdir -p "$1"
  cp LICENSE "$1/${2:-LICENSE}"
  cp THIRD_PARTY_NOTICES.md "$1/"
  cp crates/pdf-core/fonts/OFL.txt "$1/OFL-GreatVibes.txt"
}
