# v6e2: organizar campos, ações e campo Imagem. Plano de implementação

> **Para agentes:** sub-skill obrigatória: superpowers:executing-plans (execução nativa, escolhida
> pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:** seleção múltipla e organização dos campos no Preparar, ações de campo e de página
executadas ao preencher, e o campo Imagem.

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6e2-organizar-acoes-imagem-design.md`

## Restrições globais

- Cada operação do Preparar é uma etapa do desfazer, mesmo quando muda vários campos.
- Tetos:
  - 64 ações numa cadeia /Next;
  - 400 cópias;
  - 10 000 itens lidos de um array.
- O JavaScript não executa.
- Abrir arquivo só aceita PDF, e com confirmação.
- Textos em pt-BR.
- Testes:
  - `cargo test -q -p app-state -p gera-ui`;
  - `cargo test -q -p pdf-core -- --test-threads=4`;
  - `cargo clippy -q --all-targets -- -D warnings`.
- E2E: `cargo build --release --features gera-ui/e2e`.

## Foco da revisão

1. Cadeia /Next em ciclo ou enorme num PDF hostil: termina, sem pânico.
2. Abrir arquivo, link ou ação Launch para um programa: nunca executa.
3. Duplicar ou colar: os widgets do mesmo campo continuam com o mesmo valor, e excluir um não
   quebra os outros.
4. Organizar 3 campos e desfazer: tudo volta numa etapa.
5. Imagem enorme ou inválida no campo Imagem: aviso, sem travar.

---

### Tarefa 1: geometria (app-state/src/arrange.rs)

**Produz:**
- `Edge::{Left, Right, Top, Bottom, CenterH, CenterV}`;
- `Axis::{H, V, Both}`;
- `SizeKind::{Width, Height, Both}`;
- `align`, `center_on_page`, `distribute`, `match_size`, `nudge`, `copies_grid` e `snap`.

- [ ] Testes RED:
  - `align_to_the_anchor`;
  - `center_moves_the_group`;
  - `distribute_spaces_centers`;
  - `match_size_keeps_the_top_left`;
  - `copies_grid_rows_and_columns`;
  - `snap_to_a_nearby_edge`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: operações de vários campos (pdf-core)

**Produz:** as `PrepareOp` `MoveMany`, `DeleteMany`, `Duplicate` e `Copies`.

- [ ] Testes RED (`tests/prepare.rs`):
  - `move_many_is_one_undo_step`;
  - `delete_many_is_one_undo_step`;
  - `duplicate_across_pages_shares_the_value`;
  - `multiple_copies_make_new_fields`;
  - `deleting_one_duplicate_keeps_the_others`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: interface de organizar (gera-ui)

**Produz:**
- seleção múltipla com `Shift`/`Ctrl`+clique, retângulo e `Ctrl+A`;
- âncora;
- mover em grupo;
- setas;
- `Delete`;
- `L R T B H V`, `Shift+H` e `Shift+V`;
- menu Organizar e menu de contexto;
- janelas Duplicar e Criar várias cópias;
- `Ctrl+C` e `Ctrl+V`;
- `P` (visualizar) e `G` (guias).

- [ ] Testes RED:
  - `shift_click_adds_and_removes`;
  - `marquee_selects_inside`;
  - `align_key_sends_move_many`;
  - `arrow_nudges_the_selection`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 4: modelo de ações (pdf-core/src/actions.rs)

**Produz:**
- `ActionKind::{GoTo { page }, Uri(String), ShowHide { fields, hide }, Reset { fields, exclude },
  Named(String), Launch(String), JavaScript(String), ImportIcon, Other}`;
- `FieldTrigger` e `PageTrigger`;
- `read_actions` e `write_actions` (/A, /AA e /Next);
- `FieldProps.actions`;
- `PrepareOp::SetPageActions`;
- `page_actions`;
- `show_fields`.

- [ ] Testes RED:
  - `each_action_round_trips`;
  - `next_chain_is_read_in_order`;
  - `next_cycle_is_cut`;
  - `page_actions_round_trip`;
  - `show_hide_sets_the_hidden_flag`.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: executar e editar ações (gera-ui)

**Produz:**
- a aba Ações (lista por gatilho, editor de ação);
- Propriedades da página com Ações;
- a execução ao clicar, passar o mouse, focar e mudar de página.

- [ ] Testes RED:
  - `button_click_runs_mouse_up_actions`;
  - `hover_runs_enter_and_exit`;
  - `actions_tab_builds_the_props`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 6: campo Imagem e opções do botão

**Produz:**
- `NewFieldKind::Image`;
- `FieldProps.layout`, `.behavior` e `.icon`;
- o /AP do botão com ícone;
- `set_button_icon`;
- a aba Opções;
- o clique no campo Imagem, que abre o seletor (`GERA_E2E_PICK` no E2E).

- [ ] Testes RED:
  - `image_field_is_created_like_acrobat`;
  - `button_icon_is_drawn`;
  - `invalid_image_changes_nothing`;
  - `options_tab_builds_the_props`.
- [ ] Implementar, GREEN, commit; E2E.

### Fim da etapa

- [ ] `simplify`, E2E, `measure.sh`, revisão Opus e uma rodada de correções.
- [ ] Merge, AppImage, docs 0.20.0, tag, push, release e página de andamento.
