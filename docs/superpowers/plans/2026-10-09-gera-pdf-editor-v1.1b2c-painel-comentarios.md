# v1.1-B2c — Painel Comentários: plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Painel Comentários do Acrobat (lista com autor/data, respostas, status, visto, filtros, ordenação e busca) gravando respostas e status no formato do Acrobat.

**Architecture:** pdf-core grava respostas/status como anotações Text filhas (`/IRT`, NoView) e datas; app-state monta a árvore, filtra e ordena (puro); gera-ui desenha o painel na direita quando "Comentar" está ligado.

**Tech Stack:** Rust 2021, mupdf 0.8, chrono 0.4 (já no lock), egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1.1b2c-painel-comentarios-design.md`

## Global Constraints

- pt-BR; commits com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; simplify só no fim; E2E pelo executor em Xvfb.
- fmt, clippy `-D warnings` e testes verdes ao fim de cada tarefa.
- Formato compatível com o Acrobat (seção 2 da spec): `/IRT`, `/RT /R`, `/StateModel`, `/State`, NoView.
- Toda operação nova entra no desfazer/refazer (inversa exata) e no Salvar.
- Nunca usar `pkill -f` (mata o próprio shell); usar `pkill -x gera-pdf-editor`.

## Review Focus

1. **Comentário com respostas apagado e desfeito** volta com todas as respostas: `deleting_a_comment_takes_its_replies_and_undo_brings_them_back` (tarefa 1).
2. **Resposta não aparece na página nem é clicável** (mesmo `/Rect` do pai): `replies_are_not_drawn_on_the_page` (tarefa 1) e `hit_ignores_replies` (tarefa 3).
3. **Status vigente** é o mais recente, mesmo com vários autores: `current_state_is_the_latest` (tarefa 2).
4. **Busca sem acento e sem caixa** ("revisao" acha "Revisão"): `search_ignores_accents_and_case` (tarefa 2).
5. **PDF de outro programa com resposta órfã** (`/IRT` para anotação inexistente) não quebra nem aparece como comentário solto: `orphan_replies_are_ignored` (tarefa 2).

---

### Tarefa 1: `pdf-core` — respostas, status, visto, datas e exclusão em cascata

**Files:** Modify `crates/pdf-core/Cargo.toml` (chrono), `src/types.rs`, `src/doc.rs`, `src/annot_edit.rs`; Test `crates/pdf-core/tests/comments.rs`.

**Interfaces (Produces):**
- `pub enum StateModel { Review, Marked }`
- `AnnotInfo.reply_to: Option<i32>`, `AnnotInfo.state: Option<(StateModel, String)>` (nome PDF do estado, ex. "Accepted"), `AnnotInfo.created: String`, `AnnotInfo.modified: String`.
- `AnnotOp::Reply { page: i32, parent: i32, text: String }`, `AnnotOp::SetState { page: i32, parent: i32, model: StateModel, state: String }`.
- `Edit::Contents` passa a guardar `modified: Option<String>` (o `/M` a gravar).

- [ ] **Passo 1: testes** (`tests/comments.rs`):

```rust
mod common;
use common::write_sample;
use pdf_core::{AnnotOp, NewAnnot, PdfDoc, StateModel};

fn doc_with_note() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc, i32) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_author(Some("Ana".into()));
    d.apply(AnnotOp::Create(NewAnnot::Note { page: 0, at: (100.0, 100.0), text: "pai".into() })).unwrap();
    let id = d.annotations(0).unwrap()[0].id;
    (dir, path, d, id)
}

#[test]
fn replies_and_states_are_children_with_authors_and_dates() {
    let (_d, path, mut d, id) = doc_with_note();
    d.apply(AnnotOp::Reply { page: 0, parent: id, text: "concordo".into() }).unwrap();
    d.apply(AnnotOp::SetState { page: 0, parent: id, model: StateModel::Review, state: "Accepted".into() }).unwrap();
    d.save(&path).unwrap();
    let all = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert_eq!(all.len(), 3);
    let reply = all.iter().find(|a| a.contents == "concordo").unwrap();
    assert_eq!(reply.reply_to, Some(id));
    assert_eq!(reply.author, "Ana");
    assert!(reply.created.starts_with("D:20") && reply.modified.starts_with("D:20"));
    let state = all.iter().find(|a| a.state.is_some()).unwrap();
    assert_eq!(state.state, Some((StateModel::Review, "Accepted".into())));
    assert_eq!(state.reply_to, Some(id));
}

#[test]
fn replies_are_not_drawn_on_the_page() {
    let (_d, _p, mut d, id) = doc_with_note();
    let before = d.render(0, 1.0, false).unwrap().rgb;
    d.apply(AnnotOp::Reply { page: 0, parent: id, text: "x".into() }).unwrap();
    assert_eq!(d.render(0, 1.0, false).unwrap().rgb, before);
}

#[test]
fn deleting_a_comment_takes_its_replies_and_undo_brings_them_back() {
    let (_d, _p, mut d, id) = doc_with_note();
    d.apply(AnnotOp::Reply { page: 0, parent: id, text: "r1".into() }).unwrap();
    d.apply(AnnotOp::Reply { page: 0, parent: id, text: "r2".into() }).unwrap();
    let before = d.annotations(0).unwrap();
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap(), before);
}

