# v6c2 — Miniaturas, Camadas, Destinos e rótulos de página

Data: 2026-10-11 · Etapa da avaliação para a 1.0 (lacunas P1/P2 dos painéis laterais) · Versão alvo 0.16.0

## Objetivo

Completar os painéis laterais no padrão do Acrobat Pro:

- **Miniaturas de página** com seleção, menu de contexto e arraste;
- painel **Camadas** (conteúdo opcional, OCG);
- painel **Destinos** (destinos nomeados);
- **rótulos de página** (numeração lógica);
- **copiar páginas entre PDFs** abertos.

Entram também os 7 ajustes menores adiados da revisão da v6c.

Sucesso é o usuário de Acrobat encontrar os mesmos lugares, nomes e atalhos, com resultado igual no
arquivo: rótulos e destinos legíveis por outros leitores, camadas mostradas como no Acrobat.

## Comportamento de referência (Acrobat Pro)

- **Miniaturas:**
  - clique seleciona; `Ctrl` e `Shift` + clique estendem a seleção;
  - arrastar reordena, com uma linha indicando onde as páginas caem;
  - arrastar para outro documento copia as páginas;
  - `Delete` exclui as selecionadas, depois de confirmar;
  - menu de contexto: Inserir páginas (De arquivo…, Página em branco…), Extrair páginas…, Substituir
    páginas…, Excluir páginas…, Recortar páginas…, Girar páginas (90° horário e anti-horário),
    Rótulos de página…, Imprimir páginas…, Ampliar e Reduzir miniaturas.
- **Rótulos de página** (diálogo "Rótulos de página"):
  - Páginas: Todas, Selecionadas ou De–Até;
  - Numeração: "Iniciar nova seção" (Estilo: Nenhum, 1 2 3, i ii iii, I II III, a b c, A B C;
    Prefixo; Início) ou "Estender a numeração usada na seção anterior";
  - a caixa de página mostra o rótulo e a posição, como "iii (3 de 10)";
  - digitar um rótulo na caixa vai até a página dele;
  - preferência "Usar números lógicos de página", ligada por padrão.
- **Destinos:**
  - lista com Nome e Página, ordenável por qualquer um dos dois;
  - menu Opções: Novo destino (da vista atual), Ordenar por nome ou página;
  - menu de contexto: Ir para o destino, Definir destino (vista atual), Renomear e Excluir;
  - clique duplo vai até o destino.
- **Camadas:**
  - árvore como no `/Order` do arquivo, com rótulos de grupo e aninhamento;
  - olho por camada; camada travada (`/Locked`) mostra o cadeado e não muda;
  - Opções: Expandir tudo, Recolher tudo, Redefinir para visibilidade inicial e Propriedades da
    camada (nome e estado padrão);
  - o olho muda só a vista: não marca o documento como alterado e não é gravado;
  - Propriedades muda o arquivo e entra no desfazer.

## Decisões (Rulings)

1. **Diálogos do menu das miniaturas sem trocar de modo.** Os diálogos do Organizar (extrair, página
   em branco, recortar) são desenhados como janelas sobre a vista atual, sem trocar de modo. A
   seleção das miniaturas vira o alvo deles. Custo se errado: pouco, porque o código dos diálogos é o
   mesmo.
2. **Copiar páginas entre PDFs.** Como só a aba ativa aparece, arrastar miniaturas até o rótulo de
   outra aba na barra de abas copia as páginas para o fim daquele documento. O menu de contexto tem
   também "Copiar para ▸ <documentos abertos>".
   - Como no Acrobat, arrastar entre documentos copia; não move.
   - A origem grava as páginas numa cópia temporária na pasta privada 0700 (prefixo `gera-copia-`,
     varrido ao abrir), e o destino insere de lá.
   - Documento protegido contra cópia recusa; documento de destino não alterável ou assinado
     recusa.
3. **Camadas sem API no MuPDF 0.8.** O olho grava `/OCProperties /D /ON` e `/OFF` no documento em
   memória e reabre a partir dos bytes, como o desfazer.
   - O estado padrão do arquivo fica guardado à parte e volta antes de cada gravação e de cada
     instantâneo do histórico, então o olho nunca chega ao arquivo nem ao desfazer.
   - A escolha de visibilidade fica no `PdfDoc` e é reaplicada depois de desfazer, refazer e
     salvar: a vista não pula de volta sozinha.
   - Custo: um olho clicado em PDF grande leva o tempo de um desfazer, aceitável para uma ação
     rara.
4. **Rótulos lidos e gravados pelo pdf-core.** O pdf-core lê e grava a árvore numérica `/PageLabels`,
   com tetos e conserto de entradas fora de ordem; a formatação (romanos e letras AA, AB…) é Rust
   puro e testado.
   - Gravar regrava `/Nums` como lista simples ordenada.
   - Excluir, mover ou inserir páginas não ajusta os rótulos, como o MuPDF e o Acrobat (o Acrobat
     mantém as faixas por índice).
