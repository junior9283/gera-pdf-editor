# Digitalizar e OCR (v5d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5d-ocr-design.md

## Review Focus
- Página girada (/Rotate 90): a camada de texto fica sobre as palavras.
- Página enorme (limitar pixels do render do OCR).
- Caracteres fora do WinAnsi (ç, ã, õ entram; outros viram "?").
- Conteúdo original que deixa o estado gráfico alterado (q/Q em volta).
- Recursos herdados do /Pages.

### Task 1: pdf-core ocr.rs + testes
### Task 2: worker + gera-ui + AppImage
### Task 3: E2E, revisão, merge
