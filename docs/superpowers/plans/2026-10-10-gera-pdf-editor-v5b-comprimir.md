# Comprimir PDF (v5b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5b-comprimir-design.md

## Review Focus
- Imagem compartilhada por várias páginas (trocar uma vez só).
- Imagem gigante (decodificar não pode estourar a memória: limite de pixels).
- PDF com imagens em XObjects de formulário aninhados.
- Imagem 1 bit ou 16 bits por componente.
- Resultado maior que o original (não piorar: manter o original da imagem).

### Task 1: pdf-core compress.rs + testes
### Task 2: worker + gera-ui
### Task 3: E2E, revisão, merge
