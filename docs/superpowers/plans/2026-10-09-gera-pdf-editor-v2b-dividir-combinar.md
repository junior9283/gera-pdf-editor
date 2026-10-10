# v2b — Dividir PDF e Combinar arquivos: plano

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Dividir PDF e Combinar arquivos como no Acrobat. **Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v2b-dividir-combinar-design.md`

## Global Constraints
- pt-BR; commits com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; simplify no fim; E2E em Xvfb; nunca `pkill -f`.
- Gravação atômica; nunca sobrescrever arquivo existente nem o documento aberto.

## Review Focus
1. Página única maior que o limite de tamanho vira uma parte sozinha (não laço infinito): `split_by_size_puts_a_big_page_alone`.
2. Marcadores de primeiro nível fora de ordem ou apontando para a mesma página: `split_by_bookmarks_handles_unsorted_and_repeated`.
3. Combinar arquivo protegido sem senha falha sem gravar nada: `combine_needs_passwords`.
4. Título de marcador com "/" ou ":" vira nome de arquivo válido: `bookmark_titles_become_safe_file_names`.
5. Combinar o mesmo arquivo duas vezes funciona (dois blocos de marcadores): `combine_same_file_twice`.

### Tarefa 1: pdf-core — `split` e `combine` (testes em `tests/split_combine.rs`)
### Tarefa 2: gera-ui — diálogo Dividir, aba Combinar arquivos, hub
### Tarefa 3: simplify, E2E, medição, revisão independente, correções, merge, AppImage
