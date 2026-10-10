# Redigir (v5e) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ferramenta Redigir do Acrobat: marcar (texto, área, localizar por frase ou padrão), aplicar
de forma permanente e limpar informações ocultas.

**Architecture:** marcas = anotações `/Redact` criadas pelo caminho das anotações (`NewAnnot::Redact`,
desfazer, seleção e exclusão já existentes). `pdf-core/src/redact.rs`: localizar (busca do MuPDF e
regex sobre o texto estruturado), aplicar (`apply_redactions_with_options` + caixas pintadas no
conteúdo) e limpar. UI: `Tool::RedactText`/`Tool::RedactArea` no `EditState`, linha da ferramenta e
janelas (Localizar, Propriedades, Aplicar, Limpar) em `gera-ui/src/redact.rs`.

**Tech Stack:** Rust, mupdf 0.8, regex 1, egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-10-gera-pdf-editor-v5e-redigir-design.md`

## Global Constraints

- Comportamento e nomes como no Acrobat Pro; textos em pt-BR.
- Recusar em PDF assinado ou sem permissão; desfazer por instantâneo.
- Aplicar é permanente: confirmação antes.

## Review Focus

1. Texto sob a marca sai de verdade (busca, cópia, extração) — também em página girada.
2. Imagem parcialmente coberta: só a parte sob a marca perde os pixels; desenhos só os cobertos.
3. Padrões: falsos positivos óbvios (números de página, valores) e formatos com/sem pontuação.
4. Marcas de outro programa (/Redact já no arquivo) são aplicadas também.
5. Limpar não quebra o documento (formulários, links internos continuam).

---

### Task 1: pdf-core — marcas, localizar, aplicar, limpar
- [ ] RED: `tests/redact.rs` (aplicar tira o texto e pinta; imagem perde pixels; desfazer; localizar
  frase e padrões; limpar metadados/anexos/scripts/comentários; recusa em assinado).
- [ ] Implementar; worker (`FindRedactions`, `ApplyRedactions`, `Sanitize`); GREEN; commit.

### Task 2: UI — ferramentas, linha e janelas
- [ ] RED: lógica das janelas (padrões escolhidos → consulta; resultados marcados → marcas por
  página; propriedades).
- [ ] Implementar; GREEN; E2E; commit.
