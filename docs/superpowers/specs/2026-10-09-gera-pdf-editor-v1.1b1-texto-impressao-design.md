# Gera PDF Editor — Subprojeto 1.1-B1: Seleção de texto, impressão e propriedades

Data: 2026-10-09 · Status: aguardando revisão

## 1. Objetivo

Completar o uso cotidiano de leitura no arranjo do Acrobat Pro DC, sem mudar o modelo de anotações
(isso é a v1.1-B2):

- selecionar texto **pelo fluxo** (como o Acrobat), copiar, selecionar tudo e agir sobre a seleção
  com o botão direito;
- **imprimir** por um diálogo próprio no estilo do Acrobat, enviando ao CUPS;
- ver as **Propriedades** do documento.

### Critérios de sucesso

- Arrastar com a ferramenta Selecionar marca o texto na ordem de leitura (linhas inteiras entre o
  ponto inicial e o final), inclusive com a vista girada; Ctrl+C copia esse texto com quebras de linha.
- Ctrl+A copia (com Ctrl+C) o texto do documento inteiro.
- Botão direito sobre a seleção: Copiar, Destacar texto, Sublinhar texto, Riscar texto.
- Ctrl+P abre o diálogo de impressão; "Imprimir" envia ao CUPS o PDF com as opções escolhidas e
  informa o resultado na barra de status.
- Ctrl+D mostra as propriedades (descrição e avançado).
- Metas da v1 continuam: abertura < 300 ms (binário), memória < 150 MB, binário < 25 MB.

### Fora de escopo

- v1.1-B2: caixa de texto, formas, linha/seta, carimbos, edição/remoção/movimento de anotações,
  respostas.
- Impressão em pôster e livreto; propriedades da impressora; editar metadados (título, autor).

### Decisões já tomadas

- Comportamentos e atalhos copiam o Acrobat (Ctrl+P imprimir, Ctrl+D propriedades, Ctrl+A
  selecionar tudo, Ctrl+C copiar).
- Impressão: **diálogo próprio + CUPS** (`lp`), PDF vetorial, sem rasterizar.
- O riscado (StrikeOut) entra nesta etapa, por fazer parte do menu da seleção.

## 2. Arquitetura

| Crate | Mudança |
|---|---|
| pdf-core | `select_text(page, a, b) -> TextSelection { quads, text }` (via `highlight_selection` do MuPDF e os caracteres dentro das quads, em ordem de leitura); `page_text(page) -> String`; `NewAnnot::StrikeOut`; `properties() -> DocProperties`; `export_for_print(pending, dest, with_annots)` (grava uma cópia temporária com as pendentes, ou sem nenhuma anotação) |
| pdf-core/worker | pedidos `SelectText { tag, page, a, b }`, `DocumentText { tag }`, `Properties`, `ExportForPrint { pending, dest, with_annots }`, com as respostas correspondentes |
| app-state | `print.rs`: `PageRange` (parser de "1-3, 7"), `PrintOptions` e `lp_args(&PrintOptions, file) -> Vec<String>` (pura); `pdf_date.rs`: conversão de `D:AAAAMMDDhhmmss…` para "dd/mm/aaaa hh:mm" |
| gera-ui | seleção em `edit.rs` (estado da seleção, pintura, menu de contexto); `shell/print_dialog.rs`; `shell/properties.rs`; impressão em segundo plano (`print_job.rs`) |

### Seleção

- Estado por aba: `Selection { page, a, b, quads, text }` (uma página por vez na v1.1-B1; seleção
  entre páginas fica para depois).
- Ao arrastar com Selecionar, a UI pede `SelectText` ao worker com uma `tag` crescente; respostas
  de tags antigas são descartadas. Pinta as quads em azul translúcido via `PageMap`.
- Destacar e Sublinhar (ferramentas) também passam a usar `SelectText` no lugar de
  `words_in_rect`, gerando as marcas com as quads da seleção.
- Ctrl+A pede `DocumentText` e guarda o texto como seleção "documento inteiro"
  (sem pintar quads); Ctrl+C copia o que estiver selecionado.
- Botão direito sobre uma quad da seleção abre o menu de contexto; Destacar/Sublinhar/Riscar
  criam a anotação pendente com as quads e limpam a seleção.

