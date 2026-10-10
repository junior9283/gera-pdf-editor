# Preparar formulário (v3c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Criar e editar campos AcroForm como o Preparar formulário do Acrobat.

**Architecture:** pdf-core cria/edita widgets (instantâneo no histórico); worker repassa; app-state tem nomes e tamanhos; gera-ui tem a linha, seleção/colocação e a janela de propriedades.

**Tech Stack:** Rust, mupdf 0.8, egui/eframe 0.36.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v3c-preparar-formulario-design.md

## Global Constraints

- pt-BR; comportamento do Acrobat; arquivo legível por outros leitores (aparências gravadas).
- Toda alteração passa pelo histórico e pelo worker.

## Review Focus

- PDF protegido por senha (Salvar mantém a criptografia; campos novos legíveis depois).
- PDF com `/AcroForm` herdado sem `/DR`/`/DA` ou com XFA: campo novo funciona.
- Página girada: campo colocado onde o clique foi, texto em pé.
- Nome com ponto ("a.b") ou vazio: recusado (o ponto cria hierarquia).
- Excluir o último botão de um grupo: o grupo some de `/Fields`.

---

### Task 1: pdf-core — criar campos e propriedades
- [ ] Testes de criação por tipo, ida e volta, grupo de opção, propriedades, nome repetido, mover/excluir/desfazer, AcroForm ausente.
- [ ] Implementar `prepare.rs` e passar.

### Task 2: worker e app-state
- [ ] `Request::Prepare`; `next_name`, tamanhos; testes.

### Task 3: gera-ui
- [ ] Linha Preparar formulário, colocação, seleção, mover/redimensionar, Delete/setas, Propriedades.

### Task 4: E2E, simplify, medição, revisão, merge
