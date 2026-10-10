# Gera PDF Editor — Subprojeto 1.1-B2b: caixa de texto, formas, linhas e carimbos

Data: 2026-10-09 · Status: aguardando revisão

## 1. Objetivo

Completar as ferramentas de desenho e texto da linha **Comentar** do Acrobat Pro DC, no modelo
editável da v1.1-B2a: **caixa de texto, retângulo, elipse, linha, seta e carimbos** (padrão comercial
e dinâmicos). Respostas a comentários e o painel Comentários ficam para a **v1.1-B2c**.

### Critérios de sucesso

- Cada ferramenta nova cria a anotação como no Acrobat (tabela da seção 2); o que aparece na tela
  é o que vai para o arquivo (aparência gerada pelo MuPDF).
- Depois de criadas, todas se selecionam, movem, redimensionam (formas, caixa e carimbo pelos cantos;
  linha e seta pelas duas pontas), apagam, mudam de propriedades e entram no desfazer/refazer
  (Ctrl+Z / Ctrl+Shift+Z) e no Salvar da v1.1-B2a.
- Duplo clique numa caixa de texto edita o texto **sobre a página**.
- Carimbos dinâmicos mostram título, nome, data e hora; o nome é pedido uma vez (diálogo
  "Identidade") e fica nas preferências.
- Toda anotação criada leva o nome do autor (quando definido).
- Shift restringe: retângulo → quadrado, elipse → círculo, linha/seta → múltiplos de 45°.
- **Esc** volta para a ferramenta Selecionar.
- Metas da v1 continuam (abertura < 300 ms, memória < 150 MB, binário < 25 MB).

### Fora de escopo

- v1.1-B2c: painel Comentários, respostas, status, filtro, exibição do autor/data.
- Nuvem, polígono, linha conectada, lápis-borracha, carimbos de imagem do usuário, anexos, áudio.
- Fontes além de Helvetica; texto rico (negrito/itálico parcial) na caixa de texto.

## 2. Comportamento (copiado do Acrobat)

| Ferramenta | Criar | Padrão | Editar depois |
|---|---|---|---|
| **Caixa de texto** | Clique cria caixa de 200×24 pt com o cursor; arrastar define o retângulo. Digitar; clicar fora (ou Esc) termina. Vazia ao terminar → descartada. | Texto preto, Helvetica 12 pt, borda vermelha 1 pt, sem preenchimento | Duplo clique edita sobre a página; mover; 4 alças |
| **Retângulo** / **Elipse** | Arrastar de canto a canto (Shift: quadrado/círculo). Arrasto menor que 3 pt não cria. | Linha vermelha 1 pt, sem preenchimento | Mover; 4 alças |
| **Linha** / **Seta** | Arrastar do início ao fim (Shift: 45°). Menor que 3 pt não cria. Seta: ponta aberta no fim. | Vermelha, 1 pt | Mover; alça em cada ponta |
| **Carimbo ▾** | Menu: **Dinâmicos** (APROVADO, REVISADO, RECEBIDO) e **Padrão comercial** (APROVADO, NÃO APROVADO, CONFIDENCIAL, RASCUNHO, FINAL, VENCIDO, PARA COMENTÁRIO, ULTRASSECRETO, CONFORME, VENDIDO, DEPARTAMENTAL, EXPERIMENTAL, DIVULGAÇÃO PÚBLICA, NÃO DIVULGAR). Escolhido o carimbo, clique na página coloca-o centrado no ponto. | Largura 190 pt, proporção do catálogo; cor do catálogo | Mover; 4 alças (proporção livre) |

- A ferramenta continua ativa depois de usada (como Nota e Desenhar hoje); **Esc** volta para
  Selecionar (fora de campos de texto).
- **Propriedades** (janela da B2a, por tipo):
  - forma/linha/seta: cor da linha, **preenchimento** (Nenhum + 8 cores; só forma), espessura
    1–12 pt, opacidade;
  - caixa de texto: cor do texto, tamanho da fonte (8–72 pt), cor da borda, preenchimento, opacidade;
  - carimbo: opacidade.
- **Carimbo dinâmico**: segunda linha "Maria Silva, 09/10/2026 14:30" (hora local no momento
  de carimbar). Se o nome não estiver definido, abre "Identidade" (campo Nome · OK · Cancelar);
  Cancelar não cria o carimbo.
