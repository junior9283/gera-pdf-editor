# Marca d'água e Cabeçalho e rodapé (v2c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ferramentas Marca d'água e Cabeçalho e rodapé (com Bates) do Acrobat: Adicionar, Atualizar
e Remover.

**Architecture:** `pdf-core/src/pagination.rs` desenha a marca (XObject de formulário compartilhado) e
o cabeçalho/rodapé (texto por página) dentro de `/Artifact <</Type/Pagination/Subtype/…>> BDC … EMC`,
como o Acrobat; remover apaga esses trechos do conteúdo. O texto usa as rotinas de fonte e glifos do
Editar texto, movidas para `page_content.rs`. A UI tem duas janelas com prévia e a linha de cada
ferramenta (Adicionar…, Atualizar…, Remover).

**Tech Stack:** Rust, mupdf 0.8 (`add_font`, `add_image`, `graft_object`, `register_ext_gstate`,
`insert_contents`), serde/serde_json (escolhas guardadas), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-10-gera-pdf-editor-v2c-marca-cabecalho-design.md`

## Global Constraints

- Comportamento e nomes como no Acrobat Pro; textos em pt-BR.
- Recusar em PDF assinado ou sem permissão de alterar; desfazer por instantâneo.
- O `GeraConfig` lido do arquivo é dado não confiável: só preenche a janela, validado com padrões.
- Metas: abertura < 300 ms, memória < 150 MB, binário < 25 MB.

## Review Focus

1. Página girada (/Rotate) e caixa deslocada: marca e cabeçalho no lugar da página como vista.
2. Conteúdo em vários fluxos e marcado aninhado (BDC dentro de BDC): Remover tira só o trecho certo.
3. Marca d'água do Acrobat (`/Artifact <</Subtype/Watermark/Type/Pagination>> BDC`) é removida.
4. PDF de origem protegido, com várias páginas ou página inexistente; imagem inválida.
5. Documento com centenas de páginas: a marca é um objeto só (não um por página).

---

### Task 1: pdf-core — marca d'água, cabeçalho e rodapé, remover

**Files:** Create `crates/pdf-core/src/pagination.rs`, `crates/pdf-core/tests/pagination.rs`;
Modify `page_content.rs` (Lexer e escrita de texto compartilhados), `edit_text.rs`, `edit_images.rs`,
`lib.rs`, `worker.rs`, `Cargo.toml` (serde, serde_json).

**Interfaces (Produces):**
- `pub enum StdFont { Helvetica, Times, Courier }`, `pub struct TextLook { font, bold, size, color }`
- `pub enum WatermarkSource { Text { text, look }, Image(PathBuf), PdfPage { path, page } }`
- `pub struct Watermark { source, rotation, opacity, scale: Option<f32>, behind, vertical: (Align, f32), horizontal: (Align, f32), pages: PageSet }`
- `pub struct HeaderFooter { texts: [String; 6], look, margins: [f32; 4], start: u32, date_format, bates: Option<Bates>, pages: PageSet }`
- `pub enum PageSet { All, Range(String), Even, Odd }`; `pub enum Pagination { Watermark, HeaderFooter }`
- `PdfDoc::{add_watermark, add_header_footer, remove_pagination(kind) -> usize, pagination_config(kind)}`
- Worker: `Request::{AddWatermark, AddHeaderFooter, RemovePagination, PaginationConfig}`;
  `Response::PaginationConfig`.

- [ ] Testes (RED): marca de texto em todas/só nas escolhidas; atrás/na frente; opacidade (ExtGState);
  imagem e página de PDF; página girada; um XObject só; Atualizar (remove + acrescenta); Remover
  (também trecho do Acrobat, BDC aninhado, vários fluxos); cabeçalho/rodapé nas seis posições com
  `<<n>>`, `<<total>>`, data, Bates e número inicial; desfazer; config lida de volta.
- [ ] Implementar; GREEN; commit.

### Task 2: UI — janelas e linhas das ferramentas

**Files:** Create `crates/gera-ui/src/shell/watermark_dialog.rs`, `header_footer_dialog.rs`;
Modify `doc_tab.rs`, `toolbar.rs`, `app.rs`, `tools_catalog.rs`.

- [ ] Testes (RED) das janelas: escolhas → `Watermark`/`HeaderFooter`; validação do intervalo;
  inserir número/data/Bates; preencher pela config.
- [ ] Implementar (prévia: página atual renderizada com a marca aproximada); GREEN; E2E; commit.
