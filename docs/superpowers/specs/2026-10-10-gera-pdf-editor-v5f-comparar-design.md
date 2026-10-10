# v5f — Comparar arquivos (design)

Última ferramenta do Acrobat Pro marcada como "Em breve" no catálogo. Spec aprovada pelo próprio
Claude (autonomia total, ver CLAUDE.md).

## O que o Acrobat faz (referência)

- Ferramenta **Comparar arquivos**: escolhe o **arquivo antigo** e o **arquivo novo** (o documento aberto
  entra como antigo), com opção de intervalo de páginas de cada um, botão para trocar os dois e a opção
  "Comparar somente texto".
- Resultado: visão **lado a lado** (antigo à esquerda, novo à direita), alterações destacadas por cor e por
  tipo (**Substituído**, **Inserido**, **Excluído**), uma página de **resumo** com a contagem e uma legenda,
  lista das alterações num painel à direita, **Próxima/Anterior alteração** (Ctrl+. / Ctrl+,).
- Filtros: texto, imagens, cabeçalho/rodapé, formatação, anotações e fundo. Formatação, anotações e fundo
  vêm desligados.

Fontes: helpx.adobe.com/acrobat/using/compare-documents.html; adobe.com/acrobat/features/compare-pdfs.html.

## Decisões

1. **O resultado é um PDF** (como o "relatório de comparação" do Acrobat), aberto numa aba nova. Isso
   reaproveita o visualizador, o zoom, a impressão, o salvar e o painel Comentários.
   - Página 1: **resumo**. Traz o título, o nome e as páginas de cada arquivo e a data. Também traz a
     contagem por tipo, a legenda de cores e o link "Ir para a primeira alteração". Se não houver
     diferenças, diz "Não foram encontradas diferenças".
   - Depois, uma página **lado a lado** por par de páginas alinhadas:
     - a página antiga à esquerda e a nova à direita, como XObjects de formulário (vetoriais, sem perda);
     - acima de cada lado, uma faixa com "Antigo — página N (nome)" e "Novo — página M (nome)";
     - uma página sem par aparece sozinha, com o outro lado vazio e o aviso "Página inserida" ou
       "Página excluída".
   - Marcadores: "Resumo" e um por par ("Antigo p. N ↔ Novo p. M").
2. **Cada alteração é uma anotação de destaque** (Highlight com QuadPoints), travada (`/F` Print|Locked).
   Leva o autor "Comparação", o assunto com o tipo e um conteúdo descritivo. Exemplos:
   - `Substituído: «de» → «para»`;
   - `Inserido: «…»`;
   - `Excluído: «…»`.

   Assim, o painel Comentários vira a lista de alterações do Acrobat, com busca e estado.
   - A anotação fica no lado **novo** (Substituído e Inserido) ou no **antigo** (Excluído).
   - No outro lado, a marca é desenhada no conteúdo da página: um retângulo translúcido da mesma cor,
     ou um acento vertical na posição onde o texto entrou ou saiu.
3. **Cores** (iguais nas marcas e na legenda):
   - Substituído: azul `#4F7BF0`;
   - Inserido: verde `#2EAD4B`;
   - Excluído: vermelho `#E5484D`;
   - Formatação: laranja `#F2994A`;
   - Imagem: roxo `#9B59D0`.

   A opacidade do destaque é 0,35.
4. **Diferenças de texto** por **palavra**, com o crate `similar` (Apache-2.0):
   - algoritmo Patience, com prazo de 10 s; ao estourar o prazo, cai para um resultado mais grosso, sem
     travar;
   - a sequência é a do documento inteiro, na ordem de leitura do MuPDF, com as palavras de
     `TextPage::words` dentro das páginas escolhidas.

   Uma operação do diff que atravessa pares de páginas vira uma alteração por par. Uma substituição
   cujo lado antigo e lado novo caem em pares diferentes vira uma Exclusão num par e uma Inserção no
   outro.
5. **Alinhamento das páginas**: programação dinâmica monotônica que maximiza as palavras iguais de cada
   par (peso = 2 × iguais + 1), de modo que páginas reescritas por inteiro ainda se emparelhem em
   sequência. Uma página só fica sem par quando emparelhá-la custaria pares melhores.
