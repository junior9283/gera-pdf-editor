# Editar PDF: texto (v4b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** No modo Editar PDF, os blocos de texto da página aparecem com contorno; clicar num bloco abre a
edição no lugar e confirmar troca o texto (mesma posição, fonte, tamanho e cor); **Adicionar texto** cria
um bloco novo.

**Architecture:** `pdf-core/src/edit_text.rs` lista os blocos pelo texto estruturado do MuPDF (fonte,
tamanho, cor e origem de cada caractere). Editar = redação só de texto (sem tarja, sem tocar em imagens
nem traços) no retângulo do bloco + texto novo escrito por cima (`q … BT … ET Q` acrescentado ao conteúdo,
no espaço da tela, com quebra automática na largura do bloco). A fonte original é reaproveitada se tiver
todos os glifos do texto novo; senão a padrão equivalente (Helvetica/Times/Courier, negrito/itálico).
Depois da redação, se ainda houver texto antigo no bloco, a edição é recusada e o instantâneo devolve o
documento como estava. Na UI, `gera-ui/src/edit_pdf.rs` ganha os blocos, o editor no lugar e o Adicionar
texto.

**Tech Stack:** Rust, mupdf 0.8 (`PdfPage::add_redact_annotation`, `apply_redactions_with_options`,
`PdfDocument::add_font`, `TextPage`), egui/eframe 0.36.

**Spec:** `docs/superpowers/specs/2026-10-10-gera-pdf-editor-v4-editar-design.md` (seção 3)

## Global Constraints

- Qualidade e comportamento no mínimo iguais aos do Acrobat Pro; atalhos iguais (Esc e clique fora
  confirmam a edição; Ctrl+Z desfaz).
- Recusar em PDF assinado e sem permissão de alterar (`require_modifiable`), como na v4a.
- Desfazer por instantâneo (`with_snapshot` via `guarded_page_edit`).
- Textos da interface e comentários em pt-BR.
- Metas: abertura < 300 ms, memória < 150 MB, binário < 25 MB.

## Review Focus

1. Blocos colados (linhas a 1–2 pt um do outro): a redação de um não pode apagar letras do vizinho.
2. Página com /Rotate e caixa deslocada: o texto novo sai em pé e no lugar, como era visto na tela.
3. Fonte subconjunto sem o glifo de espaço ou sem as letras novas (cai para a padrão sem quadradinhos).
4. Texto dentro de formulário (XObject) ou fonte Type3: a redação não remove → recusa sem estragar.
5. Acentos e caracteres fora do Latin-1 (ç, ã, €, “aspas”): saem certos e copiáveis (ToUnicode).

---

### Task 1: pdf-core — listar e editar blocos de texto

**Files:**
- Create: `crates/pdf-core/src/edit_text.rs`
- Modify: `crates/pdf-core/src/lib.rs`, `crates/pdf-core/src/edit_images.rs` (recursos próprios
  genéricos: `own_resource(doc, page_obj, "XObject" | "Font")`), `crates/pdf-core/src/worker.rs`
- Test: `crates/pdf-core/tests/edit_text.rs`

**Interfaces:**
- Produces:
  - `pub struct PageTextBlock { pub rect: PdfRect /*tela*/, pub text: String /*linhas com \n*/, pub size: f32, pub color: [u8; 3], pub font: String, pub editable: bool }`
  - `PdfDoc::page_text_blocks(page) -> Result<Vec<PageTextBlock>>` (só blocos horizontais são
    editáveis)
  - `PdfDoc::edit_text_block(page, rect: PdfRect, text: &str) -> Result<()>` (texto vazio = apagar o
    bloco; `rect` identifica o bloco: o de retângulo mais próximo, tolerância 2 pt, senão recusa)
  - `PdfDoc::add_text(page, at: (f32, f32) /*canto superior esquerdo, tela*/, text: &str) -> Result<()>`
    (Helvetica 12, preto, largura até a margem direita menos 36 pt)
  - Worker: `Request::PageTextBlocks{page}` → `Response::PageTextBlocks{page, blocks}`;
    `Request::EditTextBlock{page, rect, text}`, `Request::AddText{page, at, text}` →
    `Changed::Pages(vec![page])`; falhas em `EditFailed`.

