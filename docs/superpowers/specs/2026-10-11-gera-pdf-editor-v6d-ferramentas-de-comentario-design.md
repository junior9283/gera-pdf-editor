# v6d — Ferramentas de comentário (marcação de texto, desenho, texto explicativo, anexo)

Data: 2026-10-11 · Etapa da avaliação para a 1.0 (lacunas C01–C06) · Versão alvo 0.17.0

## Objetivo

Completar as ferramentas da linha Comentar no padrão do Acrobat Pro:
- inserir e substituir texto;
- Riscar na barra;
- nuvem, polígono e linhas conectadas;
- texto explicativo;
- borracha do lápis;
- anexar arquivo como comentário.

Tudo editável no modelo da B2a (selecionar, mover, propriedades, desfazer, painel Comentários).

## Fora desta etapa

Carimbos personalizados, exportar e importar FDF/XFDF, Resumir comentários e agrupar ficam na
**v6d2** (C08, C09, C11).

## Comportamento de referência (Acrobat Pro)

- **Marcação de texto** (barra Comentar, com `U`/`Shift+U` alternando Destacar, Sublinhar e
  Riscar):
  - **Riscar** risca o texto arrastado.
  - **Substituir texto:** arrastar sobre o texto risca e abre uma nota para o texto novo. Grava
    `StrikeOut` + `Caret` agrupados (`/IRT` com `/RT /Group`).
  - **Inserir texto:** clicar entre letras põe um cursor de inserção (`Caret`) com nota.
- **Desenho** (menu da ferramenta de desenho, com `D`/`Shift+D` alternando):
  - Retângulo, Oval, Linha, Seta (já existem);
  - **Polígono:** cliques nos vértices; clique duplo ou clique no primeiro ponto fecha;
  - **Linhas conectadas:** polilinha; clique duplo termina;
  - **Nuvem** (`Q`): polígono com borda de nuvem (`/BE /S /C`);
  - **Lápis** (já existe) e **Borracha:** apaga trechos dos traços do lápis sob o arraste.
- **Texto explicativo:** arrastar a caixa e depois a ponta da seta. Grava `FreeText` com
  `/IT /FreeTextCallout` e `/CL` (3 pontos). A caixa e a ponta se movem em separado.
- **Anexar arquivo** (`Shift+J` alterna Carimbo e Anexar arquivo): clicar na página, escolher
  o arquivo e gravar `FileAttachment` com o arquivo embutido e o ícone de clipe.
  - O anexo aparece no painel Anexos (v6c), e abrir segue a mesma regra de segurança.
- **Edição:**
  - polígono, polilinha e nuvem mostram alças nos vértices (arrastar um vértice muda só ele);
  - texto explicativo mostra alças da caixa e da ponta;
  - Propriedades: cor, espessura, opacidade, preenchimento (polígono e nuvem), intensidade da
    nuvem.

## Decisões (Rulings)

1. **Aparências pelo MuPDF 1.27.** Polígono, polilinha, nuvem, `Caret`, `FreeTextCallout` e
   ícone do anexo têm aparência gerada pelo MuPDF 1.27 (`pdf-appearance.c`), sem desenho
   próprio. Custo se errado: a aparência fica igual à do MuPDF, que já segue a especificação.
2. **Substituir texto** cria duas anotações: o riscado, que é o pai, e o `Caret`, com
   `/IRT` = riscado e `/RT /Group`.
   - O painel Comentários mostra o par como um item só ("Substituir texto").
   - Mover, excluir ou desfazer agem no par.
3. **Borracha só no lápis.** Apaga os pontos dentro do raio (8 pt na tela) e corta o traço onde
   houve apagamento. Traço que fica com menos de 2 pontos sai; desenho sem traços sai inteiro.
   Uma passada da borracha é uma etapa do desfazer.
4. **Anexo como comentário** usa o mesmo teto da v6c (100 MB).
   - O nome do arquivo vai para `/FS` (UF/F) e a descrição para `/Contents`.
   - Em documento protegido contra alteração, a ferramenta fica indisponível.
5. **Polígono com cliques:**
   - clique simples acrescenta vértice;
   - clique duplo termina;
   - `Esc` cancela;
   - `Backspace` remove o último vértice;
   - clicar a menos de 6 px do primeiro ponto fecha o polígono.
   - Mínimo de 3 vértices (polígono e nuvem) ou 2 (polilinha).
   - Teto de 1000 vértices.

## Arquitetura

**pdf-core:**
- `AnnotKind` ganha `Polygon`, `PolyLine`, `Cloud`, `Caret`, `Callout` e `FileAttachment`.
- `AnnotInfo` ganha:
  - `vertices: Vec<(f32, f32)>` (polígono, polilinha, nuvem);
  - `callout: Option<[(f32, f32); 3]>`;
  - `group: Option<i32>` (substituir texto: o id do par).
- `NewAnnot` ganha:
  - `Polygon { page, vertices, closed: bool, cloud: bool }`;
  - `Caret { page, rect, text }`;
  - `ReplaceText { page, rects, text }` (riscado + caret agrupados);
  - `Callout { page, rect, knee, tip, text, size, color }`;
  - `FileAttachment { page, at, path, description }`.
- `AnnotOp`:
  - `MoveVertex { page, id, index, to }`;
  - `MoveCallout { page, id, knee, tip }`;
  - `Erase { page, id, at: Vec<(f32, f32)>, radius }`.
- Funções puras:
  - `erase_strokes(strokes, path, radius) -> Vec<Vec<(f32, f32)>>`;
  - `close_polygon(points, click, tolerance)`.

**gera-ui:**
- `app_state::tools::Tool`:
  - ganha `StrikeOut`, `Polygon`, `PolyLine`, `Cloud`, `Eraser`, `Callout`, `InsertText`,
    `ReplaceText` e `Attach`;
  - grupos de alternância: `U` (Destacar, Sublinhar, Riscar), `D` (desenhos), `Q` (Nuvem),
    `Shift+J` (Carimbo, Anexar).
- Barra Comentar: ícones novos, com menus de desenho e de marcação de texto como no Acrobat.
- `edit.rs`:
  - polígono em construção (pré-visualização);
  - borracha (círculo sob o ponteiro);
  - texto explicativo em dois arrastes;
  - alças de vértice e de ponta.

## Erros e limites

- Tudo que é lido do PDF passa por teto: 1000 vértices por anotação e 10 000 pontos por desenho.
- Coordenadas não finitas são descartadas.
- Arquivo anexado: até 100 MB, o nome passa por `safe_display` e o arquivo nunca é executado.

## Testes

- **Unidade:** `erase_strokes` (corte no meio, apaga tudo, sem tocar), `close_polygon`, ciclos
  de `U`/`D`/`Shift+J`.
- **Integração (pdf-core):**
  - criar e reler cada tipo, mover vértice e ponta;
  - substituir texto com grupo e desfazer em uma etapa;
  - borracha;
  - anexo grava o arquivo e aparece em `attachments()`.
- **E2E:** desenhar nuvem com cliques, polígono, texto explicativo, inserir e substituir texto,
  borracha e anexar.
