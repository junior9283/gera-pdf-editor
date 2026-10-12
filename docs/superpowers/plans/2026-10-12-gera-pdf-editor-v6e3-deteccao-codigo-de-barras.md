# v6e3: detecção automática de campos e campo Código de barras. Plano de implementação

> **Para agentes:** sub-skill obrigatória: superpowers:executing-plans (execução nativa).

**Spec:** `docs/superpowers/specs/2026-10-12-gera-pdf-editor-v6e3-deteccao-codigo-de-barras-design.md`

## Restrições globais

- A detecção só acrescenta, numa etapa do desfazer.
- Tetos:
  - 400 campos por página;
  - 20 000 traços por página;
  - 2 000 caracteres no código de barras.
- Nenhum script de outro programa executa.
- Testes:
  - `cargo test -q -p app-state -p gera-ui`;
  - `cargo test -q -p pdf-core -- --test-threads=4`;
  - clippy;
  - E2E com `--features gera-ui/e2e`.

## Foco da revisão

1. Página hostil com milhões de traços ou texto gigante: a detecção termina rápido e sem pânico.
2. Detecção num PDF que já tem campos: nada se sobrepõe e nada muda.
3. Valor enorme ou com caracteres fora do Latin-1 no código de barras: aviso, sem pânico.
4. O código de barras gravado é lido de volta (as três simbologias).

---

### Tarefa 1: candidatos (pdf-core/src/detect.rs, puro)

- [ ] Testes RED:
  - `line_with_label_becomes_named_text_field`;
  - `underscores_become_a_field`;
  - `small_square_becomes_checkbox`;
  - `box_with_text_is_not_a_field`;
  - `signature_and_date_labels`;
  - `no_overlap_with_existing`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: coleta e `PrepareOp::Detect`

- [ ] Testes RED (`tests/detect.rs`):
  - `drawn_form_is_detected_in_one_undo_step`;
  - `huge_page_is_skipped_fast`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: interface da detecção

- Detectar ao entrar no Preparar sem campos.
- Item "Detectar campos automaticamente" no menu Organizar.
- [ ] Teste RED: `entering_prepare_without_fields_asks_for_detection`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 4: código de barras no núcleo (barcode.rs)

- [ ] Testes RED:
  - `barcode_field_is_written_like_acrobat`;
  - `filling_updates_the_barcode`;
  - `barcode_reads_back` (PDF417, QR, Data Matrix);
  - `foreign_barcode_draws_its_value`;
  - `too_much_data_warns`.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: interface do código de barras

- Ferramenta Código de barras e abas Valor e Opções.
- [ ] Teste RED: `barcode_tabs_build_the_props`.
- [ ] Implementar, GREEN, commit; E2E.

### Fim da etapa

- [ ] `simplify`, E2E, `measure.sh`, revisão Opus e uma rodada de correções.
- [ ] Merge, AppImage, docs 0.21.0, tag, push, release e página de andamento.
