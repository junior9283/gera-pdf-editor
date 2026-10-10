# Gera PDF Editor — Subprojeto 5e: Redigir

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Redigir** do Acrobat Pro: marcar informações sensíveis e apagá-las de vez do arquivo.

## 2. Funcionalidades

- **Marcar texto** (arrastar sobre o texto, como o Destacar) e **Marcar área** (retângulo): cria
  marcas de redação (anotações `/Redact`, como o Acrobat), visíveis, selecionáveis, apagáveis e com
  desfazer, antes de aplicar.
- **Localizar texto e redigir**: frase (sem diferenciar maiúsculas) ou padrões — CPF, CNPJ,
  telefone, e-mail, cartão de crédito, CEP e data; resultados com página e trecho, cada um com
  caixa de seleção; "Marcar selecionados" cria as marcas.
- **Propriedades**: cor da caixa, texto sobreposto (ex.: "SIGILOSO") e cor do texto.
- **Aplicar**: confirmação (é permanente); em todas as páginas com marcas: o texto sob a marca sai,
  os pixels das imagens sob a marca são apagados, os desenhos cobertos saem; a caixa é pintada com a
  cor e o texto sobreposto. Links sob as marcas também saem.
- **Limpar documento** (Remover informações ocultas): metadados (Info e XMP), anexos, scripts e
  ações, comentários — cada um com caixa de seleção.
- Recusa em PDF assinado ou sem permissão de alterar; desfazer por instantâneo no Aplicar e no
  Limpar.

## 3. Testes

- Marcar e aplicar: o texto sob a marca some de `page_text` e da busca; o vizinho fica; a caixa
  aparece pintada; imagem sob a marca perde os pixels; desfazer volta.
- Localizar: frase e cada padrão acham os trechos certos (e não acham os errados).
- Limpar: metadados, anexos, scripts e comentários saem.
