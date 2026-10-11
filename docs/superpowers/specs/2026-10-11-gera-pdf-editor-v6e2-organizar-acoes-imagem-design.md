# v6e2: organizar campos, ações e campo Imagem

Data: 2026-10-11 · Etapa da avaliação para a 1.0 (lacunas F07, F05 e a parte Imagem da F04) ·
Versão alvo 0.20.0

## Objetivo

O Preparar formulário passa a trabalhar com **vários campos de uma vez**, como no Acrobat:
- seleção múltipla;
- alinhar, centralizar e distribuir;
- igualar o tamanho;
- duplicar em outras páginas;
- criar várias cópias;
- copiar e colar;
- guias de alinhamento;
- visualizar.

Os campos ganham a aba **Ações**, e as páginas, as ações de abrir e fechar. O preenchimento executa
essas ações. O novo campo **Imagem** recebe uma figura escolhida ao preencher.

## Fora desta etapa

- **v6e3:**
  - detecção automática de campos (F01);
  - campo Código de barras (o restante da F04).
- **v6e4:**
  - importar e exportar dados (F08);
  - a ação Enviar formulário, que precisa da exportação de dados;
  - nivelar (F09);
  - preenchimento automático (F10).
- **Sem execução** (são gravadas e preservadas, e o aviso de scripts continua):
  - as ações JavaScript, multimídia, 3D e "Importar dados do formulário".

## Comportamento de referência (Acrobat Pro)

### Seleção e organização (F07)

- **Seleção:**
  - `Shift`+clique (ou `Ctrl`+clique) junta um campo à seleção ou o tira dela;
  - arrastar numa área vazia seleciona por retângulo;
  - `Ctrl+A` seleciona os campos da página.
- **Campo âncora:** é o último clicado, com o contorno mais forte. Alinhar e igualar o tamanho
  tomam esse campo como referência.
- **Mover:**
  - arrastar um campo selecionado move todos;
  - as setas movem 1 ponto, e `Shift`+setas, 10 pontos;
  - `Delete` exclui a seleção numa etapa do desfazer.
- **Alinhar** (2 campos ou mais), pela âncora:
  - `L` esquerda, `R` direita, `T` topo e `B` base;
  - `H` "horizontalmente" põe os centros na mesma linha horizontal;
  - `V` "verticalmente" põe os centros na mesma linha vertical.
- **Centralizar na página:**
  - `Shift+H` na horizontal, `Shift+V` na vertical, e "Ambos";
  - com vários campos, o grupo vai inteiro e mantém as distâncias entre eles.
- **Distribuir** (3 campos ou mais): vertical ou horizontalmente. Os centros ficam a distâncias
  iguais entre o primeiro e o último.
- **Mesmo tamanho** que a âncora: altura, largura ou ambos.
- **Duplicar** (`Duplicar…`):
  - o campo é repetido em todas as páginas ou num intervalo, na mesma posição;
  - as cópias são widgets do mesmo campo, então mostram o mesmo valor.
- **Criar várias cópias…:**
  - informa as cópias para baixo e para o lado;
  - pode mudar a largura, a altura e a posição do conjunto;
  - mostra uma prévia.
- **Copiar e colar** (`Ctrl+C`, `Ctrl+V`): o campo colado é um widget do mesmo campo, deslocado.
- **`P`** alterna entre editar e visualizar. Visualizar é preencher sem sair do Preparar.
- **`G`** liga e desliga as guias:
  - ao arrastar, aparecem linhas quando bordas ou centros coincidem com os de outro campo;
  - a 4 pixels de distância, o campo encaixa nessa linha.
- Tudo está no menu de contexto e no menu **Organizar** da barra do Preparar.

### Ações (F05)

- **Aba Ações** nas propriedades de qualquer campo.
- **Gatilhos:** Mouse para cima, Mouse para baixo, Mouse entra, Mouse sai, Em foco e Fora de foco.
- **Ações:**
  - Ir para uma página;
  - Abrir um link da web;
  - Mostrar/ocultar um campo;
  - Redefinir um formulário (todos os campos ou os escolhidos);
  - Executar um item de menu (página seguinte, anterior, primeira, última, imprimir, voltar,
    avançar, tela cheia, localizar);
  - Abrir um arquivo;
  - Executar um JavaScript (gravado e não executado).
- **Lista de ações:** cada gatilho tem a sua, com Adicionar, Editar, Excluir, Subir e Descer.
  Várias ações no mesmo gatilho são encadeadas com /Next, como o Acrobat grava.
- **Ações da página:** "Propriedades da página…" no menu das Miniaturas tem a aba Ações, com os
  gatilhos Abrir página e Fechar página.
- **Ao preencher:**
  - clicar executa Mouse para baixo e Mouse para cima;
  - passar o mouse executa Mouse entra e Mouse sai;
  - focar e desfocar executam Em foco e Fora de foco;
  - abrir e fechar uma página executam as ações dela.
- **Comportamento de cada ação:**
  - link da web: pede confirmação, como os links da página;
  - Abrir um arquivo: abre só PDF, numa aba nova e com confirmação; programas nunca são abertos;
  - Mostrar/ocultar: muda a visibilidade dos campos e marca o documento como alterado.

### Campo Imagem (parte da F04)

- **Ferramenta Imagem** no Preparar. Como no Acrobat, é um botão com:
  - layout "Somente ícone";
  - a ação Mouse para cima `event.target.buttonImportIcon();`.
