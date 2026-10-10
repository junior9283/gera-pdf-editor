# v5f Comparar arquivos — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (execução nativa, escolhida pelo
> dono nas etapas anteriores). Passos com checkbox (`- [ ]`).

**Goal:** comparar dois PDFs e gerar o relatório lado a lado do Acrobat, com navegação pelas alterações.

**Architecture:**
- `pdf-core/src/compare.rs` extrai as palavras (com estilo) e as imagens das páginas escolhidas.
  - Faz o diff por palavra com `similar` (Patience com prazo) e alinha as páginas por programação
    dinâmica.
  - Grava um PDF de resultado: o resumo e as páginas lado a lado, com XObjects de formulário e
    anotações de destaque travadas.
- A interface ganha a janela "Comparar arquivos" (no nível do app, como Combinar) e a linha
  "Comparação" na aba do resultado.

**Tech Stack:** Rust, mupdf 0.8, similar 3.2 (Apache-2.0), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5f-comparar-design.md`

## Global Constraints

- **Cores:**
  - Substituído `#4F7BF0`;
  - Inserido `#2EAD4B`;
  - Excluído `#E5484D`;
  - Formatação `#F2994A`;
  - Imagem `#9B59D0`.

  Opacidade 0,35.
- **Anotações do resultado:** Highlight, `/F` = Print|Locked (4|128), autor `"Comparação"`, assunto
  com o tipo, conteúdo cortado em 300 caracteres com "…".
- `/Info /GeraCompare true` no resultado.
- O diff tem prazo de 10 s.
- **Limite:** 20 000 páginas por lado.
- **Recusas:**
  - arquivo com cópia proibida;
  - destino igual a uma das entradas ou a um arquivo aberto numa aba.
- **Atalhos:** Ctrl+, (anterior) e Ctrl+. (próxima).
- Textos em pt-BR.
- Commits terminam com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Páginas giradas (/Rotate 90)** nos originais: o XObject e as marcas têm de cair sobre o texto
   certo. Coberto pelo teste `rotated_pages_keep_marks_on_their_words`.
2. **Texto repetido** (cabeçalho igual em toda página) e páginas inteiras inseridas: o alinhamento não
   pode embaralhar. Coberto por `inserted_page_stands_alone`.
3. **Documento sem texto** (escaneado) dos dois lados: 0 alterações de texto, sem erro.
   Coberto por `image_only_documents_compare_images`.
4. **Intervalo de páginas inválido ou vazio**: recusa clara na janela, sem iniciar.
   Coberto por um teste da janela.
5. **Cancelar no meio**: nenhum arquivo parcial no destino. Coberto por `cancel_leaves_no_file`.

---

### Task 1: núcleo do diff (palavras, alinhamento, alterações)

**Files:** criar `crates/pdf-core/src/compare.rs` e `crates/pdf-core/tests/compare.rs`; alterar
`lib.rs` (pub use).

**Produces:**
- `pub struct CompareInput { pub path: PathBuf, pub password: Option<String>, pub pages: Option<Vec<i32>> }`
- `pub struct CompareOptions { pub old: CompareInput, pub new: CompareInput, pub text_only: bool, pub formatting: bool }`
- `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ChangeKind { Replaced, Inserted, Deleted, Formatting, Image }`
  - `label()`: "Substituído", "Inserido", "Excluído", "Formatação", "Imagem";
  - `color() -> [f32; 3]`.
- `pub struct Change { pub kind, pub pair: usize, pub old_rects: Vec<PdfRect>, pub new_rects: Vec<PdfRect>, pub description: String }`
- `pub struct Comparison { pub pairs: Vec<(Option<i32>, Option<i32>)>, pub changes: Vec<Change>, pub old_pages: i32, pub new_pages: i32 }`
- `pub enum CompareStage { ReadingOld(i32, i32), ReadingNew(i32, i32), Diffing, Writing(usize, usize) }`
- `pub fn compare(opts: &CompareOptions, progress: &mut dyn FnMut(CompareStage) -> bool) -> Result<Comparison>`.
  Devolver `false` cancela, com `PdfError::Cancelled` (ou o erro de cancelamento que já existir).
- `pub(crate) fn align_pages(weights: &[Vec<u32>]) -> Vec<(Option<usize>, Option<usize>)>`: pura, por
  programação dinâmica, com score de par = 2w + 1.

**Passos:**
- [ ] Testes RED. Os PDFs são gerados no teste com `sample`, ou à mão com a fonte Helvetica.
  - `identical_documents_have_no_changes`
  - `replaced_inserted_deleted_words`: "o gato preto" → "o cão preto dormiu" dá Replaced
    (gato→cão) e Inserted (dormiu), com o texto na descrição.
  - `deleted_word_is_on_the_old_side`
  - `inserted_page_stands_alone`
  - testes unitários de `align_pages`: identidade, inserção, tudo diferente emparelha em sequência.
  - `restricted_file_is_refused`
