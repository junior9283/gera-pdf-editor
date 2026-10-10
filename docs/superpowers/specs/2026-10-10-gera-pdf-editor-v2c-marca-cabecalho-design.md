# Gera PDF Editor — Subprojeto 2c: Marca d'água e Cabeçalho e rodapé

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

As ferramentas **Marca d'água** e **Cabeçalho e rodapé** do Acrobat Pro, cada uma com **Adicionar…**,
**Atualizar…** e **Remover**, como no Acrobat (Ferramentas → Editar PDF → Marca d'água / Cabeçalho e
rodapé). A numeração Bates entra como opção do cabeçalho e rodapé.

## 2. Marca d'água

Janela "Adicionar marca d'água" com prévia da página:

- **Origem**: texto (várias linhas; fonte Helvetica, Times ou Courier, negrito; tamanho; cor) ou
  arquivo (imagem PNG/JPEG, ou uma página de um PDF).
- **Aparência**: rotação (45°, 0°, −45° ou outro valor), opacidade (0–100%), escala relativa à página
  (% da página) ou tamanho absoluto, **atrás** ou **na frente** da página.
- **Posição**: vertical (topo, centro, base + distância em pt) e horizontal (esquerda, centro,
  direita + distância em pt), relativa à página como ela aparece (com a rotação dela).
- **Páginas**: todas, um intervalo ("1-3, 5") ou só pares / só ímpares.
- Atualizar troca a marca existente pela nova (a janela abre com as escolhas da atual); Remover tira
  as marcas d'água de todas as páginas — também as feitas pelo Acrobat.

Técnica: um XObject de formulário com a marca (texto com a fonte padrão embutida, imagem ou a página do
outro PDF enxertada), desenhado em cada página dentro de
`/Artifact <</Type/Pagination/Subtype/Watermark/GeraConfig <…>>> BDC … EMC` (o mesmo marcado que o
Acrobat usa), com opacidade por ExtGState. Atrás = antes do conteúdo; na frente = depois, com o
conteúdo antigo entre q/Q. `GeraConfig` guarda as escolhas (JSON em hexadecimal) só para preencher a
janela do Atualizar; lido do arquivo, é tratado como dado não confiável (validado, com padrões).

## 3. Cabeçalho e rodapé

Janela "Adicionar cabeçalho e rodapé" com prévia:

- Seis caixas: cabeçalho esquerdo, central e direito; rodapé esquerdo, central e direito.
- Fonte (Helvetica, Times, Courier), tamanho, cor; margens superior, inferior, esquerda e direita
  (padrão 36 pt).
- **Inserir número da página** (`<<n>>`, com formatos "1", "1 de n", "Página 1", "Página 1 de n") e
  **Inserir data** (`<<data>>`, formatos dd/mm/aaaa, d/m/aa, aaaa-mm-dd); número inicial; total
  `<<total>>`.
- **Numeração Bates** (`<<bates>>`): prefixo, sufixo, número de dígitos e número inicial.
- Páginas: todas, intervalo, pares/ímpares. Atualizar e Remover como na marca d'água.

Técnica: texto por página dentro de `/Artifact <</Type/Pagination/Subtype/Header>> BDC` (e `/Footer`),
por cima do conteúdo.

## 4. Regras comuns

- Desfazer por instantâneo; recusar em PDF assinado ou sem permissão de alterar.
- Removido = os trechos marcados saem do conteúdo de todas as páginas.

## 5. Testes

- Marca de texto em todas as páginas e só nas escolhidas; atrás/na frente; opacidade; rotação; imagem
  e página de PDF como origem; posição relativa à página girada; Atualizar troca; Remover tira (também
  um trecho no formato do Acrobat); desfazer.
- Cabeçalho/rodapé nas seis posições; `<<n>>`/`<<total>>`/data/Bates; número inicial; páginas
  escolhidas; Remover.