5. **Destinos nomeados.**
   - Lê a árvore `/Names /Dests` (com `/Kids`, visitados e tetos de 50 mil nomes e 32 níveis) e o
     dicionário antigo `/Dests` do catálogo.
   - Grava sempre na árvore `/Names /Dests`, como lista `/Names` simples ordenada.
   - Destinos do `/Dests` antigo, quando editados, são migrados para a árvore.
   - Nome repetido ganha sufixo " (2)".
6. **Imprimir páginas** no menu abre o diálogo Imprimir já com "Páginas: <seleção>".
7. **Ampliar e Reduzir miniaturas** mudam a largura entre 80 e 320 px, em passos de 20 %, guardada
   nas preferências.

## Ajustes adiados da v6c (minors)

- M-1: soltar um filho sobre o próprio pai não gera mais erro (o arrastado sai da lista de
  irmãos).
- M-2: Enter sem mudar o nome não envia Renomear.
- M-3: a seleção do painel Anexos é guardada por (origem, nome), não por índice.
- M-4: `xml_escape` remove caracteres de controle proibidos no XML 1.0.
- M-5: a cópia do anexo aberto é apagada ao fechar a aba.
- M-6: `/Outlines` gravado direto no catálogo é promovido a objeto indireto antes de gravar.
- M-7: o teste da árvore com ciclo confere `/Count`, `/Parent` e `/Last` depois do conserto.

## Arquitetura

**pdf-core:**
- `page_labels.rs`:
  - `PageLabelStyle` e `PageLabelRange {start_page, style, prefix, first}`;
  - `page_labels() -> Vec<PageLabelRange>`, `labels() -> Vec<String>` (tetos: 10 mil faixas) e
    `set_page_labels(Vec<PageLabelRange>)` (instantâneo);
  - funções puras `format_label` e `apply_labels(ranges, pages, new_range, extend)`.
- `layers.rs`:
  - `LayerInfo {id, name, visible, default_on, locked}` e `LayerNode` (árvore do `/Order`, com
    rótulos);
  - `layers()`, `set_layer_view(id, on)` (sem histórico; reabre);
  - `reset_layer_view()`;
  - `edit_layer(id, name, default_on)` (instantâneo).
- `destinations.rs`: `DestInfo {name, page, at}`, `destinations()`,
  `add_destination(name, page, at) -> String` (o nome final), `rename_destination`,
  `remove_destination` e `set_destination`.
- **Worker:**
  - requests: `PageLabels(Vec<PageLabelRange>)`, `LayerView{id,on}`, `ResetLayers`,
    `EditLayer{..}`, `Destination(DestOp)` e `ExportPages{pages, dest}`;
  - responses `LayersChanged`, `DestinationsChanged` e `PagesExported{dest}`;
  - `Changed::Labels`, `Changed::Layers` e `Changed::Destinations`.
- `DocSummary` ganha `labels: Vec<String>` (vazio quando não há rótulos), `layers` e
  `destinations`.

**gera-ui:**
- `thumbnails.rs` (novo, substitui `sidebar::thumbnails`):
  - seleção com `PageSet`, arraste com linha de inserção e arraste para fora do painel;
  - menu de contexto e teclas;
  - devolve `ThumbAction`.
- `layers_panel.rs`, `destinations_panel.rs` e `page_labels_dialog.rs` (novos).
- `doc_tab`: aplica as ações e mostra os diálogos do Organizar fora do modo.
- `tabs` e `app`: arraste até o rótulo de uma aba → `CopyPages`.
- `page_controls` mostra o rótulo.
- `LeftPanel` ganha `Layers` ("Camadas") e `Destinations` ("Destinos").
- `app-state`: `prefs.logical_page_numbers` (padrão true) e `prefs.thumb_width`.

## Erros e limites

- Tudo que vem do PDF passa por tetos, verificação de visitados e `safe_display` antes de ir para a
  tela.
- Rótulo com mais de 64 caracteres é cortado na exibição.
- Edições em documento protegido contra alteração ou assinado são recusadas com mensagem.
- O olho das camadas continua funcionando (é vista), como expandir marcadores.

## Testes

- **Unidade:**
  - formatação de rótulos (romano até 3999 e acima, letras AA…);
  - `apply_labels` (nova seção e estender);
  - leitura de `/Nums` fora de ordem e com ciclo de `/Kids`;
  - nomes únicos de destino;
  - ordenação;
  - `ThumbAction` de teclas e alvo do arraste.
- **Integração (pdf-core):** gravar e reler rótulos e destinos; camadas com `/Order` aninhado e
  `/Locked`; o olho não marca alterado nem chega ao arquivo salvo; propriedades da camada desfazem;
  `ExportPages` + `InsertFrom` copia páginas com anotações.
- **E2E (Xvfb):**
  - menu das miniaturas;
  - arrastar para reordenar;
  - arrastar até outra aba copia;
  - diálogo de rótulos com "iii (3 de 10)" na caixa;
  - painel Camadas esconde o conteúdo;
  - painel Destinos cria, vai até e renomeia.
