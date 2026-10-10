# Proteger PDF (v5a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Goal:** Proteger com senha/permissões, remover segurança e respeitar permissões.
**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5a-proteger-design.md

## Review Focus
- Senha com acentos/emoji (UTF-8 vs PDFDocEncoding no AES-256): reabrir funciona.
- Proteger e depois "Salvar como" para outro arquivo; salvar duas vezes.
- Desfazer depois de proteger (instantâneo) não perde a proteção pendente.
- Permissões gravadas como o Acrobat lê (bits 3,4,5,6,9,11,12 e reservados ligados).
- PDF já protegido aberto com a senha de abrir: Salvar mantém a proteção (Keep).

### Task 1: pdf-core security.rs + testes
### Task 2: worker/summary
### Task 3: gera-ui janela, linha, bloqueios
### Task 4: E2E, simplify, revisão, merge
