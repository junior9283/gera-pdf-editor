# v6d2: Carimbos personalizados, FDF/XFDF, Resumir e agrupar. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:** fechar a linha Comentar: carimbos personalizados, exportar e importar comentários,
Resumir comentários e agrupar, mais os 6 ajustes adiados da v6d.

**Arquitetura:**
- `pdf-core` ganha `annot_group.rs`, `comments_io.rs` (FDF/XFDF/PDF), `comment_summary.rs` e
  `stamp_custom.rs`.
- `app-state` ganha o catálogo de carimbos em disco (`custom_stamps.rs`).
- `gera-ui` ganha os menus, os diálogos e o `Ctrl`+clique.

**Tecnologia:** Rust, mupdf 0.8 (MuPDF 1.27), quick-xml 0.41 (já na árvore), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6d2-carimbos-fdf-resumo-agrupar-design.md`

## Restrições globais

- Tetos:
  - importar: 5000 comentários por arquivo e 10 MB por XFDF;
  - resumo: 5000 comentários e 2000 páginas, 2000 caracteres por item;
  - carimbo: 300 pt no lado maior;
  - nomes de carimbo e categoria: até 64 caracteres, sem `/`, `\`, `..` nem caracteres de
    controle.
- Coordenadas não finitas descartadas.
- Sem `unwrap` sobre dados de arquivo; texto de arquivo na tela por `links::safe_display`.
- Importar, agrupar e desagrupar recusam documento protegido contra alteração ou assinado
  (`require_modifiable`). Cada um é uma etapa do desfazer.
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. XFDF malicioso: XML gigante, entidades, atributos com NaN, página negativa, `inreplyto` em
   ciclo. Tem que ler com teto, sem travar e sem pânico.
2. FDF ou PDF importado com aparências enormes ou referências em ciclo: o enxerto não pode travar
   nem trazer objetos sem limite.
3. Nome de carimbo ou categoria com `../`, `/` ou link simbólico: nada sai da pasta de carimbos.
4. Agrupar e depois excluir, mover ou desfazer só um membro: o grupo inteiro age e o desfazer
   volta tudo.
5. Resumo de documento com páginas giradas, de tamanhos diferentes ou sem comentários: as linhas
   de conexão apontam para o lugar certo.

---

### Tarefa 1: agrupar no pdf-core
**Produz:**
- `AnnotOp::Group { page, ids }` (o primeiro id é o principal) e `AnnotOp::Ungroup { page, id }`;
- `AnnotOp::DeleteMany { page, ids }`, numa etapa;
- `Move` de um membro gera um `Batch` com o grupo todo.

- [ ] Testes RED (`tests/comments.rs`):
  - `grouping_moves_and_deletes_together_and_undoes`;
  - `ungroup_splits_and_undoes`;
  - `replies_and_other_pages_are_not_grouped`;
  - `delete_many_is_one_step`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: multisseleção e Agrupar na interface
**Produz:**
- `EditState.extra: Vec<(i32, i32)>`: `Ctrl`+clique na mesma página;
- clicar num membro seleciona o grupo;
- menu de contexto com Agrupar e Desagrupar;
- `Delete` com vários selecionados.

- [ ] Testes RED (unidade): `ctrl_click_toggles_extra_selection_on_the_same_page`,
  `group_members_select_together`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 3: FDF e PDF (exportar e importar) no pdf-core
**Produz:**
- `CommentFormat::{Fdf, Xfdf}`, decidido pela extensão;
- `PdfDoc::export_comments(&self, path, format, only: Option<&[i32]>) -> Result<usize>`;
- `PdfDoc::import_comments(&mut self, path) -> Result<ImportReport>`, que aceita FDF ou PDF;
- respostas (`/IRT`) remapeadas.

- [ ] Testes RED (`tests/comments_io.rs`):
  - `fdf_round_trip_keeps_kind_page_text_author_color_and_replies`;
  - `import_from_another_pdf`;
  - `import_skips_missing_pages_and_counts_them`;
  - `import_is_one_undo_step`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: XFDF
**Produz:** `xfdf_write(&[AnnotInfo], &ReplyMap) -> String` e `xfdf_read(&str) -> Result<Vec<XfdfAnnot>>`,
com tetos.

- [ ] Testes RED:
  - unidade: cor `#RRGGBB`, `rect`, `coords`, `vertices`, `gesture`, XML malformado e gigante;
  - integração: `xfdf_round_trip_keeps_kind_page_text_author_color_and_replies`.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: menu do painel Comentários (exportar e importar)