#[test]
fn editing_text_updates_the_modification_date_and_undo_restores_it() {
    let (_d, _p, mut d, id) = doc_with_note();
    let before = d.annotations(0).unwrap()[0].clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    d.apply(AnnotOp::SetContents { page: 0, id, text: "novo".into() }).unwrap();
    assert_ne!(d.annotations(0).unwrap()[0].modified, before.modified);
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], before);
}
```

- [ ] **Passo 2:** rodar → falha de compilação.
- [ ] **Passo 3: implementar.**
  - `chrono = { version = "0.4.38", default-features = false, features = ["clock"] }` no pdf-core; `fn pdf_now() -> String` = `Local::now().format("D:%Y%m%d%H%M%S")` + fuso `+hh'mm'`.
  - Create (todos os tipos): grava `/CreationDate` e `/M` (`dict_put` com `new_string`).
  - `Reply`: `add_text_annotation` no `/Rect` do pai, `/IRT` = objeto do pai, `/RT /R`, flag `NO_VIEW`, autor, datas; histórico = `Visibility{hidden:true}` (como Create).
  - `SetState`: igual, sem texto, com `/StateModel` e `/State` (nomes PDF), `/Contents` vazio.
  - `annot_info`: `reply_to` = `get_dict("IRT")?.as_indirect()`; `state` de `/StateModel` + `/State`; datas de `/CreationDate` e `/M`.
  - `annotations()`: continua pulando Hidden; inclui NoView (filhas).
  - `Delete` de anotação com filhas: `Edit::Batch` com `Visibility{hidden:true}` do pai e de cada filha visível (`/IRT` = id).
  - `Edit::Contents { modified }`: grava `/M` = `pdf_now()` e a inversa guarda o `/M` antigo.
- [ ] **Passo 4:** `cargo test -p pdf-core` verde.
- [ ] **Passo 5:** commit `feat(pdf-core): respostas, status, visto e datas dos comentários`.

---

### Tarefa 2: `app-state` — árvore de comentários, filtros, ordenação e busca

**Files:** Create `crates/app-state/src/comments.rs`, `crates/app-state/src/review.rs`; Modify `lib.rs`.

**Interfaces (Produces):**
- `review::ReviewState { None, Accepted, Cancelled, Completed, Rejected }` com `pdf_name()`, `from_pdf(&str)`, `label()` ("Nenhum", "Aceito", "Cancelado", "Concluído", "Rejeitado"), `ALL`.
- `comments::Thread { root: AnnotInfo, replies: Vec<AnnotInfo>, review: Option<(ReviewState, String /*autor*/)>, checked: bool }`.
- `comments::threads(annots: &[AnnotInfo]) -> Vec<Thread>` (só comentários; respostas por data; status/visto vigentes = maior `modified`, depois maior id).
- `comments::CommentFilter { text: String, kinds: Vec<AnnotKind>, authors: Vec<String>, statuses: Vec<ReviewState>, checked: Option<bool> }` com `matches(&Thread) -> bool` e `is_active()`.
- `comments::SortBy { Page, Author, Date, Kind }` e `sorted(Vec<Thread>, SortBy) -> Vec<Thread>`.
- `comments::fold(s: &str) -> String` (minúsculas sem acento).

- [ ] **Passo 1: testes** (em `comments.rs`): construir `AnnotInfo` com `..Default::default()`:
  - `replies_go_under_their_parent_sorted_by_date`
  - `orphan_replies_are_ignored`
  - `current_state_is_the_latest` (dois status, autores diferentes, `modified` diferentes → o mais recente; empate de data → maior id)
  - `checked_follows_the_latest_marked_state`
  - `filters_combine` (tipo + status + visto)
  - `search_ignores_accents_and_case` (busca "revisao" acha conteúdo "Revisão"; busca por autor também; texto de resposta também conta)
  - `sort_by_author_and_date`
- [ ] **Passo 2:** rodar → falha.
- [ ] **Passo 3:** implementar (sem dependências novas; `fold` troca acentos do português por letras simples).
- [ ] **Passo 4:** `cargo test -p app-state` verde.
- [ ] **Passo 5:** commit `feat(app-state): árvore, filtros, ordenação e busca de comentários`.

---

### Tarefa 3: `gera-ui` — painel Comentários

**Files:** Create `crates/gera-ui/src/comments_panel.rs`; Modify `app.rs` (painel da direita), `doc_tab.rs`, `edit.rs` (hit-test ignora filhas), `sidebar.rs` (lista da esquerda só comentários), `main.rs`.

**Comportamento:** como a spec, seção 1. Cartão: rótulo do tipo + autor (negrito) + data formatada (`pdf_date::format_pdf_date`) + texto (3 linhas, expandido quando selecionado) + "N respostas" + status. Selecionado: respostas (autor, data, texto) e `TextEdit` "Adicionar resposta…" + Responder (desativado se vazio; Enter com Ctrl envia). Menu ⋯/clique direito: Responder, Definir status ▸, Marcar com visto, Excluir, Propriedades…. Barra: busca, Ordenar ▾, Filtrar ▾ (tipos, autores presentes, status, visto; "Limpar filtros").

- [ ] **Passo 1: testes** (puros):
  - `edit.rs`: `hit_ignores_replies` (anotação com `reply_to` no mesmo retângulo não é escolhida).
  - `comments_panel.rs`: `page_header_counts_comments` ("Página 2 (3)"), `reply_button_needs_text`.
- [ ] **Passo 2–4:** implementar, testes verdes, smoke.
- [ ] **Passo 5:** commit `feat(gera-ui): painel Comentários com respostas, status, filtros e busca`.

---

### Tarefa 4: Refatoração, E2E, medição e revisão

- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`; verde; commit.
- [ ] E2E (Xvfb): Comentar → painel; clicar cartão seleciona e rola; responder; status Aceito; visto; filtrar por status; excluir com respostas e desfazer; salvar, reabrir, conferir.
- [ ] `scripts/measure.sh` (no Xvfb, comparando com a main se a tela real estiver bloqueada); verificação final.
- [ ] Revisão independente (modelo mais capaz) e uma rodada de correções.