- [ ] **Step 1: testes (RED)** em `tests/edit_text.rs`, com página gerada (Helvetica 18 “Texto acima”,
  linha vermelha “Segunda linha”, imagem, “Texto abaixo”):
  - `blocks_are_listed_with_style`: texto, tamanho ≈ 18, cor vermelha na linha vermelha, editável.
  - `editing_replaces_only_that_block`: troca “Texto acima” por “Novo título”; `page_text` tem o novo,
    não tem o velho, mantém “Texto abaixo”; a imagem continua (`page_images().len() == 1`).
  - `accents_and_symbols_round_trip`: “ação é útil — 10 €” sai igual em `page_text`.
  - `long_text_wraps_inside_the_block_width`: texto longo num bloco estreito → várias linhas, todos os
    caracteres com x dentro do bloco (tolerância 1 pt).
  - `style_is_kept`: a cor vermelha e o tamanho 18 continuam no bloco editado.
  - `close_neighbours_survive`: duas linhas a 2 pt uma da outra em blocos separados; editar uma não
    apaga letra da outra.
  - `rotated_page_edits_in_screen_space`: /Rotate 90; o bloco editado fica no mesmo retângulo de tela
    (canto superior esquerdo ±2 pt) e horizontal na tela.
  - `undo_brings_the_old_text_back`, `empty_text_deletes_the_block`,
    `adding_text_places_it_at_the_click`.
  - `text_that_cannot_be_removed_is_refused`: bloco num XObject de formulário → se a redação não o
    tirar, `Err` e o documento intacto (texto antigo presente, nada duplicado).
- [ ] **Step 2:** rodar `cargo test -p pdf-core --test edit_text` → falha (módulo não existe).
- [ ] **Step 3: implementar** `edit_text.rs`: extração (`to_text_page(TextPageFlags::empty())`, blocos
  `Text`; estilo do caractere mais frequente; editável se todas as linhas têm direção ≈ (1,0)); escrita
  (`Writer`: fonte → `add_font`, nome livre `GeraFn` em /Resources/Font próprio; quebra gulosa por
  palavras com `advance_glyph`; espaço sem glifo vira ajuste no `TJ`; `q inv(page_ctm) cm BT r g b rg
  /GeraFnN size Tf 1 0 0 -1 x y Tm [<gids>] TJ … ET Q` com entrelinha = a do bloco original ou 1,2 ×
  tamanho); redação (`PdfRedactOptions{black_boxes:false, image_method: None, line_art: None,
  text: Remove}`), conferência pós-redação e recusa.
- [ ] **Step 4:** testes passam (GREEN); `cargo test -p pdf-core`.
- [ ] **Step 5:** commit `feat(pdf-core): editar e acrescentar texto da página`.

### Task 2: UI — contornos de texto, edição no lugar e Adicionar texto

**Files:**
- Modify: `crates/gera-ui/src/edit_pdf.rs`, `crates/gera-ui/src/doc_tab.rs`,
  `crates/gera-ui/src/shell/toolbar.rs`, `crates/app-state/src/tools_catalog.rs` (descrição)

**Interfaces:**
- Consumes: Task 1 (`PageTextBlock`, requests).
- Produces: `EditPdfState::{on_text_blocks, editing}`; `ImageAction::{EditText{page, rect, text},
  AddText{page, at, text}}` (o enum vira `EditAction`).

- [ ] **Step 1: testes (RED)** de unidade em `edit_pdf.rs`:
  - clicar num bloco editável abre a edição com o texto dele; clicar fora confirma (ação `EditText`
    só se o texto mudou);
  - Esc confirma e fecha; texto inalterado não gera ação;
  - Adicionar texto: o clique abre a edição vazia no ponto; confirmar vazio não gera ação;
  - bloco não editável (vertical) dá aviso.
- [ ] **Step 2:** `cargo test -p gera-ui edit_pdf` → falha.
- [ ] **Step 3: implementar**: contornos dos blocos (cinza claro, azul ao passar o mouse, como o
  Acrobat); editor `TextEdit::multiline` sobre o bloco (fundo branco, tamanho do bloco × zoom, cor do
  bloco) com foco; clique fora ou Esc confirmam; Delete/setas não agem na imagem durante a edição;
  botão **Adicionar texto** (alternável) na linha da ferramenta; ao abrir outra ferramenta ou trocar de
  documento, a edição em aberto é confirmada.
- [ ] **Step 4:** testes passam; `cargo clippy -p gera-ui`.
- [ ] **Step 5:** commit `feat: editar texto no modo Editar PDF`.