- Cores do catálogo: verde `[0.13,0.55,0.13]` (APROVADO, CONFORME, FINAL, RECEBIDO, REVISADO),
  vermelho `[0.8,0.1,0.1]` (NÃO APROVADO, CONFIDENCIAL, VENCIDO, ULTRASSECRETO, NÃO DIVULGAR),
  azul `[0.1,0.3,0.75]` (demais).

## 3. Arquitetura

### pdf-core

- `NewAnnot` ganha:
  - `TextBox { page, rect, text, size, color }`
  - `Shape { page, rect, ellipse: bool }`
  - `Line { page, a, b, arrow: bool }`
  - `Stamp { page, rect, title, subtitle: Option<String>, color }`
- `AnnotKind` ganha `TextBox`, `Rect`, `Ellipse`, `Line`, `Stamp`. `AnnotInfo` ganha `fill:
  Option<[f32;3]>`, `line: Option<((f32,f32),(f32,f32))>`, `font: Option<(f32, [f32;3])>`
  (tamanho e cor do texto), `author: String`. `width` passa a existir para formas, linhas e caixa.
- `AnnotOp` ganha `MoveLinePoint { page, id, end: LineEnd, to }`, `Fill { page, id, color:
  Option<[f32;3]> }`, `Font { page, id, size, color }`; `Move` de linha desloca os dois pontos.
- `Edit` ganha `LinePoints`, `Fill` (com `ColorEdit`, inclusive "sem preenchimento") e `Font`;
  cada um devolve a inversa exata, como na B2a.
- **Carimbo como imagem** (`stamp.rs`): monta um PDF de 1 página em memória com a moldura
  arredondada (2 pt) e o texto em Times-Bold com codificação WinAnsi (acentos), título em cima e
  subtítulo menor embaixo, centralizados e ajustados à largura; rasteriza com fundo transparente
  a 300 dpi; grava como `AP/N/Resources/XObject/I` do Stamp (`/Name` = título). O MuPDF gera a
  aparência a partir da imagem e a reajusta em mover/redimensionar.
- **Autor**: `PdfDoc::set_author(Option<String>)`; `Create` grava `/T` quando definido.
- Caixa de texto: `/DA` com Helvetica, tamanho e cor; borda via `/BS /W` e `/C`; preenchimento via
  interior color.

### app-state

- `Tool` ganha `TextBox`, `Rect`, `Ellipse`, `Line`, `Arrow`, `Stamp(StampKind)`.
- `annot_geom`: `rect_from_drag(a, b, square)`, `snap_45(a, b)`, `dist_to_segment(p, a, b)`,
  `line_handle_at(a, b, p, size) -> Option<LineEnd>`.
- `stamps.rs`: catálogo (`StampKind`, título, cor, proporção, `dynamic`), `subtitle(name, now)`.
- `Prefs.author: Option<String>`.

### gera-ui

- Linha Comentar: botões novos e o menu do Carimbo.
- Gestos de criação com pré-visualização (contorno tracejado/linha) e envio ao soltar.
- Edição da caixa de texto sobre a página (`TextEdit` posicionado pela `PageMap`).
- Propriedades por tipo; diálogo "Identidade"; Esc → Selecionar; seleção e alças de linha.

## 4. Erros

- Falha ao criar/editar: `AnnotFailed`, nada muda, status mostra o motivo (como na B2a).
- Falha ao gerar a imagem do carimbo: o carimbo não é criado e o status informa.
- Identidade cancelada: o carimbo dinâmico não é criado.
- Caixa de texto vazia ao terminar: descartada sem aviso.

## 5. Testes

- **pdf-core**: criar cada tipo, salvar e reler (tipo, retângulo, cor, preenchimento, fonte, autor,
  pontos da linha, seta); mover/redimensionar/estilo/preenchimento/fonte/pontos com desfazer
  devolvendo o estado idêntico; carimbo sobrevive a mover e salvar e a renderização mostra
  pixels da cor do carimbo dentro do retângulo; texto acentuado no carimbo.
- **app-state**: `rect_from_drag` (com e sem Shift, qualquer direção), `snap_45`,
  `dist_to_segment`, `line_handle_at`, catálogo (títulos únicos, cores) e `subtitle`.
- **E2E (Xvfb, pelo Claude)**: criar cada ferramenta e conferir; editar caixa de texto sobre a
  página; puxar a ponta da seta; preencher retângulo; carimbo dinâmico pedindo o nome; salvar,
  reabrir e conferir; Esc volta a Selecionar.
