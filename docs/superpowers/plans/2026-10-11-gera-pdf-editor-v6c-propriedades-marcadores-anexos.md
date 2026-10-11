# v6c: Propriedades, Marcadores e Anexos. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:**
- Propriedades editáveis, com abas e XMP.
- Painel Marcadores editável: Ctrl+B, renomear, excluir, arrastar, estilo.
- Painel Anexos: abrir com aviso, salvar, adicionar, excluir.

**Arquitetura:**
- `pdf-core` ganha três módulos de leitura e edição (`properties_edit.rs`, `outline.rs`,
  `attachments.rs`), todos desfazíveis por snapshot.
- `gera-ui` ganha as janelas e os painéis.

**Tecnologia:** Rust, mupdf 0.8 (API de arquivos embutidos e objetos PDF), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-11-gera-pdf-editor-v6c-propriedades-marcadores-anexos-design.md`

## Restrições globais

- Leituras com teto:
  - fontes: 10 000;
  - nós de marcador: 50 000, profundidade 64;
  - anexo: 100 MB para adicionar, 512 MB para ler.
- As edições recusam documento protegido contra alteração.
- Sem `unwrap` sobre dados do PDF.
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. Árvore de marcadores com ciclo, `/Count` errado ou `/Parent` faltando: lê sem travar e grava
   consertada.
2. Mover um marcador para dentro de um descendente dele: recusado, e a árvore não muda.
3. Anexo com nome malicioso (`../../x`, `/etc/passwd`, nome vazio): o nome sugerido ao salvar é saneado.
4. PDF/A: editar o título mantém `pdfaid:part` no XMP.
5. Desfazer depois de cada operação nova: volta o estado exato (marcadores, propriedades, anexos).

---

### Tarefa 1: propriedades no pdf-core

**Produz:**
- `PropertiesEdit { title, author, subject, keywords, custom: Vec<(String, String)>, initial: InitialView, window: WindowOptions, lang: String, print: PrintPrefs }`;
- `DocProperties` estendido (os mesmos campos, mais `fonts: Vec<FontInfo>`);
- `PdfDoc::set_properties(&PropertiesEdit) -> Result<()>`;
- `Request::SetProperties(PropertiesEdit)`, que responde `PagesChanged` (o resumo leva a visualização
  inicial).

- [ ] Testes RED em `tests/properties_edit.rs`:
  - `description_and_custom_round_trip`;
  - `standard_names_are_refused_as_custom`;
  - `initial_view_and_print_prefs_round_trip`;
  - `xmp_follows_the_title_and_keeps_pdfa_id`;
  - `undo_restores_the_old_properties`;
  - `fonts_are_listed_with_embedding`.
- [ ] Implementar, GREEN, commit.

### Tarefa 2: marcadores no pdf-core

**Produz:**
- `OutlineNode`;
- `PdfDoc::outline_tree()`;
- `OutlineOp`;
- `PdfDoc::edit_outline(OutlineOp) -> Result<Option<i32>>`;
- `Request::Outline(OutlineOp)`, que responde `OutlineChanged { outline, history }`.
- `DocSummary.outline: Vec<OutlineNode>`.

- [ ] Testes RED em `tests/outline.rs`:
  - `reads_titles_destinations_and_style`;
  - `add_rename_delete_with_children`;
  - `move_reorders_and_nests_and_refuses_cycles`;
  - `counts_are_rewritten`;
  - `broken_tree_with_a_cycle_is_read_and_fixed`;
  - `each_op_undoes_in_one_step`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: anexos no pdf-core

**Produz:**
- `AttachmentInfo`;
- `AttachmentOrigin`;
- `PdfDoc::attachments()`, `add_attachment`, `remove_attachment`, `set_attachment_description` e
  `attachment_bytes`;
- os pedidos ao worker `Request::Attachments*` e a resposta `AttachmentsChanged`.
- `DocSummary.attachments`.

- [ ] Testes RED em `tests/attachments.rs`:
  - `lists_document_and_comment_attachments`;
  - `add_gives_unique_names_and_bytes_round_trip`;
  - `remove_and_description_undo`;
  - `too_big_is_refused`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: janela Propriedades com abas

- [ ] Testes RED:
  - `properties_draft_cancel_keeps_the_document`;
  - `display_title_changes_the_tab_title`.
- [ ] Implementar `shell/properties.rs` com as abas Descrição, Segurança, Fontes, Visualização inicial,
  Personalizado e Avançado.
- [ ] GREEN, commit.

### Tarefa 5: painel Marcadores

- [ ] Testes RED:
  - `drop_zone_by_thirds`;
  - `ctrl_b_adds_after_the_selected_with_the_selected_text`;
  - `panel_keys`.
- [ ] Implementar o painel (barra, árvore, renomear no lugar, menu, arrastar, teclas) e o `Ctrl+B`.
- [ ] GREEN, commit.

### Tarefa 6: painel Anexos

- [ ] Testes RED:
  - `only_pdf_attachments_open`;
  - `save_name_is_sanitized`.
- [ ] Implementar o painel, o aviso de abrir, Salvar, Adicionar, Excluir, Descrição e o `/PageMode
  /UseAttachments`.
- [ ] GREEN, commit.

### Fim da etapa

- simplify → E2E → medição → revisão do Opus → uma rodada de correções → merge → AppImage → página →
  0.15.0.
