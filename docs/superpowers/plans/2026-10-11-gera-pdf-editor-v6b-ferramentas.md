# v6b: ferramentas de tela, preferências, menus e busca. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:** ferramentas Mão e Zoom com marca, aceleradores de tecla única, Preferências (`Ctrl+K`),
Recortar/Copiar/Colar de comentários, imagens e texto, Arquivo → Reverter/Criar/Exportar/Proteger, busca
com opções e `F3`, menu de contexto da página, `F1`, e as pendências da v6a.

**Arquitetura:**
- `pdf-core`: a busca com opções (sobre o motor da redação).
- `app-state`: ferramentas, `fit_rect`, preferências e opções de busca.
- `gera-ui`: gestos, janelas e menus.

**Tecnologia:** Rust, egui 0.36, mupdf 0.8, arboard 3 (já na árvore pelo egui).

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6b-ferramentas-design.md`

## Restrições globais

- Atalhos iguais aos do Acrobat. As teclas únicas só valem com a preferência ligada, nunca em campo de
  texto.
- Imagem colada: teto de 40 Mpx. Texto colado: até 10 000 caracteres.
- Sem `unwrap` sobre dados de fora (área de transferência, PDF).
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. Espaço segurado dentro de um campo de texto (busca, nota): digita espaço, e não vira Mão.
2. Colar quando o documento proíbe comentários: avisa e não cria nada.
3. Área de transferência com imagem gigante ou corrompida: recusa sem travar.
4. Busca com caracteres de regex (`a.b`, `(x)`, `\`): casa o texto literal.
5. Reverter com edições de comentário e de página: reabre exatamente o arquivo salvo e limpa o desfazer.

---

### Tarefa 1: app-state (ferramentas, `fit_rect`, preferências, opções de busca)

**Produz:**
- `Tool::{Hand, Zoom}`, que ficam fora de `Tool::ALL` (a linha Comentar);
- `zoom::fit_rect(rect_pts: (w, h), avail: (w, h)) -> f32`, com limites;
- os campos novos de `Prefs`:
  - `single_key: bool`;
  - `default_mode: Option<ViewMode>`;
  - `default_zoom: DefaultZoom { Auto, FitPage, FitWidth, FitHeight, Level(f32) }`;
  - `restore_views: bool` (padrão `true`);
  - `max_recents: usize` (padrão 20, de 1 a 50);
  - `last_views: BTreeMap<String, ViewEntry>` (até 200).
- `ViewEntry` passa a ser `Serialize`/`Deserialize`. `ViewMode` e `Fit` também.
- `pdf_core::SearchOptions { whole_words, match_case, bookmarks, comments }`, `Default`, com todos
  desligados, menos `comments`, que fica desligado também (como no Acrobat).

- [ ] Testes RED:
  - `hand_and_zoom_are_not_comment_tools`;
  - `fit_rect_fills_the_window_and_is_capped`;
  - `prefs_defaults_match_acrobat`;
  - `last_views_are_capped`;
  - `bad_prefs_are_sanitized` (`max_recents` 0 → 1, 999 → 50).
- [ ] GREEN e commit.

### Tarefa 2: busca com opções no pdf-core

**Produz:**
- `PdfDoc::search_with(page, needle, &SearchOptions) -> Result<Vec<PdfRect>>`;
- `PdfDoc::search_bookmarks(needle, &SearchOptions) -> Vec<(i32, PdfRect)>`.
- `Request::Search { id, needle, options, cancel }`.
- O worker manda os marcadores primeiro, depois página a página o texto e os comentários (`content`)
  dessa página.

- [ ] Testes RED em `tests/search.rs`:
  - `whole_words_skip_parts_of_words`;
  - `match_case_is_optional`;
  - `regex_characters_are_literal`;
  - `comments_are_found_when_asked`;
  - `bookmarks_are_found_when_asked`.
- [ ] Implementar com o motor de `find_patterns` (regex escapada, `(?i)` sem `match_case`, `\b` com
  `whole_words`). O `search` antigo passa a chamar `search_with`.
- [ ] GREEN e commit.

### Tarefa 3: Mão, Espaço e Zoom com marca

- [ ] Testes RED:
  - `marquee_zoom_fits_the_rectangle` (`DocView::zoom_to_rect` devolve o zoom e a rolagem x, y);
  - `space_hand_is_temporary` (estado puro `TempTool`: pressionar guarda a anterior, soltar devolve).
- [ ] Implementar:
  - os botões na barra;
  - os gestos no `canvas` (Mão: `scroll` pelo arraste; Zoom: retângulo e cliques);
  - `scroll_to(x, y)`;
  - o Espaço sai da virada de página.
- [ ] GREEN e commit.

### Tarefa 4: aceleradores e Preferências

- [ ] Testes RED:
  - `single_key_shortcuts_only_with_the_preference` (`shortcuts_for(&prefs)`);
  - `preferences_window_cancel_keeps_the_old_values` (estado `PrefsDraft`);
  - `restore_last_view_on_reopen` (`DocTab` recebe a `ViewEntry` na abertura);
  - `recents_respect_the_maximum`.
- [ ] Implementar:
  - `shell/preferences.rs` (categorias, OK/Cancelar);
  - o `Ctrl+K` e o menu;
  - a vista padrão nos documentos novos;
  - gravar a última vista ao fechar e ao sair.
- [ ] GREEN e commit.

### Tarefa 5: Recortar, Copiar, Colar e Desmarcar tudo

- [ ] Testes RED (`gera-ui/src/clipboard.rs`, puro):
  - `copied_comment_pastes_with_offset_on_the_same_page`;
  - `newer_outside_text_wins_over_the_copied_comment`;
  - `pasted_text_becomes_a_text_box_and_is_capped`;
  - `huge_image_is_refused`;
  - `paste_is_denied_without_comment_permission`.
- [ ] Implementar:
  - `DocCommand::{Cut, Paste, DeselectAll}`;
  - a leitura de imagem pelo `arboard`, numa thread, com teto;
  - o carimbo de imagem pelo caminho do `fill_sign`.
- [ ] GREEN e commit.

### Tarefa 6: Arquivo, busca na interface, menu de contexto e Ajuda

- [ ] Testes RED:
  - `revert_only_with_unsaved_changes`;
  - `search_keys_match_acrobat` (`F3`, `Ctrl+G`, `Shift+F3`, `Ctrl+Shift+G`);
  - `enter_in_the_search_box_steps_after_the_first_search` (`SearchState`);
  - `help_opens_the_project_readme`.
- [ ] Implementar:
  - os itens do menu Arquivo;
  - Reverter (pergunta e reabre);
  - o ⏷ de opções da busca;
  - o menu de contexto da página;
  - o `F1`.
- [ ] GREEN e commit.

### Tarefa 7: pendências da v6a

- [ ] Testes RED:
  - `dragging_the_page_number_records_one_view`;
  - `escape_goes_to_open_windows_first`;
  - `the_tab_brought_forward_in_full_screen_presents`;
  - `full_screen_question_belongs_to_its_tab`;
  - `zoom_focus_is_not_left_pending`.
- [ ] GREEN e commit.

### Fim da etapa

- `simplify` → E2E → `measure.sh` → revisão do Opus → uma rodada de correções → merge → AppImage →
  página → 0.14.0.
