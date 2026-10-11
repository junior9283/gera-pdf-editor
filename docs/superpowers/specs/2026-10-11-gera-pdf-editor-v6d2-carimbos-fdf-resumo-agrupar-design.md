# v6d2 — Carimbos personalizados, FDF/XFDF, Resumir comentários e agrupar

Data: 2026-10-11 · Etapa da avaliação para a 1.0 (lacunas C08, C09, C11) · Versão alvo 0.18.0

## Objetivo

Fechar a linha Comentar no padrão do Acrobat Pro:
- carimbos personalizados criados de imagem ou PDF, em categorias próprias;
- exportar e importar comentários (FDF, XFDF e comentários de outro PDF);
- Resumir comentários num PDF novo;
- agrupar e desagrupar comentários.

Também entram os 6 ajustes adiados da v6d (M1–M6).

## Fora desta etapa

- Carimbos dinâmicos personalizados com campos de formulário (JavaScript): o Acrobat usa scripts;
  aqui os dinâmicos continuam os do catálogo (nome, data e hora).
- Imprimir com o resumo de comentários direto do diálogo Imprimir: o resumo vira um PDF que se
  imprime como qualquer outro.

## Comportamento de referência (Acrobat Pro)

### Carimbos personalizados
- Menu do Carimbo ▸ **Carimbos personalizados ▸ Criar…**:
  - escolher um PDF ou uma imagem (PNG, JPEG, TIFF, BMP, GIF);
  - para PDF de várias páginas, escolher a página;
  - escolher a categoria (existente ou nova) e o nome.
- Os carimbos ficam guardados no perfil do usuário e aparecem no menu do Carimbo, numa entrada por
  categoria, ao lado de Dinâmicos e Padrão comercial.
- **Gerenciar carimbos…**: renomear, trocar de categoria e excluir.
- Na página, o carimbo personalizado se comporta como os outros (clicar põe no tamanho natural,
  arrastar define o tamanho; mover, redimensionar, Propriedades, desfazer).

### Exportar e importar
- Menu "···" do painel Comentários:
  - **Exportar todos para arquivo de dados…** (FDF ou XFDF, pela extensão);
  - **Exportar selecionados…** (o comentário selecionado e as respostas dele);
  - **Importar arquivo de dados…** (FDF, XFDF ou PDF: os comentários de outro PDF).
- Importar acrescenta os comentários às páginas pelo número de página. Página inexistente:
  o comentário é ignorado, e um aviso diz quantos.
- Importar é uma etapa do desfazer.

### Resumir comentários
- Menu "···" do painel Comentários ▸ **Resumir comentários…**, com o diálogo:
  - **Layout**, com as 4 opções do Acrobat:
    1. Documento e comentários com linhas de conexão em páginas separadas;
    2. Somente comentários;
    3. Documento e comentários com linhas de conexão em páginas únicas;
    4. Documento e comentários com números de sequência em páginas separadas.
  - **Ordenar por:** página, autor, data ou tipo.
  - **Incluir:** todos os comentários, ou só os que o filtro do painel mostra.
  - **Páginas:** todas ou um intervalo.
  - **Tamanho da fonte:** pequeno, médio ou grande.
  - **Incluir páginas sem comentários.**
- O resultado é um PDF novo, que abre numa aba nova: "Resumo de comentários em <arquivo>".
  Cada item traz número, tipo, autor, data, texto e respostas.

### Agrupar
- Com Selecionar, `Ctrl`+clique junta comentários à seleção (na mesma página). O menu de contexto
  ganha **Agrupar**; num grupo, **Desagrupar**.
- O grupo se move junto, se exclui junto e aparece no painel como um item (o principal).
- Clicar num membro seleciona o grupo todo.
- Grava `/IRT` = principal com `/RT /Group`, como o Acrobat e como o Substituir texto da v6d.

## Decisões (Rulings)

1. **Carimbos guardados como PDF de uma página** em
   `$XDG_CONFIG_HOME/gera-pdf-editor/stamps/<categoria>/<nome>.pdf`.
   - A imagem vira PDF ao criar (MuPDF abre a imagem como documento).
   - Ao aplicar, a página entra como Form XObject na aparência (/AP /N) do `Stamp`.
   - O `/Name` do carimbo é `#` + o nome, e o tamanho natural é o da página, com teto de 300 pt
     no lado maior.
   - Custo se errado: carimbos de outros programas não entram no catálogo (só os criados aqui).
