# Gera PDF Editor — Subprojeto 3c: Preparar formulário

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Preparar formulário** do Acrobat: criar e editar campos de formulário (AcroForm) num
PDF, que depois são preenchidos pela v3a em qualquer leitor.

### Critérios de sucesso

- Central de Ferramentas: "Preparar formulário" pronta; abre a linha da ferramenta (uma por vez, como
  Comentar e Preencher e assinar). No modo Preparar, os campos aparecem com contorno e nome, não são
  preenchíveis, e a barra roxa some.
- Linha: **Selecionar**, **Campo de texto**, **Caixa de seleção**, **Botão de opção**, **Lista
  suspensa**, **Caixa de listagem**, **Campo de assinatura**, desfazer/refazer.
- Clique coloca o campo no tamanho padrão do tipo (texto 150×22 pt, caixa/opção 14×14, listas 150×22 /
  150×60, assinatura 180×40); arrastar define o tamanho. Nome automático único por tipo
  ("Texto1", "Caixa1", "Grupo1", "Lista suspensa1", "Lista1", "Assinatura1").
- **Botão de opção**: enquanto a ferramenta continua ativa, os botões colocados entram no mesmo grupo,
  com valores "Opção1", "Opção2"… (o "Adicionar botão" do Acrobat); trocar de ferramenta começa um grupo
  novo.
- **Selecionar**: clique seleciona (contorno azul e alças), arrastar move, alças redimensionam,
  Delete exclui, setas movem (1 pt; Shift 10 pt), duplo clique abre **Propriedades**.
- **Propriedades do campo**: nome, dica, obrigatório, somente leitura; texto: várias linhas, senha,
  limite de caracteres, valor padrão, tamanho da fonte (0 = automático); listas: opções (rótulo e
  valor de exportação; adicionar, remover, subir, descer), "aceitar texto digitado" (lista suspensa);
  caixa/opção: valor de exportação e marcada por padrão; botão de opção: nome do grupo (trocar move
  para outro grupo, existente ou novo). Nome repetido (fora do grupo de opção) é recusado com aviso.
- Tudo com desfazer/refazer e Salvar; o arquivo gerado é lido e preenchido pela v3a (e pelo Acrobat):
  `/AcroForm /Fields`, `/DA`, `/DR` com Helvetica e ZapfDingbats, aparências geradas.

### Fora de escopo

Detecção automática de campos, campos de data com formatação JavaScript, botões com ações, cálculos,
ordem de tabulação manual (a v3a segue a ordem de criação, que é a de /Annots), campos que aparecem em
várias páginas.

## 2. Arquitetura

- **pdf-core** `prepare.rs`: `NewFieldKind`, `FieldProps`, `PdfDoc::add_field(page, rect, kind) ->
  id`, `field_props(page, id)`, `set_field_props(page, id, props)`, `move_field(page, id, rect)`,
  `delete_field(page, id)`; todos com instantâneo no histórico (`Step::Snapshot`), como Limpar
  formulário. Widget criado com `create_annotation(Widget)`, dicionário do campo, `/AcroForm` (cria se
  faltar) e `update()` para a aparência; caixa e opção ganham `/AP /N` com o estado ligado e "Off"
  desenhados por nós (ZapfDingbats ✓ / ●) para ficarem corretos em qualquer leitor.
- **worker**: `Request::Prepare(PrepareOp)` → `Response::PagesChanged` (resumo completo: campos e
  anotações).
- **app-state** `prepare.rs`: tamanhos padrão, nomes automáticos (`next_name`), validação de nome.
- **gera-ui** `prepare.rs` (contornos, seleção, colocação, mover/redimensionar) e
  `shell/field_props.rs` (janela de propriedades).

## 3. Testes

- pdf-core: criar cada tipo, salvar, reabrir e ler pela API da v3a (`fields`) com tipo e nome; o campo
  criado aceita `set_field`; grupo de opção com dois botões funciona como grupo (v3a); propriedades ida
  e volta; nome repetido recusado; mover, excluir e desfazer; PDF sem `/AcroForm` ganha um.
- app-state: nomes automáticos únicos; tamanho padrão; clique vs arrasto.
- E2E: criar texto, caixa e duas opções, salvar, reabrir, preencher (v3a) e salvar de novo.
