# v6e4: dados do formulário, Enviar, nivelar e preenchimento automático. Plano

> **Para agentes:** sub-skill obrigatória: superpowers:executing-plans (execução nativa).

**Spec:** `docs/superpowers/specs/2026-10-12-gera-pdf-editor-v6e4-dados-enviar-nivelar-autopreencher-design.md`

## Restrições globais

- Arquivos de dados lidos com tetos: 2 MB, 10 000 campos e profundidade 32.
- Enviar formulário:
  - só http, https e mailto;
  - sempre com confirmação;
  - tempo-limite de 30 s e resposta de no máximo 1 MB.
- O preenchimento automático nunca guarda senha.
- Testes:
  - `cargo test -q -p app-state -p gera-ui`;
  - `cargo test -q -p pdf-core -- --test-threads=4`;
  - clippy;
  - E2E.

## Foco da revisão

1. Arquivo de dados hostil (XML aninhado, FDF com ciclo em /Kids, nomes enormes): termina e não
   trava.
2. Importar só mexe nos campos existentes, numa etapa do desfazer, sem tocar em assinaturas.
3. Enviar nunca sai sem confirmação, nem para um esquema fora da lista.
4. Nivelar e desfazer devolvem o formulário.

---

### Tarefa 1: exportar e importar dados (form_data.rs)

- [ ] Testes RED:
  - `export_import_round_trip_in_every_format`;
  - `hostile_data_files_are_capped`;
  - `import_is_one_undo_step_and_skips_unknown`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: mesclar em planilha e nivelar

- [ ] Testes RED:
  - `merge_three_files_into_csv`;
  - `flatten_bakes_fields_and_undo_restores`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: ação Enviar (submit.rs + interface)

- [ ] Testes RED:
  - `submit_action_round_trips`;
  - `submit_payloads`;
  - `submit_url_is_checked`.
- [ ] Implementar (envio numa thread, confirmação), GREEN, commit.

### Tarefa 4: preenchimento automático (app-state + interface)

- [ ] Testes RED:
  - `suggestions_by_prefix`;
  - `passwords_and_numbers`;
  - `list_is_capped`.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: interface dos dados e do nivelar

- Menu Organizar ▸ Dados do formulário (Exportar, Importar, Mesclar) e Nivelar; seletores de arquivo.
- [ ] Teste RED: `data_menu_routes_requests`.
- [ ] Implementar, GREEN, commit; E2E.

### Fim da etapa

- [ ] `simplify`, E2E, `measure.sh`, revisão Opus e uma rodada de correções.
- [ ] Merge, AppImage, docs 0.22.0, tag, push, release e página de andamento.
