# Gera PDF Editor — Subprojeto 1.1-B2c: painel Comentários

Data: 2026-10-09 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

O **painel Comentários** do Acrobat Pro: com a ferramenta Comentar aberta, a coluna da direita lista
todos os comentários do documento, com autor, data e texto; permite **responder** (conversa),
**definir status** de revisão, **marcar com visto**, **filtrar**, **ordenar** e **buscar**. As
respostas e os status ficam gravados no PDF do jeito do Acrobat (anotações `/IRT`), para que o
Acrobat e outros leitores os mostrem.

### Critérios de sucesso

- Com "Comentar" ligado, o painel da direita mostra os comentários agrupados por página
  ("Página 2 (3)"), cada um com ícone/tipo, **autor**, **data e hora**, texto e a contagem de
  respostas; clicar seleciona a anotação na página e rola até ela (e vice-versa: selecionar na página
  destaca o cartão).
- Cartão selecionado mostra as respostas (autor, data, texto) e um campo **"Adicionar resposta…"**
  com o botão **Responder**; a resposta vira uma anotação de resposta no PDF (Acrobat a mostra na
  conversa).
- Menu do cartão (botão ⋯ e clique direito): **Responder**, **Definir status ▸ Nenhum / Aceito /
  Cancelado / Concluído / Rejeitado**, **Marcar com visto**, **Excluir**, **Propriedades…**.
  O status aparece no cartão ("Aceito — Ana Lúcia").
- Barra do painel: **busca** (texto, autor), **Ordenar por** página / autor / data / tipo, **Filtrar**
  por tipo, autor, status e visto (os filtros se combinam; "Limpar filtros").
- Excluir um comentário exclui também as respostas e status dele (como no Acrobat); desfazer traz tudo
  de volta.
- Respostas, status e visto entram no desfazer/refazer e no Salvar da B2a.
- Datas: criação e modificação gravadas (`/CreationDate`, `/M`) ao criar e ao mudar o texto; mostradas
  como "09/10/2026 21:08".

### Fora de escopo

Importar/exportar comentários (FDF/XFDF), resumo de comentários para impressão, comentários em
formulários, notificações de revisão compartilhada.

## 2. Formato no PDF (compatível com o Acrobat)

- **Resposta:** anotação Text com `/IRT` → comentário pai, `/RT /R`, `/T` autor, `/Contents`,
  `/M` e `/CreationDate`, mesmo `/Rect` do pai e flag **NoView** (não aparece na página).
- **Status de revisão:** anotação Text com `/IRT` → pai, `/StateModel /Review`, `/State /Accepted |
  /Cancelled | /Completed | /Rejected | /None`, `/T` autor, NoView. O status vigente é o do último
  status (maior `/M`, depois maior id).
- **Visto:** mesmo esquema com `/StateModel /Marked`, `/State /Marked | /Unmarked`.
- Respostas, status e vistos **não** aparecem na lista como comentários próprios; o hit-test da página
  os ignora.

## 3. Arquitetura

### pdf-core

- `AnnotInfo` ganha `reply_to: Option<i32>` (id do pai), `state: Option<(StateModel, String)>`,
  `created: String`, `modified: String` (datas cruas `D:…`).
- `AnnotOp::Reply { page, parent, text }`, `AnnotOp::SetState { page, parent, model, state }`:
  criam a anotação filha (desfazer = esconder, como o Create).
- `AnnotOp::Delete` de um comentário com filhas: `Edit::Batch` escondendo pai e filhas.
- Datas: `chrono` gera `D:AAAAMMDDhhmmss±hh'mm'`; Create/Reply/SetState gravam `/CreationDate` e `/M`;
  `SetContents` grava `/M` (a inversa restaura o `/M` anterior).

### app-state

- `comments.rs` (puro e testado): monta a árvore a partir de `Vec<AnnotInfo>` — comentários
  (sem `reply_to`) com respostas ordenadas por data, status e visto vigentes; filtros
  (`CommentFilter { kinds, authors, statuses, checked, text }`), ordenação (`SortBy`), agrupamento por
  página e contagens.
- `review.rs`: `ReviewState` (Nenhum, Aceito, Cancelado, Concluído, Rejeitado) ↔ nomes PDF, rótulos.

### gera-ui

- `comments_panel.rs`: painel da direita quando "Comentar" está ligado (o hub de Ferramentas volta
  quando desligado); cartões, menu ⋯, campo de resposta, barra de busca/ordenação/filtros.
- Seleção sincronizada com `EditState.selected`; clicar no cartão rola até a anotação.

## 4. Erros

- Falha ao responder/definir status: `AnnotFailed`, status informa, nada muda.
- Resposta vazia: botão Responder desativado.
- Comentário pai apagado: as filhas somem junto (Batch); se só a filha falhar, nada muda.

## 5. Testes

- **pdf-core:** responder (IRT, RT, NoView, autor, datas, não renderiza na página), status e visto,
  excluir pai leva filhas e desfazer devolve tudo, `SetContents` atualiza `/M` e desfazer restaura,
  salvar/reabrir mantém a conversa.
- **app-state:** árvore (respostas sob o pai, órfãs ignoradas, status vigente = o mais recente),
  filtros combinados, ordenações, busca sem acento/maiúsculas ("revisao" acha "Revisão").
- **E2E (Xvfb):** abrir Comentar → painel lista; clicar cartão seleciona e rola; responder; definir
  status Aceito; marcar visto; filtrar por status; excluir com respostas e desfazer; salvar, reabrir e
  conferir.
