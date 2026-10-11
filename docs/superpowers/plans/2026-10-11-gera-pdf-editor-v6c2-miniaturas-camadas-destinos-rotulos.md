# v6c2: Miniaturas, Camadas, Destinos e rótulos de página. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:**
- Miniaturas com seleção, menu e arraste.
- Rótulos de página (diálogo e caixa de página).
- Painéis Destinos e Camadas.
- Copiar páginas entre PDFs.
- Os 7 ajustes menores da v6c.

**Arquitetura:**
- `pdf-core` ganha `page_labels.rs`, `destinations.rs` e `layers.rs`.
  - Rótulos, destinos e propriedades de camada são desfazíveis por snapshot.
  - O olho das camadas é estado de vista, reaplicado depois de reabrir.
- `gera-ui` ganha os painéis `thumbnails.rs`, `destinations_panel.rs` e `layers_panel.rs`, e o
  diálogo `page_labels_dialog.rs`.

**Tecnologia:** Rust, mupdf 0.8 (objetos PDF, `write_to_with_options`, reabrir dos bytes), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6c2-miniaturas-camadas-destinos-rotulos-design.md`

## Restrições globais

- Leituras com teto:
  - faixas de rótulo: 10 000;
  - destinos: 50 000 nomes e 32 níveis de `/Kids`;
  - camadas: 10 000;
  - `/Order`: profundidade 32.
- Sem `unwrap` sobre dados do PDF. Texto do PDF na tela sempre por `links::safe_display`.
- As edições recusam documento protegido contra alteração ou assinado (`require_modifiable`).
- O olho das camadas nunca chega ao arquivo salvo nem ao desfazer.
- Atalhos e nomes como no Acrobat (pt-BR).
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. Olho de camada desligado, depois Salvar: o arquivo salvo mantém o `/D` original e a vista continua
   com a camada escondida.
2. Rótulos em PDF que já tem `/PageLabels` com `/Kids` ou fora de ordem: lê certo, e uma nova seção
   preserva as outras faixas.
3. Destino com nome repetido ou vindo do `/Dests` antigo: renomear e excluir funcionam nos dois.
4. Arrastar miniaturas até a própria aba ou até a aba Início/Ferramentas: nada acontece.
5. Copiar páginas de documento protegido contra cópia: recusado com mensagem, sem arquivo temporário
   sobrando.

---

### Tarefa 1: rótulos de página no pdf-core

**Produz:**
- `PageLabelStyle { None, Decimal, UpperRoman, LowerRoman, UpperAlpha, LowerAlpha }`;
- `PageLabelRange { start_page: i32, style: PageLabelStyle, prefix: String, first: i32 }`;
- `pub fn format_label(style, prefix, n) -> String`;
- `pub fn apply_labels(ranges: &[PageLabelRange], from: i32, to: i32, new: Option<PageLabelRange>, total: i32) -> Vec<PageLabelRange>`:
  - com `new` = `None`, estende a seção anterior;
  - a faixa depois de `to` continua com a numeração que tinha;
- `PdfDoc::page_labels()`, `PdfDoc::labels() -> Vec<String>` (vazio sem `/PageLabels`) e
  `PdfDoc::set_page_labels(Vec<PageLabelRange>)`;
- `Request::PageLabels(Vec<PageLabelRange>)` → `Changed::Labels`;
- `DocSummary.labels`.

- [ ] Testes RED:
  - unidade: romanos (1, 4, 9, 14, 40, 90, 400, 3999, 4000 → decimal), letras (1 → a, 26 → z,
    27 → aa, 52 → zz, 53 → aaa) e `apply_labels` (nova seção no meio; estender);
  - integração `tests/page_labels.rs`:
    - `labels_round_trip_and_undo`;
    - `nums_out_of_order_and_kids_cycle_are_read`;
    - `a_new_section_keeps_the_other_ranges`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: destinos no pdf-core

**Produz:**
- `DestInfo { name, page: Option<i32>, at: Option<(f32, f32)> }`;
- `DestOp { Add{name, page, at}, Rename{name, to}, Remove{name}, Set{name, page, at} }`;
- `PdfDoc::destinations()` e `PdfDoc::edit_destination(DestOp) -> Result<Option<String>>` (o nome
  final no Add);
- `Request::Destination(DestOp)` → `Response::DestinationsChanged{destinations, created, history}`;
- `DocSummary.destinations`.

- [ ] Testes RED em `tests/destinations.rs`:
  - `lists_name_tree_with_kids_and_old_dests_dict`;
  - `add_rename_remove_and_undo`;
  - `repeated_name_gets_a_suffix`;
  - `editing_an_old_dests_entry_moves_it_to_the_name_tree`;
  - `kids_cycle_is_read_without_hanging`.
- [ ] Implementar (o destino aponta para a vista com `[página /XYZ x y null]`, reaproveitando
  `dest_array` do `outline.rs`, que passa a ser `pub(crate)`), GREEN, commit.

### Tarefa 3: camadas no pdf-core

**Produz:**
- `LayerNode { Layer{ id: i32, name: String, visible: bool, default_on: bool, locked: bool, kids: Vec<LayerNode> }, Label{ text: String, kids: Vec<LayerNode> } }`;
- `PdfDoc::layers() -> Vec<LayerNode>`;
- `PdfDoc::set_layer_view(id, on)` e `PdfDoc::reset_layer_view()`:
  - sem histórico e sem marcar alterado;
  - grava `/D /ON` e `/OFF` em memória e reabre;
- `PdfDoc::edit_layer(id, name, default_on)` (snapshot);
- `save` e `snapshot` gravam o `/D` original; `restore` e `save` reaplicam a vista;
- `Request::LayerView{id,on}`, `Request::ResetLayers` e `Request::EditLayer{id,name,default_on}` →
  `Response::LayersChanged{layers, history}` (reabrir muda a renderização: a aba também limpa o
  cache);
- `DocSummary.layers`.

- [ ] Testes RED em `tests/layers.rs`:
  - `nested_order_with_labels_and_locked`;
  - `eye_hides_content_without_dirtying`: o pixel do retângulo da camada some no render, e
    `is_dirty()` continua falso;
  - `eye_state_never_reaches_the_saved_file` (foco 1);
  - `eye_survives_undo_and_save`;
  - `layer_properties_edit_and_undo`;
  - `document_without_layers_lists_nothing`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: painel de miniaturas

**Produz:**
- `thumbnails.rs`, que substitui `sidebar::thumbnails`:
  - `ThumbnailsPanel { sel: PageSet, drag: Option<(i32, Option<i32>)>, width: f32 }`;
  - `ThumbAction { Go(i32), Op(PageOp), Org(OrgDialog), Extract(Vec<i32>), Labels(Vec<i32>), Print(Vec<i32>), CopyTo{pages, tab}, DragOut(Vec<i32>), Zoom(f32) }`;
- funções puras `drop_index(pointer_y, rows) -> i32` e `key_action(key, sel, total)`;
- em `doc_tab`:
  - os diálogos do Organizar aparecem fora do modo (`organize::dialog_window`);
  - `org_action()` é extraído de `organize_view` e usado pelos dois.

- [ ] Testes RED (unidade):
  - alvo do arraste (antes da primeira, entre duas, depois da última);
  - `Delete` com tudo selecionado não exclui (sempre sobra uma página);
  - setas mudam a seleção;
  - Ampliar/Reduzir entre 80 e 320.
- [ ] Implementar o menu de contexto:
  - Inserir páginas ▸ De arquivo…/Página em branco…;
  - Extrair…, Substituir…, Excluir…, Recortar…;
  - Girar 90° horário/anti-horário;
  - Rótulos de página…, Imprimir páginas…;
  - Copiar para ▸;
  - Ampliar/Reduzir miniaturas.
- [ ] GREEN e commit; E2E do menu e do arraste.

### Tarefa 5: copiar páginas entre PDFs

**Produz:**
- `Request::ExportPages{pages, dest}` → `Response::PagesExported{dest}`, que grava só as páginas (com
  anotações) por `extract` existente;
- prefixo `COPY_PREFIX = "gera-copia-"` na varredura do spool;
- `DocEvent::CopyPages{dest_tab_path, file}` e, em `app`, inserção no fim do destino com
  `PageOp::InsertFrom`, apagando o temporário depois da resposta;
- arraste até o rótulo de outra aba (`shell::tabs` informa a aba sob o ponteiro ao soltar).

- [ ] Testes RED:
  - pdf-core `export_then_insert_copies_pages_with_annotations`;
  - gera-ui `drop_on_own_or_fixed_tab_does_nothing` (foco 4);
  - `copy_from_protected_document_is_refused` (foco 5).
- [ ] Implementar, GREEN, commit; E2E com duas abas.

### Tarefa 6: rótulos na interface

**Produz:**
- `page_labels_dialog.rs`: Páginas (Todas/Selecionadas/De–Até) e Numeração (Iniciar nova seção:
  Estilo, Prefixo, Início; ou Estender);
- a caixa de página mostra "iii (3 de 10)" quando `prefs.logical_page_numbers` e há rótulos;
- digitar um rótulo vai até a página dele (`app_state::pages::page_for_input(input, labels, total)`);
- preferência "Usar números lógicos de página" (Exibição de página).

- [ ] Testes RED:
  - `page_for_input` (número, rótulo, rótulo repetido → a primeira, entrada inválida);
  - texto da caixa com e sem rótulos.
- [ ] Implementar, GREEN, commit; E2E do diálogo.

### Tarefa 7: painéis Destinos e Camadas

**Produz:**
- `LeftPanel::Destinations` ("Destinos") e `LeftPanel::Layers` ("Camadas").
- `destinations_panel.rs`:
  - colunas Nome e Página, ordenação por clique no cabeçalho;
  - Opções: Novo destino e Ordenar;
  - menu Ir, Definir destino, Renomear (no lugar) e Excluir;
  - clique duplo vai até o destino.
- `layers_panel.rs`:
  - árvore, olho e cadeado;
  - Opções: Expandir tudo, Recolher tudo, Redefinir para visibilidade inicial e Propriedades da
    camada (janela com nome e estado padrão).

- [ ] Testes RED:
  - ordenação de destinos (nome natural: "cap2" antes de "cap10"; página);
  - rótulo do olho e do cadeado;
  - árvore de camadas achatada para a exibição.
- [ ] Implementar, GREEN, commit; E2E dos dois painéis.

### Tarefa 8: ajustes adiados da v6c

- [ ] M-1 `bookmarks::drop_op`: teste `dropping_the_last_child_on_its_parent_is_a_no_op` → filtrar o
  arrastado.
- [ ] M-2: teste `enter_without_change_sends_no_rename`.
- [ ] M-3 `AttachmentsPanel`: seleção por `(AttachmentOrigin, String)`; teste
  `selection_follows_the_attachment_after_the_list_changes`.
- [ ] M-4 `xml_escape`: teste `control_chars_are_dropped_from_xmp`.
- [ ] M-5: apagar a cópia do anexo ao fechar a aba; teste `closing_an_attachment_tab_removes_the_copy`.
- [ ] M-6 `write_outline`: promover `/Outlines` direto; teste `direct_outlines_root_is_made_indirect`.
- [ ] M-7: `broken_tree_with_a_cycle_is_read_and_fixed` confere `outline_counts` e `/Parent`.
- [ ] Commit.

### Fim da etapa

- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`.
- [ ] Baterias E2E (Xvfb) e `scripts/measure.sh` na tela real.
- [ ] Revisão independente (Opus) com o foco acima e uma rodada de correções.
- [ ] Merge local, AppImage, CHANGELOG/ROADMAP/README e versão 0.16.0, tag, push da `main` e da
  tag, release e página de andamento.
