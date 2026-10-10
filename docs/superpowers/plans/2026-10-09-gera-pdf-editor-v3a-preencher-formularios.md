# v3a — Preencher formulários: plano

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** preencher AcroForm como no Acrobat. **Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v3a-preencher-formularios-design.md`

## Global Constraints
- pt-BR; commits com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; simplify no fim; E2E em Xvfb; nunca `pkill -f`.
- Nada grava o arquivo sem Salvar; desfazer exato.

## Review Focus
1. Botão de opção: escolher um desmarca o outro e o valor do grupo é o estado do escolhido: `radio_buttons_pick_one`.
2. Valor com acentos e quebra de linha em campo de várias linhas sobrevive a salvar/reabrir: `text_values_round_trip`.
3. Campo somente leitura não muda: `readonly_fields_refuse_values`.
4. Desfazer depois de salvar não existe (histórico zera) e desfazer antes volta o valor: `field_undo_restores_the_previous_value`.
5. Widgets não viram anotações (lista/seleção/apagar): `widgets_are_not_annotations`.

### Tarefa 1: pdf-core — campos (tests/forms.rs)
### Tarefa 2: gera-ui — realce, edição, barra roxa, Tab
### Tarefa 3: simplify, E2E, medição, revisão, correções, merge, AppImage
