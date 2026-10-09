# Gera PDF Editor v1.1-B2a — Anotações editáveis — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** anotações viram objetos editáveis no documento em memória (selecionar, mover, redimensionar, apagar, cor/opacidade/espessura, texto da nota), com desfazer/refazer próprio, inclusive para anotações de outros programas.

**Architecture:** o `pdf-core` aplica cada operação direto no `PdfDocument` aberto e guarda a operação inversa num histórico (`Edit`), com identidade estável por número de objeto PDF. Apagar e desfazer-criar **escondem** (flag *Hidden*) em vez de remover; salvar remove de verdade as escondidas por nós. O worker responde com `AnnotsChanged` (lista, páginas a re-renderizar, undo/redo/dirty). A interface deixa de desenhar pendentes: só mostra a página renderizada, a seleção, as alças e a pré-visualização do gesto.

**Tech Stack:** Rust 2021, egui/eframe 0.36, mupdf 0.8.

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1.1b2a-edicao-anotacoes-design.md`

## Global Constraints

- pt-BR; trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `simplify` só na última tarefa (regra do projeto); E2E feito pelo executor em Xvfb + openbox + xdotool, sem D-Bus, config temporária (regra do projeto).
- Ao fim de cada tarefa: fmt, clippy `-D warnings`, `cargo test --workspace` verdes.
- Nada que mude o arquivo do usuário sem Salvar (as edições ficam na memória até salvar).
- Gravação segura e `Encryption::Keep` da v1 continuam no Salvar.
- Comportamento do Acrobat: marcações de texto não se movem; Delete apaga; menu de contexto com Propriedades/Excluir; duplo clique edita a nota.

## Review Focus

1. **Anotação de outro programa apagada e desfeita** volta idêntica (autor, data, aparência própria): coberto por `deleting_an_external_annotation_and_undoing_restores_it_exactly` (tarefa 2).
2. **Desfazer depois de salvar**: salvar limpa o histórico — Ctrl+Z não pode "desfazer" algo já gravado num estado inconsistente: `saving_removes_hidden_and_starts_a_clean_history` (tarefa 2).
3. **Identidade após refazer criar**: o id se mantém (esconder/mostrar), e operações seguintes sobre a mesma anotação continuam valendo: `redo_create_keeps_the_same_id` (tarefa 2).
4. **Anotações escondidas no arquivo original** (Hidden de fábrica) não aparecem na lista nem são removidas ao salvar: `originally_hidden_annotations_are_left_alone` (tarefa 2).
5. **Re-render só das páginas mudadas**, sem descartar o cache inteiro (memória/fluidez): `invalidate_page_drops_only_that_page` (tarefa 5).

---

### Tarefa 1: `pdf-core` — `AnnotInfo` com identidade e estilo

**Files:** `crates/pdf-core/src/types.rs`, `crates/pdf-core/src/doc.rs`, `crates/gera-ui/src/sidebar.rs` (rótulo de `Other`); Test `crates/pdf-core/tests/annotations.rs`.

**Produces:**
- `AnnotKind::Other` (tipos que não criamos, de outros programas).
- `AnnotInfo { pub id: i32, pub kind, pub page, pub rect, pub contents, pub color: Option<[f32; 3]>, pub opacity: f32, pub width: Option<f32> }` + métodos `movable(&self) -> bool` (Note, Ink) e `resizable(&self) -> bool` (Ink).
- `annotations(page)` / `all_annotations()` listam só as **visíveis** (sem flag Hidden), com `id` = número do objeto.

- [ ] **Passo 1: testes**

```rust
#[test]
fn annotations_have_stable_ids_and_style() {
    let (_d, path, mut doc) = setup();
    doc.save_with(&[highlight(PdfRect::new(70.0, 90.0, 200.0, 110.0))], &path).unwrap();
    let a = &doc.annotations(0).unwrap()[0];
    let again = &PdfDoc::open(&path, None).unwrap().annotations(0).unwrap()[0];
    assert!(a.id > 0);
    assert_eq!(a.id, again.id, "o número do objeto não muda ao reabrir");
    assert!(a.color.is_some() && a.opacity > 0.0);
    assert!(!a.movable() && !a.resizable(), "destaque fica preso ao texto");
}
```

(Para Note/Ink: `movable()` verdadeiro; `resizable()` só Ink — teste simples com `AnnotInfo` montado à mão em `types.rs`.)

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar: `id` via `annot.object().as_indirect()?`; `color` via `annot.color()?` convertido para RGB (`Gray(g)` → `[g,g,g]`; `Cmyk` → conversão simples `1-min(1,c+k)` etc.); `opacity()?`; `width` = `border_width()` só para Ink; filtro `!flags().contains(IS_HIDDEN)`; tipos desconhecidos viram `Other` (não são mais descartados). Atualizar quem monta `AnnotInfo` (testes do gera-ui/sidebar). **Passo 4:** testes passam. **Passo 5:** commit `feat(pdf-core): anotações com identidade e estilo`.

---

### Tarefa 2: `pdf-core` — operações, histórico e salvar

**Files:** Create `crates/pdf-core/src/annot_edit.rs`; Modify `doc.rs`, `lib.rs`, `types.rs`; Test `crates/pdf-core/tests/annot_edit.rs`.

**Produces:**
- `pub enum AnnotOp { Create(NewAnnot), Move { page: i32, id: i32, dx: f32, dy: f32 }, Resize { page: i32, id: i32, rect: PdfRect }, Style { page: i32, id: i32, color: Option<[f32; 3]>, opacity: f32, width: Option<f32> }, SetContents { page: i32, id: i32, text: String }, Delete { page: i32, id: i32 } }` (Debug, Clone, PartialEq).
- `PdfDoc::apply(&mut self, op: AnnotOp) -> Result<Vec<i32>>` (páginas mudadas), `undo()/redo() -> Result<Vec<i32>>` (vazio se não havia), `can_undo()`, `can_redo()`, `is_dirty()`, `save(&mut self, dest) -> Result<()>`.
- `save_with(pending, dest)` continua (aplica `Create` de cada pendente e chama `save`) — mantém os testes da v1.
- `export_for_print(&self, dest, with_annots)` (sem `pending`): com marcações grava o **documento em memória** (sem criptografia); sem marcações mantém o caminho do disco removendo todas.

**Modelo interno** (`annot_edit.rs`):

```rust
/// Mudança elementar e reversível numa anotação (a inversa de cada uma é outra `Edit`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Edit {
    Hide { page: i32, id: i32 },
    Show { page: i32, id: i32 },
    /// Retângulo e, para desenho, os traços (mover e redimensionar).
    Geometry { page: i32, id: i32, rect: PdfRect, ink: Option<Vec<Vec<(f32, f32)>>> },
    Style { page: i32, id: i32, color: Option<[f32; 3]>, opacity: f32, width: Option<f32> },
    Contents { page: i32, id: i32, text: String },
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Edit>,   // inversas
    redo: Vec<Edit>,
    /// Escondidas por nós (apagadas ou criação desfeita): removidas de verdade ao salvar.
    pub(crate) hidden: HashSet<(i32, i32)>,
}
```

- `apply_edit(doc, edit) -> Result<Edit>` aplica e devolve a inversa (lendo o estado atual antes de mudar). `Hide`/`Show` mexem na flag `IS_HIDDEN` e em `hidden`.
- `PdfDoc::apply(op)`: converte em `Edit` (Create: cria com `add_one`, devolve `Hide{id}` como inversa; Move: lê rect/ink, desloca; Resize: escala os pontos do ink do rect antigo para o novo; Delete: `Hide`), aplica, empilha a inversa em `undo`, limpa `redo`.
- `undo()`: tira a inversa, aplica, empilha a inversa dela em `redo` (e vice-versa no `redo()`).
- `is_dirty()` = `!undo.is_empty()`.
- `save(dest)`: remove de verdade as `hidden` (só as nossas), grava como hoje (temporário + fsync + rename, `Encryption::Keep`), limpa `History`. Em falha: recarrega do disco e limpa o histórico (como a v1).
- Encontrar a anotação: percorrer `load_pdf_page(page).annotations()` comparando `object().as_indirect()`; não achou → `PdfError::InvalidAnnotation("anotação não encontrada")`.
- Toda mudança chama `annot.update()` e `page.update()` para o MuPDF refazer a aparência.

- [ ] **Passo 1: testes** (`tests/annot_edit.rs`):

```rust
mod common;
use common::write_sample;
use pdf_core::{AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 2, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn note() -> NewAnnot {
    NewAnnot::Note { page: 0, at: (100.0, 100.0), text: "olá".into() }
}

fn ink() -> NewAnnot {
    NewAnnot::Ink { page: 0, strokes: vec![vec![(100.0, 100.0), (200.0, 150.0)]] }
}

fn only(d: &PdfDoc) -> pdf_core::AnnotInfo {
    let mut v = d.annotations(0).unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

#[test]
fn create_undo_redo_keeps_the_same_id() {
    let (_d, _p, mut d) = doc();
    assert_eq!(d.apply(AnnotOp::Create(note())).unwrap(), vec![0]);
    let id = only(&d).id;
    assert!(d.is_dirty() && d.can_undo());
    d.undo().unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    assert!(!d.is_dirty() && d.can_redo());
    d.redo().unwrap();
    assert_eq!(only(&d).id, id, "refazer mostra a mesma anotação");
}

#[test]
fn moving_a_note_and_undoing() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let a = only(&d);
    d.apply(AnnotOp::Move { page: 0, id: a.id, dx: 30.0, dy: -10.0 }).unwrap();
    let moved = only(&d);
    assert!((moved.rect.x0 - (a.rect.x0 + 30.0)).abs() < 0.5);
    assert!((moved.rect.y0 - (a.rect.y0 - 10.0)).abs() < 0.5);
    d.undo().unwrap();
    assert!((only(&d).rect.x0 - a.rect.x0).abs() < 0.5);
}

#[test]
fn resizing_ink_scales_its_strokes() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(ink())).unwrap();
    let a = only(&d);
    let bigger = PdfRect::new(a.rect.x0, a.rect.y0, a.rect.x0 + a.rect.width() * 2.0, a.rect.y0 + a.rect.height() * 2.0);
    d.apply(AnnotOp::Resize { page: 0, id: a.id, rect: bigger }).unwrap();
    let r = only(&d).rect;
    assert!((r.width() - a.rect.width() * 2.0).abs() < 2.0, "{r:?}");
    d.undo().unwrap();
    assert!((only(&d).rect.width() - a.rect.width()).abs() < 0.5);
}

