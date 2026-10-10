# Gera PDF Editor — Subprojeto 5c: Criar PDF e Exportar PDF

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

As ferramentas **Criar PDF** (a partir de imagens) e **Exportar PDF** (Word, texto, imagens) do
Acrobat.

### Critérios de sucesso

- **Criar PDF**: escolher uma ou mais imagens (JPEG, PNG, TIFF, BMP, GIF) → um PDF, uma página por
  imagem, no tamanho da imagem pela resolução dela (sem resolução: 72 ppp, como o Acrobat), limitado
  ao máximo do PDF (200 pol). JPEG entra sem recompressão. O PDF novo é criado na pasta de
  documentos criados do app e aberto numa aba ("Salvar como" para guardar onde quiser). Abrir uma
  imagem pelo Abrir/linha de comando faz o mesmo (como o Acrobat).
- **Exportar PDF**: janela com o formato — **Microsoft Word (.docx)**, **Texto (.txt)**,
  **Imagens PNG** ou **JPEG** (resolução 72/150/300 ppp) — e **Exportar**; o resultado vai ao lado do
  original com nome livre ("<nome>.docx", "<nome> (imagens)/página 1.png"…), e a barra de status
  mostra onde. Respeita a permissão de copiar.
- Word: pelo conversor do MuPDF (biblioteca "extract", AGPL): texto com parágrafos, tabelas simples e
  imagens.

### Fora de escopo

Excel, PowerPoint, RTF, HTML (ficam para depois); criar PDF de documentos do Office.

## 2. Arquitetura

- **pdf-core** `create_export.rs`: `create_pdf_from_images(&[PathBuf], dest)`;
  `ExportFormat { Word, Text, Png { dpi }, Jpeg { dpi } }`; `PdfDoc::export(format, dest_dir/stem)`.
- **worker**: `Request::Export { format }` → `Response::Exported…`.
- **gera-ui**: Criar PDF (seletor de imagens; abrir imagem), janela Exportar PDF.

## 3. Testes

- pdf-core: duas imagens viram duas páginas com o tamanho certo; JPEG mantém o fluxo DCT; imagem
  inválida recusa; exportar Word gera um .docx (zip com word/document.xml contendo o texto); texto
  contém "Hello"; PNG/JPEG uma imagem por página com a resolução pedida; sem permissão de copiar
  recusa.
