# Criar PDF e Exportar PDF (v5c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5c-criar-exportar-design.md

## Review Focus
- Imagem enorme (pixels) ou arquivo que não é imagem; TIFF de várias páginas (só a primeira).
- Nome com caracteres especiais e pasta de destino sem permissão de escrita.
- Exportar documento protegido (com permissão de copiar) e sem ela.
- PDF de 500 páginas para imagens: progresso/espera sem travar a janela.
- Imagem com orientação EXIF.

### Task 1: pdf-core create_export.rs + testes
### Task 2: worker + gera-ui
### Task 3: E2E, revisão, merge
