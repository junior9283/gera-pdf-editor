# Preencher e assinar (v3b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Ferramenta Preencher e assinar do Acrobat: texto livre, marcas e assinatura/iniciais vetoriais ou de imagem.

**Architecture:** pdf-core grava (FreeText typewriter, Ink, Circle, Stamp com form XObject vetorial ou imagem); app-state tem a geometria pura e o arquivo de assinaturas; gera-ui tem a barra, a colocação e a janela de assinatura.

**Tech Stack:** Rust, mupdf 0.8, egui/eframe 0.36, serde_json.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v3b-preencher-assinar-design.md

## Global Constraints

- Textos da interface em pt-BR; comportamento igual ao Acrobat.
- Nenhum recurso da Adobe; fonte Great Vibes (OFL) com licença no repositório.
- Binário < 25 MB; memória < 150 MB.
- Toda criação passa pelo histórico (desfazer/refazer) e pelo worker.

## Review Focus

- Página girada (/Rotate 90/270): assinatura e marcas em pé na tela.
- Imagem de assinatura inválida, enorme (> 20 MB) ou com transparência: erro claro / alfa preservado.
- Texto digitado com caracteres sem glifo na fonte (ex.: emoji, CJK): ignorados sem travar.
- Arquivo de assinaturas corrompido ou imagem guardada apagada: tratado como "sem assinatura".
- Assinatura desenhada vazia (sem traços) ou um ponto só: "Aplicar" desabilitado.

---

### Task 1: pdf-core — caminhos vetoriais e Stamp de assinatura

**Files:** Create `crates/pdf-core/src/signature.rs`; modify `types.rs`, `doc.rs`, `lib.rs`; test `crates/pdf-core/tests/fill_sign.rs`.

**Interfaces — Produces:** `PathCmd`, `VectorPath`, `SignatureArt`, `NewAnnot::Signature { page, rect, art, initials }`, `signature::typed(text) -> Result<(Vec<VectorPath>, f32)>`.

- [ ] Teste: criar assinatura vetorial (um traço em Z) numa página, salvar, reabrir: anotação Stamp, `/Name /GeraSignature`, renderizar a página e achar pixels escuros dentro do retângulo; página girada 90° também.
- [ ] Teste: `typed("Ana")` devolve caminhos não vazios e `aspect("Ana Maria") > aspect("Ana")`; `typed("😀")` não falha.
- [ ] Teste: assinatura de imagem PNG; arquivo inválido devolve erro.
- [ ] Implementar e passar.

### Task 2: pdf-core — texto do Preencher e assinar e ponto

**Files:** `types.rs`, `doc.rs`; test `tests/fill_sign.rs`.

- [ ] Teste: `NewAnnot::FillText` cria FreeText sem borda (`/BS /W 0`), `/IT /FreeTextTypeWriter`, tamanho pedido.
- [ ] Teste: `NewAnnot::Dot` cria Circle com `/IC` preto.
- [ ] Implementar e passar.

### Task 3: app-state — geometria das marcas, suavização e assinaturas guardadas

**Files:** Create `crates/app-state/src/fill_sign.rs`.

- [ ] Testes: `mark_strokes(Cross|Check|Line, rect)` dentro do retângulo; `smooth` mantém extremos; `normalize` leva a 0..1 com proporção; `SavedSignatures` ida e volta; JSON corrompido = vazio.
- [ ] Implementar e passar.

### Task 4: gera-ui — barra, colocação, janela de assinatura, setas

**Files:** Create `crates/gera-ui/src/fill_sign.rs`, `crates/gera-ui/src/shell/signature_dialog.rs`; modify `app.rs`, `doc_tab.rs`, `edit.rs`, `tools_catalog.rs`.

- [ ] Teste puro: setas → deslocamento (1 pt, Shift 10 pt) no espaço da página considerando a rotação.
- [ ] Barra Preencher e assinar; prévia no cursor; clique coloca; A−/A+.
- [ ] Janela com Digitar/Desenhar/Imagem; Aplicar guarda e entra em colocar.
- [ ] Campo de assinatura clicado coloca a assinatura ajustada.

### Task 5: E2E, simplify, medição, revisão, merge