2. **FDF pelo MuPDF:**
   - Exportar monta um documento cujo /Root é o dicionário /FDF, enxerta as anotações com as
     aparências e grava com o cabeçalho trocado para `%FDF-1.2`.
   - Importar faz o inverso: lê os bytes, troca o cabeçalho e abre com o MuPDF.
   - Custo se errado: leitores muito estritos poderiam estranhar a tabela xref no FDF (o formato
     a admite).
3. **XFDF próprio**, com `quick-xml` (já na árvore de dependências).
   - Cobre: nota, marcações de texto, caixa de texto, texto explicativo, formas, linha, polígono,
     linhas conectadas, desenho, carimbo, cursor, com cor, opacidade, espessura, autor, datas,
     texto e respostas (`inreplyto`).
   - A aparência não vai no XFDF: o MuPDF a refaz ao importar. Carimbo personalizado vira o
     carimbo genérico do nome.
   - Custo se errado: perda da aparência de carimbos personalizados em XFDF; o FDF a preserva.
4. **Resumo desenhado pelo pdf-core**, com o mesmo maquinário do relatório do Comparar: páginas
   como Form XObject, texto em Helvetica WinAnsi, linhas de conexão do ponto do comentário até o
   item na coluna.
   - Páginas do resumo: A4 em pé. Com linhas de conexão em páginas únicas, a página do documento
     fica reduzida à esquerda e os comentários à direita.
   - O texto de cada item é cortado em 2000 caracteres.
   - O resumo traz no máximo 5000 comentários e 2000 páginas.
5. **Agrupar só na mesma página** e só comentários principais: respostas e status não entram, e
   um grupo não entra em outro.
   - Desagrupar tira `/IRT` e `/RT` dos membros.
   - Agrupar e desagrupar são uma etapa do desfazer cada.
6. **Multisseleção mínima:**
   - `Ctrl`+clique acrescenta ou tira da seleção na mesma página;
   - mover arrasta só o selecionado (ou o grupo dele);
   - Excluir com vários selecionados exclui todos numa etapa.

## Arquitetura

**pdf-core:**
- `annot_group.rs`: `AnnotOp::Group { page, ids }` e `AnnotOp::Ungroup { page, id }`.
  - Mover um membro move o grupo todo.
  - `Delete { page, id }` de um membro já exclui o grupo (v6d).
- `comments_io.rs`:
  - `PdfDoc::export_comments(path, format, only: Option<Vec<(i32, i32)>>) -> Result<usize>`;
  - `PdfDoc::import_comments(path) -> Result<ImportReport { added, skipped }>`.
- `comment_summary.rs`: `summarize_comments(doc, &SummaryOptions, dest) -> Result<usize>`.
- `stamp_custom.rs`:
  - `stamp_pdf_from_file(src, page, dest) -> Result<(f32, f32)>`: normaliza imagem ou PDF para
    um PDF de uma página;
  - `NewAnnot::CustomStamp { page, rect, source: PathBuf, name }`.

**app-state:**
- `custom_stamps.rs`: catálogo em disco (listar categorias e carimbos, criar, renomear, mover,
  excluir), com nomes saneados (sem `/`, `..`, nem caracteres de controle; até 64 caracteres).

**gera-ui:**
- menu do Carimbo com as categorias personalizadas;
- diálogos Criar carimbo e Gerenciar carimbos;
- menu "···" do painel Comentários (Exportar, Importar, Resumir) e o diálogo Resumir;
- `Ctrl`+clique e Agrupar/Desagrupar no menu de contexto.

## Erros e limites

- Importar:
  - teto de 5000 comentários por arquivo e 10 MB por XFDF;
  - FDF ou PDF passam pelo mesmo teto de arquivo da abertura;
  - coordenadas não finitas descartadas;
  - tipo desconhecido ignorado e contado como "ignorado".
- Arquivos do usuário: o caminho do carimbo nunca sai da pasta de carimbos (sem
  `..`, sem link simbólico para fora).
- Documento protegido contra comentários ou assinado: importar e agrupar recusados com aviso
  (`require_modifiable`).

## Testes

- **Unidade:**
  - saneamento de nomes de carimbo;
  - ordenação do resumo;
  - mapeamento de cor e retângulo do XFDF;
  - leitura com XML malformado.
- **Integração (pdf-core):**
  - exportar e reimportar FDF e XFDF preserva tipo, página, texto, autor, cor e respostas;
  - importar comentários de outro PDF;
  - resumo nos 4 layouts (contagem de páginas e texto extraído);
  - carimbo de imagem e de PDF (desenhado, sobrevive a salvar e mover);
  - agrupar, mover o grupo, desagrupar, desfazer.
- **E2E:** criar carimbo de uma imagem e aplicar; exportar XFDF e importar em outro arquivo;
  resumir; `Ctrl`+clique e agrupar.
