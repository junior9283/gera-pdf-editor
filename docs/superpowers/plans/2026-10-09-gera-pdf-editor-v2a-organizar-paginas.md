# v2a — Organizar páginas: plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modo Organizar páginas (grade de miniaturas) com girar, excluir, mover, inserir, extrair, substituir e recortar, com desfazer.

**Architecture:** pdf-core aplica `PageOp` no documento em memória com desfazer por instantâneo (bytes do documento + conjunto `hidden`), misturado ao histórico de anotações; worker responde `PagesChanged` com um `DocSummary` novo. app-state guarda a lógica pura da grade (seleção, arrasto, página atual). gera-ui desenha a grade, a barra e os diálogos.

**Tech Stack:** Rust 2021, mupdf 0.8 (`insert_pdf`, `move_page`, `delete_pages`, `new_page_at`, `set_rotation`, `set_crop_box`, `write_to_with_options`, `from_bytes`), egui 0.36, rfd.

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v2a-organizar-paginas-design.md`

## Global Constraints

- pt-BR; commits com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; simplify só no fim; E2E pelo executor em Xvfb; nunca `pkill -f`.
- fmt, clippy `-D warnings`, testes verdes ao fim de cada tarefa; asserts de bytes/pixels com `assert!(a == b)` (sem imprimir vetores).
- O documento nunca fica sem páginas; nada muda o arquivo sem Salvar; criptografia mantida no Salvar.
- Instantâneos limitados a 512 MB.

## Review Focus

1. **Excluir página com anotação apagada pendente** (hidden) e desfazer: a anotação volta apagada (não ressuscita) — `hidden_annotations_survive_page_undo` (tarefa 1).
2. **Mover várias páginas não contíguas** mantém a ordem relativa — `moving_scattered_pages_keeps_their_order` (tarefas 1 e 2).
3. **Inserir de PDF protegido** com senha errada não muda nada — `insert_from_protected_pdf_needs_the_password` (tarefa 1).
4. **Desfazer misto** (anotação, página, anotação) volta na ordem certa — `undo_mixes_page_and_annotation_steps` (tarefa 1).
5. **Excluir todas as páginas** é recusado — `cannot_delete_every_page` (tarefas 1 e 2).

---

### Tarefa 1: `pdf-core` — operações de página, instantâneos e extração

**Files:** Modify `src/types.rs` (`PageOp`, `CropMargins`), `src/annot_edit.rs` (`History` com passos de instantâneo), `src/doc.rs`, `src/worker.rs`; Create `src/pages.rs`; Test `tests/pages.rs`.

**Produces:**
- `pub enum PageOp { Rotate { pages: Vec<i32>, quarter_turns: i32 }, Delete { pages: Vec<i32> }, Move { pages: Vec<i32>, to: i32 }, InsertBlank { at: i32, size: (f32, f32) }, InsertFrom { at: i32, path: PathBuf, password: Option<String>, range: Option<(i32, i32)> }, Replace { pages: Vec<i32>, path: PathBuf, password: Option<String>, from: i32 }, Crop { pages: Vec<i32>, margins: [f32; 4] /* cima, baixo, esq, dir em pt */ } }`
- `PdfDoc::apply_pages(&mut self, op: PageOp) -> Result<()>`; `PdfDoc::extract(&self, pages: &[i32], dest: &Path, separate: bool) -> Result<Vec<PathBuf>>`.
- `History` passa a guardar `Step::Annot(Edit)` ou `Step::Snapshot(Snapshot)` (bytes + hidden); `undo/redo` devolvem `Undone::Pages(Vec<i32>)` ou `Undone::Document` (instantâneo).
- Worker: `Request::Pages(PageOp)`, `Request::Extract { pages, dest, separate }`; `Response::PagesChanged { summary: DocSummary, history: HistoryState }`, `Response::Extracted(Vec<PathBuf>)`, `Response::PagesFailed(String)`.

- [ ] **Passo 1: testes** (`tests/pages.rs`), usando `sample_pdf(n, 0)` (cada página tem "Page N" no texto, conferido por `page_text`):
  - `rotate_delete_move_insert_and_undo_each` (cada op + undo + redo pela ordem dos textos/rotação/contagem)
  - `moving_scattered_pages_keeps_their_order` ([0,2] para o fim → ordem 1,3,0,2…)
  - `cannot_delete_every_page`
  - `insert_from_file_brings_annotations_and_range`
  - `insert_from_protected_pdf_needs_the_password`
  - `replace_swaps_pages_from_another_file`
  - `crop_sets_the_crop_box_and_undo_restores`
  - `hidden_annotations_survive_page_undo`
  - `undo_mixes_page_and_annotation_steps`
  - `extract_writes_one_or_many_files_and_keeps_the_document`
  - `snapshots_respect_the_memory_limit` (limite reduzido por `History::set_snapshot_budget` só em teste via função `pub(crate)` acessada por `#[cfg(test)]` em teste de unidade de `annot_edit`)
- [ ] **Passo 2:** rodar → falha de compilação.
- [ ] **Passo 3:** implementar (`pages.rs` com as ops; `snapshot()` = `write_to_with_options(Keep)` + `hidden.clone()`; antes da op: `purge_hidden`, `hidden.clear()`; `restore(snapshot)` = `from_bytes` + `authenticate(password)`).
- [ ] **Passo 4:** `cargo test -p pdf-core` verde.
- [ ] **Passo 5:** commit `feat(pdf-core): organizar páginas com desfazer por instantâneo`.

### Tarefa 2: `app-state` — lógica da grade

**Files:** Create `crates/app-state/src/organize.rs`.

**Produces:** `PageSelection` (conjunto ordenado + âncora) com `click(i)`, `ctrl_click(i)`, `shift_click(i)`, `all(n)`, `pick(n, Picker, sizes)` (`Picker::{All, Even, Odd, Portrait, Landscape}`), `to_vec()`; `grid_columns(width, thumb_w, gap)`; `drop_target(pointer, cell, cols, n) -> i32`; `current_after(op_kind, current, …)`; `can_delete(selected, total)`.

- [ ] Testes: seleção (clique/ctrl/shift/intervalos, pares = páginas 2,4… na numeração humana), destino do arrasto (antes/depois pela metade da célula), `cannot_delete_every_page`, página atual depois de excluir/mover/inserir.
- [ ] Implementar, verde, commit `feat(app-state): seleção e arrasto da grade de páginas`.

### Tarefa 3: `gera-ui` — modo Organizar

**Files:** Create `crates/gera-ui/src/organize.rs`; Modify `doc_tab.rs` (modo e `PagesChanged`), `app.rs` (`run_tool("organizar"|"inserir"|"recortar")`, menu Exibir), `app-state/src/tools_catalog.rs` (ferramentas da v2a ficam disponíveis), `open_dialog.rs` (escolher PDF para inserir/substituir; salvar extração).

- [ ] Testes puros: rótulo da miniatura, texto dos botões habilitados conforme seleção.
- [ ] Implementar a grade (miniaturas do cache de render, seleção, arrasto com marcador, hover girar/excluir, menu de contexto), a barra e os diálogos; `PagesChanged` reconstrói a vista mantendo zoom e página.
- [ ] Smoke em Xvfb; commit `feat(gera-ui): modo Organizar páginas`.

### Tarefa 4: Refatoração, E2E, medição e revisão

- [ ] simplify; E2E da spec; measure (Xvfb); revisão independente (modelo mais capaz); correções; merge; AppImage; página de andamento.