6. **Imagens** (ligadas por padrão; desligadas por "Comparar somente texto"):
   - as imagens de cada página vêm dos blocos de imagem do MuPDF, identificadas pelo MD5 dos pixels;
   - num par, as imagens do antigo sem igual no novo são Excluídas, e as do novo sem igual no antigo
     são Inseridas;
   - quando há uma de cada lado na mesma ordem, o par vira "Imagem substituída".
   - A marca é o contorno roxo da imagem, mais a anotação no lado novo (ou no antigo, se for Excluída).
7. **Formatação** (desligada por padrão, como no Acrobat): numa palavra igual dos dois lados, compara:
   - a fonte, sem o prefixo de subconjunto `ABCDEF+`;
   - o tamanho, arredondado a 0,5 pt;
   - a cor.

   Palavras seguidas com a mesma mudança viram uma alteração só, por exemplo
   `Formatação: Helvetica 11 → Helvetica-Bold 11`.
8. **Janela "Comparar arquivos"** (no nível do app, como Combinar):
   - duas colunas, **Arquivo antigo** e **Arquivo novo**. Cada uma tem:
     - "Escolher arquivo…";
     - o nome e o número de páginas;
     - um campo de senha, quando o arquivo pede;
     - Páginas: "Todas" ou um intervalo (o mesmo `parse_ranges` da Marca d'água).
   - o antigo vem preenchido com o documento da aba ativa; se ela tiver alterações não salvas, aparece
     o aviso "Compara a versão salva no disco".
   - botão **⇄ Trocar**, caixas "Comparar somente texto" e "Incluir formatação", e o botão **Comparar**.
   - Comparar pede onde salvar o resultado (padrão `Comparação de <novo>.pdf`, ao lado do novo). Roda
     numa thread, com o progresso "Lendo o arquivo antigo… / Comparando… / Montando o resultado…" e o
     botão Cancelar.
   - Ao terminar, abre o resultado numa aba, com a linha **Comparação** aberta.
9. **Linha Comparação** na aba de resultado:
   - "Alteração i de N", **◀ Anterior** (Ctrl+,) e **Próxima ▶** (Ctrl+.);
   - a navegação percorre as anotações de autor "Comparação" em ordem de página e de posição, seleciona
     cada uma e rola até ela;
   - botão **Nova comparação**, que abre a janela.

   O resultado reaberto mais tarde também mostra a linha: o PDF leva `/GeraCompare true` no `/Info`.
10. **Segurança**:
   - arquivo com cópia de conteúdo proibida (`pages_can_be_copied`) é recusado, como no Combinar,
     porque o resultado sai sem senha;
   - arquivo com senha de abertura exige a senha;
   - o destino não pode ser um arquivo aberto numa aba nem um dos dois de entrada;
   - a gravação é atômica (`write_new`).
11. **Limites**: até 20 000 páginas de cada lado (pela memória). O texto de cada alteração no conteúdo
    da anotação é cortado em 300 caracteres, com "…".

## Fora do escopo (registrado)

- Bandas ligando os dois lados, comparação de anotações e de fundo, filtro de cabeçalho/rodapé e o tipo
  de documento (relatório, apresentação…).
- Visão "só o antigo" ou "só o novo". O resultado mostra os dois lados juntos.
- Campos de formulário e anotações dos originais não aparecem nas páginas do resultado: o conteúdo da
  página vai, as anotações não.

## Testes

- **pdf-core** (`tests/compare.rs`), com PDFs gerados no teste (texto e imagens):
  - documentos iguais dão 0 alterações;
  - os três tipos de alteração saem com o texto certo;
  - uma página inserida no meio fica sem par, e as outras se emparelham;
  - imagem trocada aparece; com "somente texto", não aparece;
  - formatação aparece só quando ligada;
  - o PDF do resultado tem o resumo, uma página por par, as anotações travadas com autor "Comparação",
    `/GeraCompare` e os marcadores;
  - arquivo restrito é recusado;
  - cancelar interrompe.
- **app-state**: o alinhamento das páginas é uma função pura testada à parte, e a navegação
  próxima/anterior é a ordenação mais o índice.
- **gera-ui**: a janela (estado: trocar, intervalos inválidos, destino proibido) e a linha Comparação.
- **E2E em Xvfb**: abrir A, Ferramentas → Comparar arquivos, escolher B, Comparar, ver a aba do
  resultado, usar Ctrl+. e ver a seleção mudar.