**Produz:**
- menu "···" do painel com Exportar todos, Exportar selecionados e Importar arquivo de dados;
- seletor de arquivo com filtros FDF/XFDF/PDF;
- aviso de ignorados.

- [ ] Teste RED: `request_for_menu_item` (qual pedido sai de cada item).
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 6: Resumir comentários no pdf-core
**Produz:** `SummaryLayout` (4), `SummarySort`, `SummaryOptions { layout, sort, only, pages, font,
include_empty }` e `summarize_comments(&PdfDoc, &SummaryOptions, dest) -> Result<usize>`.

- [ ] Testes RED (`tests/comment_summary.rs`):
  - `each_layout_has_the_expected_pages_and_text`;
  - `sort_by_author_and_date`;
  - `rotated_page_connector_hits_the_comment`;
  - `empty_pages_only_when_asked`.
- [ ] Implementar, GREEN, commit.

### Tarefa 7: diálogo Resumir comentários
**Produz:** diálogo com as opções da spec; o resultado vai para uma pasta temporária privada e
abre numa aba nova, como documento novo sem nome ("Resumo de comentários em <arquivo>").

- [ ] Teste RED: `options_from_dialog`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 8: carimbos personalizados (núcleo)
**Produz:**
- `app_state::custom_stamps`: `Catalog::load(dir)`, `create(category, name, src_pdf)`,
  `rename`, `move_to`, `remove`, e `sanitize(name) -> Option<String>`;
- `pdf_core::stamp_pdf_from_file(src, page, dest) -> Result<(f32, f32)>`;
- `NewAnnot::CustomStamp { page, rect, source, name }`, com aparência de Form XObject.

- [ ] Testes RED:
  - `sanitize_rejects_traversal_and_control_chars`;
  - `catalog_lists_categories_and_stamps`;
  - `image_and_pdf_become_one_page_stamps`;
  - `custom_stamp_is_drawn_and_survives_save_and_move`.
- [ ] Implementar, GREEN, commit.

### Tarefa 9: carimbos personalizados (interface)
**Produz:**
- menu do Carimbo com as categorias, Criar… e Gerenciar…;
- diálogo Criar (arquivo, página, categoria, nome);
- diálogo Gerenciar (renomear, mudar de categoria, excluir).

- [ ] Teste RED: `stamp_choice_for_custom_entry`.
- [ ] Implementar, GREEN, commit; E2E.

### Tarefa 10: ajustes adiados da v6d (M1–M6)
- [ ] M1: o `is_empty` do polígono conta só os vértices finitos.
- [ ] M2: `replace_text` remove o cursor se o riscado falhar.
- [ ] M3: traços que a borracha não tocou ficam, inclusive os de um ponto.
- [ ] M4: Anexar com o seletor ocupado avisa.
- [ ] M5: anexo exige arquivo comum (`is_file`).
- [ ] M6: grupo apontando para si mesmo ou em ciclo aparece no painel.
- [ ] Um teste RED por item, GREEN, commit.

### Fim da etapa
- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`.
- [ ] E2E (Xvfb) e `scripts/measure.sh` na tela real.
- [ ] Revisão independente (Opus) com o foco acima e uma rodada de correções.
- [ ] Merge local, AppImage, CHANGELOG/ROADMAP/README, versão 0.18.0, tag, push, release e página
  de andamento.
