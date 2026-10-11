# Avisos de terceiros

- **MuPDF** (Artifex Software) — AGPL-3.0. Este projeto é distribuído sob a mesma licença; o código-fonte completo precisa acompanhar qualquer distribuição do binário/AppImage.
  Inclui as bibliotecas embutidas no MuPDF: FreeType, HarfBuzz, libjpeg, OpenJPEG, jbig2dec, lcms2,
  zlib, Gumbo, Brotli, "extract" (exportar para Word; AGPL-3.0), **Tesseract OCR** e **Leptonica**
  (Apache-2.0 e BSD-2-Clause).
- **Dados de idioma do Tesseract** (tessdata por/eng, Google) — Apache-2.0; acompanham o AppImage.
- **Great Vibes** (fonte manuscrita da assinatura digitada, The Great Vibes Pro Project Authors) —
  SIL Open Font License 1.1 (`crates/pdf-core/fonts/OFL.txt`).
- **Liberation Sans** (Red Hat) — SIL Open Font License 1.1: um subconjunto dela está só no PDF de
  teste `crates/pdf-core/tests/fixtures/fonte-oca.pdf` (não vai no programa).
- **RustCrypto** (cms, x509-cert, der, rsa, p256, sha1, sha2, md-5, aes, cbc) — MIT ou Apache-2.0.
- **p12-keystore** — MIT ou Apache-2.0. **jpeg-encoder** — MIT ou Apache-2.0 (e IJG). **zip** — MIT.
- **egui / eframe** (Emil Ernerfeldt e contribuidores) — MIT ou Apache-2.0.
- **thiserror** (David Tolnay) — MIT ou Apache-2.0.
- **rfd** (Rusty File Dialog, PolyMeilex e contribuidores) — MIT.
- **similar** (Armin Ronacher, diferenças de texto do Comparar arquivos) — Apache-2.0.
- **arboard** (1Password e contribuidores, área de transferência ao colar imagens) — MIT ou Apache-2.0.
- **png** (image-rs, imagem colada gravada em PNG) — MIT ou Apache-2.0.
- **serde / serde_json** (Erick Tryzelaar, David Tolnay) — MIT ou Apache-2.0.

As demais dependências transitivas e suas licenças são listadas por `cargo tree` e `Cargo.lock`.
