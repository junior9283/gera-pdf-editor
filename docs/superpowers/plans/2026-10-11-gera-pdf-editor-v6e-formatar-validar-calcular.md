# v6e: Formatar, Validar e Calcular; Botão e Data; ordem de tabulação. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:** fazer os formulários formatarem, validarem e calcularem ao preencher, como no Acrobat,
com os campos Botão e Data, a ordem de tabulação e Destacar campos.

**Arquitetura:**
- `pdf-core/src/form_scripts.rs` interpreta e grava as chamadas AF do Acrobat, como funções puras.
- `forms.rs` e `prepare.rs` aplicam esse motor ao preencher e ao preparar.
- `gera-ui` ganha as abas das propriedades, o seletor de data, o filtro de teclas, a ordem e o
  destaque.

**Tecnologia:** Rust, mupdf 0.8, egui 0.36, chrono (já é dependência).

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6e-formatar-validar-calcular-design.md`

## Restrições globais

- Nada de interpretador JavaScript: só as chamadas AF e o bloco BVCALC são reconhecidos.
- Tetos:
  - script de 64 KB;
  - 10 casas decimais;
  - 64 passos de cálculo;
  - expressão de cálculo com até 256 símbolos.
- Valor cru em /V; a aparência é formatada.
- Cada preenchimento, com os recálculos que provoca, é uma etapa do desfazer.
- Textos para o usuário em pt-BR, com os avisos no tom do Acrobat.
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.
- Testes de interface: `cargo build --release --features gera-ui/e2e`.

## Foco da revisão

1. **Script AF malformado ou hostil** (números enormes, aspas sem fechar, chamada aninhada,
   expressão gigante): é tratado como script arbitrário, sem pânico.
2. **Ciclo de cálculo** (A = B + 1, B = A + 1): termina e não trava.
3. **Valor colado com letras num campo Número:** recusado sem mudar o campo.
4. **PDF feito no Acrobat com /CO e AFNumber_Format:** lê, mostra formatado e recalcula.
5. **Desfazer depois de um preenchimento que recalculou três totais:** tudo volta numa etapa.

---

### Tarefa 1: motor AF (form_scripts.rs)
**Produz:**
- `FieldFormat::{None, Number { dec, sep, neg, currency, prepend }, Percent { dec, sep },
  Date(String), Time(u8), Special(Special), Custom}`;
- `FieldValidate::{None, Range { min, max }, Custom}`;
- `FieldCalc::{None, Simple { op, fields }, Sfn(String), Custom}`;
- `parse_*` e `to_js_*`;
- `format_value(&FieldFormat, raw) -> Result<String, String>`;
- `keystroke_ok(&FieldFormat, text) -> bool`;
- `validate(&FieldValidate, raw) -> Result<(), String>`;
- `evaluate(&FieldCalc, lookup) -> Option<f64>`.

- [ ] Testes RED (unidade):
  - `number_styles_and_negatives`;
  - `percent_and_currency`;
  - `dates_and_times`;
  - `special_masks`;
  - `keystrokes`;
  - `acrobat_scripts_round_trip`;
  - `sfn_expressions`;
  - `range`;
  - `hostile_scripts_are_custom`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: preencher com formato, validação e cálculo (forms.rs)
**Produz:**
- `FieldInfo.display`, `.format`, `.validate` e `.calc`;
- `set_field` valida, grava cru com aparência formatada e recalcula na ordem /CO, numa etapa.

- [ ] Testes RED (`tests/forms_calc.rs`):
  - `formatted_appearance_keeps_raw_value`;
  - `range_refusal_changes_nothing`;
  - `total_recalculates_in_one_undo_step`;
  - `calc_cycle_stops`;
  - `acrobat_made_form_is_read`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: preparar (Botão, Data, propriedades e ordem de cálculo)
**Produz:**
- `NewFieldKind::{Button, Date}`;
- `FieldProps.format`, `.validate`, `.calc` e `.button_label`;
- `PrepareOp::SetCalcOrder`;
- a ação Redefinir do botão.

- [ ] Testes RED (`tests/prepare.rs`):
  - `button_and_date_fields_are_created`;
  - `format_validate_calc_props_round_trip`;
  - `calc_order_is_saved`;
  - `reset_button_clears_the_form`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: ordem de tabulação
**Produz:**
- `TabOrder::{Row, Column, Structure, Manual(Vec<i32>)}`;
- `PrepareOp::SetTabOrder { page, order }`;
- `PdfDoc::tab_order(page) -> Vec<i32>`.
- A navegação com Tab em `fields.rs` passa a seguir essa ordem.

- [ ] Testes RED:
  - `row_column_and_manual_orders`;
  - em `fields.rs`, `tab_follows_the_page_order`.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: interface do Preparar
**Produz:**
- as abas Formatar, Validar e Calcular nas propriedades;
- as ferramentas Botão e Data;
- `Shift+N` com os números;
- o menu Ordem de tabulação, com mover para cima e para baixo;
- a ordem de cálculo.

- [ ] Teste RED: `props_tabs_build_the_right_ops`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 6: interface do preencher
**Produz:**
- o filtro de teclas;
- o aviso de validação;
- o seletor de data;
- o clique no Botão com Redefinir;
- Destacar campos (`Shift+L`), com a preferência lembrada.

- [ ] Testes RED: `keystroke_filter_blocks_letters_in_numbers`, `date_picker_writes_the_format`.
- [ ] Implementar, GREEN, commit; E2E.

### Fim da etapa
- [ ] `simplify`, E2E, `measure.sh`, revisão Opus e uma rodada de correções.
- [ ] Merge, AppImage, docs 0.19.0, tag, push, release e página de andamento.
