# Gera PDF Editor — Subprojeto 2a: Organizar páginas

Data: 2026-10-09 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Organizar páginas** do Acrobat Pro: o documento vira uma **grade de miniaturas** onde
se seleciona páginas e se **gira, exclui, extrai, insere (de arquivo ou em branco), substitui,
recorta e reordena arrastando**, tudo com desfazer/refazer e gravado só ao Salvar. Dividir PDF e
Combinar arquivos ficam para a **v2b**.

### Critérios de sucesso

- Ferramentas → Organizar páginas (ou Exibir → Organizar páginas) troca a área do documento pela
  grade; "Fechar" (ou Esc) volta. A barra secundária, como no Acrobat:
  **Selecionar ▾ (Todas, Pares, Ímpares, Retrato, Paisagem)**, **Girar ⟲ / ⟳**, **Excluir**,
  **Extrair**, **Inserir ▾ (De arquivo…, Página em branco)**, **Substituir…**, **Recortar…**,
  zoom das miniaturas (slider), Desfazer/Refazer.
- Seleção: clique, Ctrl+clique (alterna), Shift+clique (intervalo), Ctrl+A; Delete exclui as
  selecionadas (nunca todas: o documento precisa ficar com ao menos uma página).
- **Arrastar** miniaturas (uma ou várias selecionadas) reordena, com marcador de inserção.
- Clique direito numa miniatura: Girar ⟲/⟳, Excluir, Extrair, Inserir antes/depois ▸, Substituir.
- Cada miniatura mostra o número da página e, ao passar o mouse, botões girar e excluir (como no
  Acrobat).
- **Extrair**: diálogo com "Excluir páginas após extrair" e "Extrair como arquivos separados";
  grava em arquivo(s) escolhido(s) (o documento aberto só muda com "excluir após").
- **Inserir de arquivo**: escolhe o PDF (pede senha se protegido), posição (antes/depois da página N,
  início, fim) — as anotações das páginas inseridas vêm junto.
- **Substituir**: páginas selecionadas trocadas por um intervalo de outro PDF, mesma quantidade.
- **Recortar**: margens (mm) em cima/baixo/esquerda/direita aplicadas às selecionadas (CropBox),
  com pré-visualização do retângulo na miniatura.
- Desfazer/refazer (Ctrl+Z / Ctrl+Shift+Z) cobre operações de página e de anotação misturadas.
- Depois de cada operação, a vista, o painel de miniaturas, o sumário e as anotações refletem o
  documento novo; a página atual é mantida quando possível.

### Fora de escopo (v2b)

Dividir PDF, Combinar arquivos, numeração de páginas (Bates), rótulos de página.

## 2. Arquitetura

### pdf-core

- `PageOp` (aplicada no documento em memória): `Rotate { pages, quarter_turns }`, `Delete { pages }`,
  `Move { pages, to }` (ordem relativa mantida; `to` = índice antes do qual ficam),
  `InsertBlank { at, size }`, `InsertFrom { at, path, password, range }`,
  `Replace { pages, path, password, from }`, `Crop { pages, margins_pt }`.
- `PdfDoc::extract(pages, dest, separate) -> Vec<PathBuf>`: grava novo(s) PDF(s) (sem senha,
  como o Acrobat faz ao extrair), não muda o documento.
- **Desfazer por instantâneo**: antes de uma `PageOp`, o documento é serializado em memória
  (`Encryption::Keep`) junto com o conjunto `hidden`; as apagadas (escondidas) são removidas de
  verdade antes da operação (os índices mudam). Desfazer/refazer de página troca o documento pelo
  instantâneo. O histórico passa a ter dois tipos de passo: edição de anotação e instantâneo.
  Limite de memória dos instantâneos: 512 MB (os mais antigos saem).
- Worker: `Request::Pages(PageOp)`, `Request::Extract { pages, dest, separate }`;
  `Response::PagesChanged { summary, history }` (recarrega tudo na interface), `Response::Extracted`.
  Undo/Redo que voltam um instantâneo também respondem `PagesChanged`.

### app-state

- `organize.rs`: seleção (clique, Ctrl, Shift, todas/pares/ímpares/retrato/paisagem), destino do
  arrasto a partir da posição na grade, `layout` da grade (colunas pela largura e zoom), mapeamento
  da página atual depois de mover/excluir/inserir, validação (não excluir todas).
- `page_range` reaproveita o parser de intervalos da impressão para "Substituir por páginas 3-5".

### gera-ui

- `organize.rs`: a grade (miniaturas pelo cache de render existente, seleção, arrasto, menus,
  botões de hover) e a barra secundária; diálogos Extrair, Inserir de arquivo, Substituir,
  Recortar, Página em branco (tamanho: igual à página atual, A4, Carta).
- `DocTab` ganha o modo Organizar; `PagesChanged` reconstrói a vista.

## 3. Erros

- Operação inválida (excluir todas, intervalo fora do documento, PDF de origem corrompido ou senha
  errada): nada muda; status explica.
- Falha ao gravar a extração: status com o motivo; nenhum arquivo pela metade (gravação segura).
- Memória dos instantâneos acima do limite: os mais antigos são descartados (o desfazer não alcança
  passos muito antigos).

## 4. Testes

- **pdf-core**: cada `PageOp` + desfazer + refazer (contagem, ordem verificada pelo texto das
  páginas, rotação, CropBox, anotações acompanhando as páginas), mistura com edições de anotação,
  apagadas antes de mover, extrair (1 arquivo e separados), inserir de PDF protegido, limite de
  memória.
- **app-state**: seleção (Ctrl/Shift/intervalos), destino do arrasto, página atual depois das
  operações, não excluir todas.
- **E2E (Xvfb)**: abrir Organizar; selecionar 2 páginas e girar; arrastar a página 3 para o início;
  excluir; inserir em branco; inserir de arquivo; extrair; desfazer tudo; salvar e reabrir.