- **Ao preencher:** clicar no campo abre o seletor de imagem. A figura vira o ícone do botão,
  ajustada proporcionalmente e centrada.
- **Aba Opções dos botões:**
  - Layout: Somente rótulo, Somente ícone, Ícone em cima e rótulo embaixo, Rótulo em cima e ícone
    embaixo, Ícone à esquerda e rótulo à direita, Rótulo à esquerda e ícone à direita;
  - Comportamento: Nenhum, Inverter, Contorno, Pressionar;
  - Rótulo;
  - Ícone: Escolher… e Limpar.

## Decisões (Rulings)

1. **Âncora = último campo clicado.** Centralizar com vários campos move o grupo inteiro.
   Distribuir usa os centros. Custo se errado: o resultado difere pouco do Acrobat.
2. **Duplicar, colar e várias cópias:**
   - Duplicar e colar criam widgets do mesmo campo, como o Acrobat.
   - Criar várias cópias cria campos novos com nomes `nome_1`, `nome_2`… O Acrobat renomeia o
     original para `nome.0.0` e cria `nome.0.1`…, mas nomes simples evitam mexer no original.
   - Custo: os nomes diferem dos do Acrobat; preencher e calcular funcionam igual.
3. **Setas:** 1 ponto; com `Shift`, 10 pontos.
4. **Organizar e alterar vários campos** é uma operação com um instantâneo, ou seja, uma etapa do
   desfazer.
5. **Os scripts não executam.** O JavaScript não roda; só `event.target.buttonImportIcon();`
   (exato) é reconhecido, como o pedido de imagem do campo Imagem.
6. **Abrir um arquivo** só aceita PDF, com confirmação. Programas e outros tipos são recusados com
   um aviso. Custo: um formulário que abre uma planilha não a abre; o Acrobat pede confirmação e
   abre.
7. **Mostrar/ocultar** grava o bit Oculto (/F 2) dos widgets. A mudança marca o documento como
   alterado e não entra no desfazer, como no Acrobat.
8. **Aparência do botão com ícone:**
   - o app gera o /AP: imagem como XObject e rótulo com Helvetica, no layout pedido;
   - o MuPDF não desenha /MK /I.

## Arquitetura

**app-state:** `arrange.rs`, com funções puras que devolvem os novos retângulos:
- `align(rects, anchor, Edge)`;
- `center_on_page(rects, page, Axis)`;
- `distribute(rects, Axis)`;
- `match_size(rects, anchor, Size)`;
- `nudge`;
- `copies_grid`;
- `snap_lines(moving, others, tol)` para as guias.

**pdf-core:**
- Novas `PrepareOp`:
  - `MoveMany { moves }`;
  - `DeleteMany { ids }`;
  - `Duplicate { page, id, pages, offset }`;
  - `Copies { page, id, rows, cols, gap, size }`;
  - `SetPageActions { page, actions }`.
- `actions.rs`:
  - `FieldTrigger`, `PageTrigger` e `ActionKind`;
  - leitura e escrita de /A, /AA e /Next, com teto;
  - `FieldProps.actions`;
  - `PdfDoc::field_actions` e `page_actions`;
  - `PdfDoc::show_fields(names, show)`.
- **Botão:**
  - `FieldProps.layout`, `.behavior` e `.icon`;
  - `PdfDoc::set_button_icon(page, id, path)` (imagem ao preencher);
  - a geração do /AP com ícone.
- `NewFieldKind::Image`.

**gera-ui:**
- `PrepareState` passa a ter `selection: Vec<(i32, i32)>`, a âncora, o retângulo de seleção, as
  guias e a visualização.
- Menu Organizar e menu de contexto.
- Janelas Duplicar e Criar várias cópias.
- Abas Ações e Opções nas propriedades.
- Propriedades da página, com a aba Ações.
- Execução das ações em `fields.rs` e `doc_tab.rs`.

## Erros e limites

- Uma cadeia /Next com mais de 64 ações, ou em ciclo, é cortada.
- No máximo 400 cópias por "Criar várias cópias" (20 × 20).
- Duplicar só nas páginas que existem; a página do original é pulada.
- Imagem: os mesmos formatos e o mesmo teto de "Adicionar imagem" (PNG, JPEG…). Um arquivo
  inválido mostra um aviso e não muda o campo.

## Testes

- **Unidade (app-state):**
  - alinhar nas 6 direções;
  - centralizar um campo e um grupo;
  - distribuir;
  - mesmo tamanho;
  - setas;
  - grade de cópias;
  - linhas de guia.
- **Integração (pdf-core):**
  - MoveMany e DeleteMany numa etapa;
  - Duplicar em páginas (mesmo campo, mesmo valor);
  - Criar cópias;
  - ações: ida e volta de cada tipo e de cada gatilho, cadeia /Next, ciclo cortado;
  - ações da página;
  - mostrar/ocultar;
  - ícone do botão (/MK /I e /AP com a imagem);
  - campo Imagem criado com o script do Acrobat.
- **E2E:**
  - selecionar 3 campos com `Shift`+clique e alinhar com `L`;
  - distribuir;
  - duplicar numa segunda página;
  - botão com a ação "Mostrar/ocultar" que oculta um campo;
  - campo Imagem preenchido;
  - `P` visualizar.
