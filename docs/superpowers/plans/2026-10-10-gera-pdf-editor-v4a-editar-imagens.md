# Editar imagens (v4a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans.

**Spec:** docs/superpowers/specs/2026-10-10-gera-pdf-editor-v4-editar-design.md (seção 2)

## Review Focus
- Conteúdo em vários fluxos (array /Contents) e comprimidos; nome de XObject com caracteres #xx.
- Página girada: mover/redimensionar no espaço da tela.
- Imagem com /SMask, imagem mascarada, imagem dentro de formulário (recusa).
- `Do` dentro de string literal ou comentário (falso positivo do tokenizador).
- Recursos herdados de /Pages.

### Task 1: pdf-core edit_images.rs + testes
### Task 2: worker + gera-ui (modo Editar PDF)
### Task 3: E2E, revisão, merge
