# Gera PDF Editor — Subprojeto 5d: Digitalizar e OCR (Reconhecer texto)

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

O **Reconhecer texto** do Acrobat: tornar pesquisável e selecionável o texto de páginas
digitalizadas, sem mudar a aparência delas.

### Critérios de sucesso

- Ferramenta **Digitalizar e OCR** → janela: idioma (Português, Inglês, Português + Inglês), páginas
  (todas / página atual), **Reconhecer texto**, barra de progresso por página e **Cancelar**.
- Cada página sem texto (menos de 3 caracteres) é renderizada a 300 ppp, reconhecida pelo
  Tesseract (embutido no MuPDF) e recebe uma **camada de texto invisível** (modo 3) sobre as palavras,
  com a mesma posição e largura — a página continua igual, mas busca, seleção e cópia funcionam.
  Páginas que já têm texto são puladas (como o Acrobat: "a página já contém texto").
- O conteúdo original fica intacto (só se acrescenta um fluxo de conteúdo); desfazer volta tudo.
  Salvar grava; PDF assinado recusa (alteraria as assinaturas); exige a permissão de alterar.
- Dados de idioma: `TESSDATA_PREFIX`, os do AppImage (`usr/share/tessdata`) ou os do sistema
  (`/usr/share/tesseract/tessdata`, `/usr/share/tessdata`); sem dados, a janela explica.

### Fora de escopo

Digitalizar do scanner (SANE), corrigir inclinação, "ClearScan" (fontes editáveis), outros idiomas.

## 2. Arquitetura

- **pdf-core** `ocr.rs`: `tessdata_dir()`, `OcrLanguage`, `PdfDoc::recognize_text(pages, lang,
  progress) -> OcrReport { recognized, skipped, words }`; instantâneo no histórico.
- **worker**: `Request::RecognizeText { pages, lang, cancel }` → `OcrProgress` e `OcrDone`.
- **gera-ui**: janela Reconhecer texto com progresso e Cancelar.
- **packaging**: copiar por/eng.traineddata para o AppImage.

## 3. Testes

- pdf-core: página só imagem (texto renderizado) → depois do OCR `page_text` contém o texto e a
  busca acha; a imagem renderizada não muda; página com texto é pulada; desfazer remove; cancelar
  pelo progresso para; sem dados de idioma dá erro claro.
