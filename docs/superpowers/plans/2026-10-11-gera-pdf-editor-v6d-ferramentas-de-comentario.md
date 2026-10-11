# v6d: Ferramentas de comentário. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:**
- Inserir e substituir texto, Riscar na barra.
- Nuvem, polígono e linhas conectadas.
- Texto explicativo, borracha e anexar arquivo como comentário.
- Tudo editável e desfazível.

**Arquitetura:** `pdf-core` ganha os tipos novos em `annot_edit.rs`/`doc.rs` (criar, ler, mover vértice/ponta,
apagar trechos de desenho), com as aparências geradas pelo MuPDF 1.27. `gera-ui` ganha as ferramentas e as
interações em `edit.rs`, a barra Comentar e as alças.

**Tecnologia:** Rust, mupdf 0.8 (MuPDF 1.27: `set_vertices`, `set_border_effect`, dicionários `/CL` `/IT` `/FS`),
egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6d-ferramentas-de-comentario-design.md`

## Restrições globais

- Tetos: 1000 vértices por anotação, 10 000 pontos por desenho, anexo até 100 MB.
- Coordenadas não finitas descartadas.
- Sem `unwrap` sobre dados do PDF; texto do PDF na tela por `links::safe_display`.
- Edições recusam documento protegido contra alteração ou assinado (`require_modifiable`).
- Cada criação ou edição é uma etapa do desfazer; substituir texto é uma etapa só.
- Atalhos como no Acrobat:
  - `U`/`Shift+U` alternam Destacar, Sublinhar e Riscar;
  - `D`/`Shift+D` alternam os desenhos;
  - `Q` é Nuvem;
  - `K`/`Shift+J` alternam Carimbo e Anexar.
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. PDF de outro programa com `/Vertices` de tamanho ímpar, gigante ou com NaN: lê sem travar e mostra o
   que der.
2. Substituir texto e depois excluir só o riscado (ou só o caret) pelo painel: o par sai junto e o desfazer
   volta os dois.
3. Borracha passando pelo meio de um traço: o desenho fica em dois traços, e o desfazer devolve o original.
4. Polígono com clique duplo logo no segundo ponto (menos que o mínimo): nada é criado e a ferramenta
   continua.
5. Anexar arquivo de nome malicioso ou maior que 100 MB: recusado ou saneado, e nada é executado.

---

### Tarefa 1: polígono, linhas conectadas e nuvem no pdf-core

**Produz:**
- `AnnotKind::{Polygon, PolyLine, Cloud}`;
- `AnnotInfo.vertices: Vec<(f32, f32)>`;
- `NewAnnot::Polygon { page, vertices, closed, cloud }`;
- `AnnotOp::MoveVertex { page, id, index, to }`;
- leitura com teto e filtro de NaN;
- mover a anotação inteira desloca os vértices;
- `pub fn close_polygon(points: &[(f32, f32)], click: (f32, f32), tolerance: f32) -> bool`.

- [ ] Testes RED em `tests/shapes.rs`:
  - `polygon_polyline_and_cloud_round_trip`;
  - `moving_a_vertex_changes_only_it_and_undoes`;
  - `odd_or_huge_vertices_are_read_safely`.
- [ ] Unidade: `close_polygon`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: inserir e substituir texto, texto explicativo no pdf-core

**Produz:**
- `AnnotKind::{Caret, Callout}`;
- `AnnotInfo.callout: Option<[(f32, f32); 3]>` e `AnnotInfo.group: Option<i32>`;
- `NewAnnot::Caret { page, rect, text }` e `NewAnnot::ReplaceText { page, rects, text }`
  (riscado e caret, este com `/IRT` = riscado e `/RT /Group`);
- `NewAnnot::Callout { page, rect, knee, tip, text, size, color }`;
- `AnnotOp::MoveCallout { page, id, knee, tip }`;
- excluir um item do grupo exclui o par;
- mover o riscado leva o caret junto.

- [ ] Testes RED em `tests/comments.rs`:
  - `caret_with_note_round_trip`;
  - `replace_text_is_one_undo_step_and_deletes_as_a_pair`;
  - `callout_round_trip_and_move_tip`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: anexo como comentário e borracha no pdf-core

**Produz:**
- `AnnotKind::FileAttachment`;
- `NewAnnot::FileAttachment { page, at, path, description }` (teto de 100 MB, nome único);
- `pub fn erase_strokes(strokes: &[Vec<(f32, f32)>], path: &[(f32, f32)], radius: f32) -> Vec<Vec<(f32, f32)>>`;
- `AnnotOp::Erase { page, id, path, radius }`: o desenho sem traços sai.

- [ ] Unidade: `erase_strokes` (corte no meio, apaga tudo, sem tocar).
- [ ] Testes RED:
  - `tests/attachments.rs`: `file_attachment_comment_embeds_the_file_and_lists_it`,
    `attaching_more_than_100_mb_is_refused`;
  - `tests/annot_edit.rs`: `eraser_splits_a_stroke_and_undoes`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: ferramentas e barra Comentar

**Produz:**
- `Tool::{StrikeOut, Polygon, PolyLine, Cloud, Eraser, Callout, InsertText, ReplaceText, Attach}`;
- grupos `U`, `D`, `Q`, `K`/`Shift+J` em `app_state::tools` (`pub fn cycle(group, current) -> Tool`);
- barra Comentar com Riscar, menu de desenho (Retângulo, Oval, Linha, Seta, Polígono, Linhas conectadas,
  Nuvem, Lápis, Borracha) e menu de texto (Substituir texto, Inserir texto), Texto explicativo e Anexar.

- [ ] Testes RED: ciclos de `U`, `D` e `Shift+J`; `Q` vai para Nuvem.
- [ ] Implementar, GREEN, commit.

### Tarefa 5: interações na página

**Produz em `edit.rs`:**
- polígono, polilinha e nuvem por cliques (pré-visualização, clique duplo, `Esc`, `Backspace`, fechar no primeiro
  ponto);
- texto explicativo em dois arrastes;
- inserir texto (clique) e substituir texto (arrastar sobre o texto), os dois com nota;
- anexar (clique, seletor de arquivo, descrição);
- borracha (arrastar sobre o desenho).

- [ ] Testes RED (unidade, `edit.rs`):
  - `polygon_builder_closes_on_first_point_and_needs_three`;
  - `double_click_with_two_points_creates_nothing`.
- [ ] Implementar, GREEN, commit; E2E de cada ferramenta.

### Tarefa 6: edição, propriedades e painel Comentários

**Produz:**
- alças de vértice (polígono, polilinha, nuvem) e da ponta e do joelho do texto explicativo;
- Propriedades com preenchimento e intensidade da nuvem;
- rótulos no painel ("Polígono", "Linhas conectadas", "Nuvem", "Inserir texto", "Substituir texto",
  "Texto explicativo", "Anexo"), com o par de substituir texto como um item.

- [ ] Testes RED:
  - rótulos;
  - o par agrupado aparece uma vez;
  - a alça de vértice certa sob o ponteiro.
- [ ] Implementar, GREEN, commit; E2E.

### Fim da etapa

- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`.
- [ ] E2E (Xvfb) e `scripts/measure.sh` na tela real.
- [ ] Revisão independente (Opus) com o foco acima e uma rodada de correções.
- [ ] Merge local, AppImage, CHANGELOG/ROADMAP/README, versão 0.17.0, tag, push, release e página de
  andamento.