- [ ] Implementar a extração:
  - `page.words` não traz o estilo; percorrer `to_text_page(TextPageFlags::empty())` e montar as
    palavras pelos caracteres (quebra em espaço), cada uma com o texto, o rect (união dos quads) e o
    estilo dominante (fonte sem prefixo de subconjunto, tamanho arredondado a 0,5, argb);
  - depois, o diff `capture_diff_slices_deadline(Algorithm::Patience, …)` sobre o texto;
  - os pesos por par de páginas a partir dos `Equal`;
  - `align_pages`;
  - por fim, a conversão das operações em alterações por par.
- [ ] GREEN; commit `feat(pdf-core): núcleo da comparação de arquivos`.

### Task 2: imagens e formatação

- [ ] Testes RED:
  - `changed_image_is_reported`;
  - `text_only_skips_images`;
  - `formatting_only_when_enabled`;
  - `image_only_documents_compare_images`.
- [ ] Implementar:
  - blocos de imagem com `TextPageFlags::PRESERVE_IMAGES`, identificados pelo MD5 dos pixels de
    `block.image()`;
  - o casamento por par de páginas;
  - a formatação nos `Equal`, juntando palavras seguidas com a mesma mudança.
- [ ] GREEN; commit.

### Task 3: PDF de resultado

**Produces:** `pub fn write_comparison(opts: &CompareOptions, cmp: &Comparison, dest: &Path, progress: …) -> Result<()>`
e `pub fn compare_files(opts, dest, progress) -> Result<Comparison>`, que faz `compare` + `write_comparison`.

- [ ] Testes RED:
  - `report_has_summary_pairs_and_locked_annotations`: páginas = 1 + pares; anotações Highlight com
    autor "Comparação" e flag Locked; `/GeraCompare`; marcadores;
  - `rotated_pages_keep_marks_on_their_words`: a palavra mudada, extraída do resultado dentro do
    retângulo da anotação, é a palavra nova;
  - `cancel_leaves_no_file`;
  - `destination_cannot_be_an_input`.
- [ ] Implementar:
  - o XObject da página (extrair de `pagination.rs` um helper `page_form(dest_doc, src_doc, page)`
    comum, que devolve o form e a caixa, e usá-lo nos dois lugares);
  - a página lado a lado, com a matriz `flip ∘ translate ∘ page_ctm`;
  - as faixas de título;
  - as marcas do outro lado no conteúdo;
  - as anotações;
  - o resumo, com Helvetica WinAnsi (`stamp::literal`), a legenda e o link;
  - os marcadores;
  - `write_new`.
- [ ] GREEN; commit.

### Task 4: catálogo e navegação (app-state)

- [ ] `tools_catalog`: `pub const COMPARE: &str = "comparar"` como Ready, e ajustar o teste do
  catálogo.
- [ ] `app_state::compare_nav::step(changes: &[(i32, f32, f32)], current: Option<usize>, forward: bool) -> Option<usize>`,
  com testes: começa na primeira ou na última, para nas pontas sem dar a volta (como o Acrobat) e
  lista vazia dá None.
- [ ] Commit.

### Task 5: janela Comparar arquivos (gera-ui)

- [ ] `crates/gera-ui/src/shell/compare.rs`: `CompareWindow` no modelo do `CombineWindow`.
  - Dois `Entry`, reaproveitando `combine::Entry`, cada um com o texto do intervalo.
  - Trocar, as caixas de opção, Comparar (diálogo Salvar), a thread com progresso e cancelamento e o
    status.
  - Testes de estado:
    - `swap_swaps`;
    - `bad_range_blocks_compare`;
    - `dest_open_in_tab_is_refused`.
- [ ] `app.rs`:
  - `run_tool(COMPARE)` abre a janela, preenchendo o antigo com a aba ativa;
  - o resultado abre a aba e chama `doc.show_row(COMPARE)`.
- [ ] Commit.

### Task 6: linha Comparação na aba

- [ ] `DocTab`:
  - campo `compare_tools`;
  - `show_row(COMPARE)` liga esse campo;
  - ao abrir um documento com `/GeraCompare`, a linha abre sozinha (`DocSummary.is_comparison`, vindo
    do pdf-core).
- [ ] `toolbar::compare_row`: "Alteração i de N", ◀ ▶ e Nova comparação.
  - Ctrl+, e Ctrl+. navegam quando a linha está aberta.
  - A navegação seleciona e rola, como `PanelAction::Select`.
- [ ] Commit.

### Task 7: E2E, documentação

- [ ] E2E em Xvfb (roteiro da spec).
- [ ] Commit da spec, do plano e do ledger.