### Impressão

- Diálogo modal (egui) com: impressora (lista de `lpstat -e`, padrão de `lpstat -d`), cópias,
  escala de cinza, páginas (Todas · Página atual · Páginas "1-3, 7"; mais opções: todas/ímpares/
  pares e ordem inversa), dimensionamento (Ajustar · Tamanho real · Reduzir páginas grandes ·
  Escala personalizada %), várias páginas por folha (1, 2, 4, 6, 9), frente e verso (não · borda
  longa · borda curta), orientação (automática · retrato · paisagem), comentários e formulários
  (Documento e marcações · Documento) e pré-visualização da página com setas.
- Ao imprimir: o worker grava o PDF temporário (`export_for_print`); uma thread chama
  `lp -d <impressora> -n <cópias> -o …  <arquivo>`, apaga o temporário e devolve o resultado.
- O executável `lp`/`lpstat` pode ser trocado pelas variáveis `GERA_LP` e `GERA_LPSTAT` (para os
  testes E2E capturarem os argumentos sem imprimir).
- Sem impressoras ou sem CUPS: o diálogo mostra "Nenhuma impressora encontrada" e o botão
  Imprimir fica desativado.

Mapeamento para o CUPS (opções padrão do `lp`):

| Opção | Argumento |
|---|---|
| Páginas "1-3, 7" | `-o page-ranges=1-3,7` |
| Página atual | `-o page-ranges=N` |
| Ímpares / pares | `-o page-set=odd` / `even` |
| Ordem inversa | `-o outputorder=reverse` |
| Ajustar | `-o fit-to-page` |
| Reduzir páginas grandes | `-o fit-to-page` só se alguma página for maior que o papel (A4); senão nada |
| Tamanho real | nenhum |
| Escala personalizada | `-o scaling=N` |
| Várias por folha | `-o number-up=N` |
| Frente e verso | `-o sides=two-sided-long-edge` / `two-sided-short-edge` (padrão `one-sided`) |
| Orientação | `-o orientation-requested=3` (retrato) / `4` (paisagem); automática: nada |
| Escala de cinza | `-o print-color-mode=monochrome` |
| Cópias | `-n N` |

### Propriedades

Janela com duas seções:
- **Descrição:** arquivo (caminho), título, autor, assunto, palavras-chave, criado, modificado,
  aplicativo (Creator), produtor (Producer).
- **Avançado:** versão do PDF, tamanho da 1ª página (mm e pol), número de páginas, tamanho do
  arquivo (KB/MB), protegido por senha (sim/não).
- Campos ausentes aparecem como "—". Datas inválidas aparecem como o texto original.

## 3. Erros

- Seleção sem texto (página escaneada): nada é selecionado; Ctrl+C não altera a área de transferência.
- Falha ao gravar o temporário de impressão ou `lp` com código ≠ 0: mensagem na barra de status
  ("Não foi possível imprimir: <erro do CUPS>"); o temporário é sempre apagado.
- Intervalo de páginas inválido ("0", "5-2", "abc", página além do total): o campo fica vermelho
  com a explicação e Imprimir fica desativado.
- `lpstat` ausente: diálogo abre com "Nenhuma impressora encontrada".

## 4. Testes

- **Unitários (app-state):** parser de intervalos (válidos, espaços, inválidos, além do total);
  `lp_args` para cada opção e combinações; conversão de datas PDF (completa, só data, com fuso,
  inválida).
- **pdf-core:** `select_text` no PDF de exemplo (texto e quads da linha), com pontos invertidos
  (de baixo para cima) e fora do texto; `page_text`; StrikeOut gravado e relido; `properties`
  (título/autor do exemplo, nº de páginas, versão); `export_for_print` com e sem anotações.
- **E2E (Xvfb + xdotool, feitos pelo Claude):** arrastar seleção e conferir o azul; Ctrl+C e ler a
  área de transferência (`xclip`/`xsel` se disponível, senão colar no campo de busca); menu de
  contexto → Riscar → salvar → reabrir; Ctrl+A + Ctrl+C; diálogo de impressão com `GERA_LP`
  falso conferindo os argumentos; Ctrl+D conferindo os campos.