#[test]
fn style_and_contents_are_undoable() {
    let (_d, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let a = only(&d);
    d.apply(AnnotOp::Style { page: 0, id: a.id, color: Some([0.0, 0.0, 1.0]), opacity: 0.5, width: None }).unwrap();
    d.apply(AnnotOp::SetContents { page: 0, id: a.id, text: "novo".into() }).unwrap();
    let b = only(&d);
    assert_eq!(b.contents, "novo");
    assert!((b.opacity - 0.5).abs() < 0.01);
    assert_eq!(b.color.map(|c| c[2] > 0.9), Some(true));
    d.undo().unwrap();
    d.undo().unwrap();
    let c = only(&d);
    assert_eq!(c.contents, "olá");
    assert!((c.opacity - a.opacity).abs() < 0.01);
}

#[test]
fn deleting_an_external_annotation_and_undoing_restores_it_exactly() {
    let (dir, path, mut d) = doc();
    d.save_with(&[note()], &path).unwrap(); // vira "externa": está no arquivo
    let mut d = PdfDoc::open(&path, None).unwrap();
    let before = only(&d);
    d.apply(AnnotOp::Delete { page: 0, id: before.id }).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.undo().unwrap();
    assert_eq!(only(&d), before, "volta idêntica");
    let _ = dir;
}

#[test]
fn saving_removes_hidden_and_starts_a_clean_history() {
    let (_d, path, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    d.apply(AnnotOp::Create(ink())).unwrap();
    let ink_id = d.annotations(0).unwrap().iter().find(|a| a.kind == pdf_core::AnnotKind::Ink).unwrap().id;
    d.apply(AnnotOp::Delete { page: 0, id: ink_id }).unwrap();
    d.save(&path).unwrap();
    assert!(!d.is_dirty() && !d.can_undo() && !d.can_redo());
    let again = PdfDoc::open(&path, None).unwrap();
    assert_eq!(again.annotations(0).unwrap().len(), 1, "o desenho apagado saiu do arquivo");
}

#[test]
fn operations_on_unknown_ids_fail_without_changes() {
    let (_d, _p, mut d) = doc();
    assert!(d.apply(AnnotOp::Delete { page: 0, id: 99_999 }).is_err());
    assert!(!d.is_dirty());
}

#[test]
fn originally_hidden_annotations_are_left_alone() {
    // anotação já escondida no arquivo: não aparece e não é removida ao salvar
    let (_d, path, mut d) = doc();
    d.save_with(&[note()], &path).unwrap();
    common::hide_all_annotations(&path); // helper: liga IS_HIDDEN direto com mupdf
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.apply(AnnotOp::Create(ink())).unwrap();
    d.save(&path).unwrap();
    assert_eq!(common::count_raw_annotations(&path), 2, "a escondida de fábrica continua no arquivo");
}

#[test]
fn hidden_annotations_are_not_rendered() {
    let (_d, _p, mut d) = doc();
    let blank = d.render(0, 0.5, false).unwrap();
    d.apply(AnnotOp::Create(ink())).unwrap();
    assert_ne!(d.render(0, 0.5, false).unwrap(), blank);
    let id = only(&d).id;
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert_eq!(d.render(0, 0.5, false).unwrap(), blank);
}

#[test]
fn print_copy_comes_from_the_edited_document() {
    let (dir, _p, mut d) = doc();
    d.apply(AnnotOp::Create(note())).unwrap();
    let dest = dir.path().join("imprimir.pdf");
    d.export_for_print(&dest, true).unwrap();
    assert_eq!(PdfDoc::open(&dest, None).unwrap().annotations(0).unwrap().len(), 1);
    d.export_for_print(&dest, false).unwrap();
    assert!(PdfDoc::open(&dest, None).unwrap().annotations(0).unwrap().is_empty());
}
```

(Helpers `hide_all_annotations` e `count_raw_annotations` em `tests/common/mod.rs`, com `#[allow(dead_code)]`, usando `mupdf::pdf::PdfDocument` direto. Ajustar `tests/print_export.rs` à nova assinatura de `export_for_print`.)

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar conforme o modelo. **Passo 4:** `cargo test -p pdf-core` todo verde. **Passo 5:** commit `feat(pdf-core): edição de anotações no documento em memória com desfazer/refazer`.

---

### Tarefa 3: `pdf-core` — protocolo do worker

**Files:** `crates/pdf-core/src/worker.rs`; Test `crates/pdf-core/tests/worker.rs`.

**Produces:** `Request::Annot(AnnotOp)`, `Request::Undo`, `Request::Redo`; `Request::Save { dest }` (sem `pending`); `Request::ExportForPrint { dest, with_annots }`; `Response::AnnotsChanged { annots: Vec<AnnotInfo>, pages: Vec<i32>, can_undo: bool, can_redo: bool, dirty: bool }`; `Response::AnnotFailed(String)`. `Saved { annots }` continua.

- [ ] **Passo 1: teste**

```rust
#[test]
fn annotation_edits_through_the_worker() {
    let (_d, w, _) = opened(1);
    w.send(Request::Annot(AnnotOp::Create(NewAnnot::Note { page: 0, at: (50.0, 50.0), text: "x".into() })));
    let id = match recv(&w) {
        Response::AnnotsChanged { annots, pages, can_undo, dirty, .. } => {
            assert_eq!(pages, vec![0]);
            assert!(can_undo && dirty);
            annots[0].id
        }
        other => panic!("{other:?}"),
    };
    w.send(Request::Undo);
    assert!(matches!(recv(&w), Response::AnnotsChanged { ref annots, can_redo: true, .. } if annots.is_empty()));
    w.send(Request::Annot(AnnotOp::Delete { page: 0, id: id + 1000 }));
    assert!(matches!(recv(&w), Response::AnnotFailed(_)));
}
```

(Atualizar os testes existentes que usam `Save { pending, dest }`: enviar antes `Request::Annot(Create(..))`.)

- [ ] **Passo 2–5:** ver falhar, implementar (`with_doc_mut` para operações), testes verdes, commit `feat(pdf-core): operações de anotação, desfazer e refazer pelo worker`.

---

### Tarefa 4: `app-state` — geometria de seleção

**Files:** Create `crates/app-state/src/annot_geom.rs`; `lib.rs`.

**Produces:**
- `pub fn annot_hit(rects: &[(i32, PdfRect)], p: (f32, f32), tol: f32) -> Option<i32>` — id da **última** (de cima) cujo retângulo, ampliado por `tol`, contém o ponto.
- `pub enum Handle { TopLeft, TopRight, BottomLeft, BottomRight }`; `pub fn handle_at(r: PdfRect, p: (f32, f32), size: f32) -> Option<Handle>`.
- `pub fn resized(r: PdfRect, h: Handle, to: (f32, f32), min: f32) -> PdfRect` — novo retângulo arrastando a alça `h` até `to`, sem inverter e com tamanho mínimo.

- [ ] **Passo 1: testes**

```rust
use super::*;
use pdf_core::PdfRect;

#[test]
fn hit_picks_the_topmost_and_respects_tolerance() {
    let rects = [(1, PdfRect::new(0.0, 0.0, 100.0, 100.0)), (2, PdfRect::new(50.0, 50.0, 150.0, 150.0))];
    assert_eq!(annot_hit(&rects, (60.0, 60.0), 0.0), Some(2), "a de cima ganha");
    assert_eq!(annot_hit(&rects, (10.0, 10.0), 0.0), Some(1));
    assert_eq!(annot_hit(&rects, (152.0, 152.0), 3.0), Some(2), "tolerância");
    assert_eq!(annot_hit(&rects, (300.0, 300.0), 3.0), None);
}

#[test]
fn handles_are_found_at_the_corners() {
    let r = PdfRect::new(10.0, 10.0, 110.0, 60.0);
    assert_eq!(handle_at(r, (11.0, 9.0), 4.0), Some(Handle::TopLeft));
    assert_eq!(handle_at(r, (108.0, 62.0), 4.0), Some(Handle::BottomRight));
    assert_eq!(handle_at(r, (60.0, 35.0), 4.0), None);
}

#[test]
fn resizing_never_flips_or_collapses() {
    let r = PdfRect::new(10.0, 10.0, 110.0, 60.0);
    assert_eq!(resized(r, Handle::BottomRight, (210.0, 160.0), 5.0), PdfRect::new(10.0, 10.0, 210.0, 160.0));
    let tiny = resized(r, Handle::BottomRight, (0.0, 0.0), 5.0);
    assert_eq!((tiny.width(), tiny.height()), (5.0, 5.0), "não inverte: fica no mínimo");
    assert_eq!(resized(r, Handle::TopLeft, (0.0, 0.0), 5.0), PdfRect::new(0.0, 0.0, 110.0, 60.0));
}
```

- [ ] **Passo 2–5:** ver falhar, implementar, verde, commit `feat(app-state): geometria de seleção e alças de anotação`.

---

### Tarefa 5: `gera-ui` — trocar para o modelo novo (paridade de comportamento)

**Files:** `edit.rs`, `doc_tab.rs`, `view.rs`, `sidebar.rs`, `app.rs`, `shell/toolbar.rs`; remover `crates/app-state/src/annots.rs` (`PendingAnnots`).

Objetivo: tudo que já existe passa a funcionar pelo modelo novo, sem a edição nova ainda:
- Criar (destacar/sublinhar/riscar pela seleção, nota, desenho) envia `Request::Annot(Create(..))`; a página volta re-renderizada pelo MuPDF. `EditState` perde `pending`; ganha `saving` (continua) e nada de `paint_pending` (só o traço ao vivo do desenho e a seleção de texto).
- `DocTab`: campos `annots: Vec<AnnotInfo>` (no lugar de `saved_annots`), `dirty`, `can_undo`, `can_redo`; `is_dirty()` = `dirty`; `run(Undo/Redo)` envia `Request::Undo/Redo`; `save()` envia `Save { dest }` quando `dirty`; `save_as` envia sempre; `ExportForPrint { dest, with_annots }`.
- `poll`: `AnnotsChanged` → atualiza campos e chama `view.invalidate_page(p)` para cada página; `AnnotFailed(msg)` → status; `Saved { annots }` → `annots`, `dirty = false`, `can_undo = can_redo = false`, `view.clear_cache()`.
- `view.rs`: `pub fn invalidate_page(&mut self, page: i32)` — cancela pedidos e remove do cache só as chaves daquela página.
- Sidebar Anotações: lista `annots` (rótulos por `AnnotKind`, inclusive `Other` → "Anotação").
- Linha Comentar: desfazer/refazer ativos por `can_undo/can_redo`.
- Barra de status: "N alterações não salvas" vira "alterações não salvas" quando `dirty`.

- [ ] **Passo 1: teste** (em `view.rs`):

```rust
#[test]
fn invalidate_page_drops_only_that_page() {
    let ctx = egui::Context::default();
    let worker = Worker::spawn(|| {});
    let mut v = view();
    let img = RenderedPage { width: 1, height: 1, rgb: vec![255, 255, 255] };
    for p in [0, 1] {
        v.request_at(&worker, p, 1.0);
        v.on_rendered(&ctx, p, 1.0, false, Rotation::R0, img.clone());
    }
    v.invalidate_page(0);
    assert!(v.texture_at(0, 1.0).is_none());
    assert!(v.texture_at(1, 1.0).is_some(), "as outras páginas ficam");
}
```

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar a troca; adaptar/remover testes do `EditState` que dependiam de `pending` (reescrever os de "recusa durante o salvamento" sobre o envio de `Create`, se fizer sentido, ou removê-los com Ruling). **Passo 4:** `cargo test --workspace` verde e smoke (`--bench-open`). **Passo 5:** commit `refactor(gera-ui): anotações pelo modelo editável do pdf-core`.

---

### Tarefa 6: `gera-ui` — selecionar, mover, redimensionar, apagar, propriedades e editar nota

**Files:** `edit.rs`, `doc_tab.rs`, `shell/commands.rs`, Create `shell/annot_props.rs`.

**Produces:** `EditState.selected: Option<(i32, i32)>` (página, id); `DocCommand::Delete` (tecla Delete, fora de campo de texto); janela "Propriedades da anotação"; menu de contexto sobre anotação.

Comportamento (ferramenta Selecionar):
1. **Pressionar** sobre uma anotação (`annot_hit` com tolerância de 3 pt/zoom, usando o ponto de `press_origin`): seleciona. Se começou sobre uma **alça** da selecionada redimensionável → modo redimensionar; se sobre o corpo de uma `movable()` → modo mover; senão (marcação) só seleciona.
2. **Arrastar** em modo mover/redimensionar: desenha a pré-visualização (contorno tracejado azul do retângulo novo); ao **soltar**, envia `Move { dx, dy }` ou `Resize { rect }` (se mudou ≥ 1 pt).
3. Pressionar fora de anotação: tira a seleção de anotação e segue a seleção de texto atual.
4. **Pintura** da selecionada: contorno azul e, se redimensionável, 4 quadradinhos nas alças.
5. **Duplo clique** numa nota: abre a janela de texto com o conteúdo atual → `SetContents`.
6. **Botão direito** sobre uma anotação: menu **Propriedades…** (abre a janela) e **Excluir** (`Delete`). Sobre seleção de texto continua o menu da B1.
7. **Delete** com anotação selecionada → `Delete` e limpa a seleção.
8. **Janela Propriedades** (`annot_props::window`): 8 cores do Acrobat (amarelo `[1,0.9,0]`, vermelho, laranja, verde, azul, roxo, preto, cinza) como quadradinhos clicáveis; opacidade (slider 0–100 %); espessura 1–12 pt só para desenho. Cada mudança envia `Style` (no slider, só quando solta: `drag_stopped` ou `changed && !dragged`).
9. Painel Anotações: clicar seleciona a anotação e leva à página.

- [ ] **Passo 1: testes** (puros, em `annot_props.rs`): paleta com 8 cores distintas; `percent(opacity)` e de volta. (A interação é verificada no E2E da tarefa 7.)
- [ ] **Passo 2–4:** implementar, testes verdes, smoke.
- [ ] **Passo 5:** commit `feat(gera-ui): selecionar, mover, redimensionar, apagar e propriedades de anotações`.

---

### Tarefa 7: Refatoração, E2E, medição e revisão

- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`; verde; commit.
- [ ] **E2E** (Xvfb + openbox + xdotool, pelo executor):
  1. Criar nota → arrastar para outro lugar → conferir posição; Ctrl+Z volta.
  2. Desenhar → selecionar → puxar a alça → maior; Ctrl+Z volta.
  3. Destacar texto → clique direito → Propriedades → azul e 50 % → conferir na tela.
  4. Selecionar anotação → Delete → some; Ctrl+Z volta; salvar → reabrir → conferir.
  5. Abrir PDF com anotação feita por outro programa (o `copia.pdf` do usuário, se tiver; senão um gerado com autor) → apagar → desfazer → igual.
  6. Duplo clique na nota → editar texto → painel Anotações mostra o novo texto.
  7. Fechar com alterações → pergunta.
- [ ] `scripts/measure.sh`; verificação final (fmt, clippy, testes, build, audit, git status).
- [ ] Revisão independente da branch (modelo mais capaz) e uma rodada de correções.
