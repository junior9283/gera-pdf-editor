# Histórico de versões

Todas as mudanças relevantes do projeto. O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/)
e as versões seguem o [Versionamento Semântico](https://semver.org/lang/pt-BR/): enquanto a versão for
0.x, cada etapa do roteiro concluída sobe o número do meio.

## [Não lançado]

- v6e — formulários avançados: detecção de campos, Botão e Data, Formatar/Validar/Calcular.

## [0.18.0] — 2026-10-11 · Carimbos personalizados, FDF/XFDF, Resumir comentários e agrupar (v6d2)

### Adicionado
- **Carimbos personalizados.** No menu do Carimbo, use **Carimbos personalizados ▸ Criar…**:
  - escolha um PDF (e a página) ou uma imagem, a categoria e o nome;
  - as suas categorias aparecem no menu, ao lado de Dinâmicos e Padrão comercial;
  - **Gerenciar…** renomeia, muda de categoria e exclui.
- **Exportar e importar comentários** (menu "···" do painel Comentários):
  - exportar todos, ou o selecionado com as respostas, para **FDF** ou **XFDF**;
  - importar de FDF, de XFDF ou de outro PDF, numa etapa do desfazer, com aviso dos ignorados
    (página inexistente ou tipo desconhecido).
- **Resumir comentários** num PDF novo, que abre numa aba:
  - os 4 layouts do Acrobat: linhas de conexão em páginas separadas ou únicas, números de
    sequência, ou somente comentários;
  - ordem por página, autor, data ou tipo;
  - só os do filtro do painel, intervalo de páginas, tamanho da fonte e páginas sem comentários.
- **Agrupar e desagrupar:**
  - `Ctrl`+clique junta comentários à seleção e o menu de contexto oferece **Agrupar**;
  - o grupo se move e se exclui junto e aparece como um item no painel;
  - excluir vários selecionados é uma etapa só.

### Corrigido
- Ajustes da v6d:
  - polígono só com vértices inválidos é recusado;
  - Substituir texto não deixa cursor solto;
  - a borracha preserva traços que não tocou;
  - anexar um arquivo especial (FIFO) não trava;
  - grupos em ciclo de outros programas aparecem no painel.
- Comentários importados nunca trazem ações (JavaScript, abrir programa) nem conteúdo multimídia.

## [0.17.0] — 2026-10-11 · Ferramentas de comentário (v6d)

### Adicionado
- **Marcação de texto na barra Comentar:** Riscar, e `U`/`Shift+U` alternam Destacar,
  Sublinhar e Riscar, como no Acrobat.
- **Editar texto:**
  - **Substituir texto:** arrastar sobre o texto risca e abre a nota do texto novo. O par
    (riscado e cursor) é um item só no painel Comentários e sai junto ao excluir.
  - **Inserir texto:** um clique põe o cursor de inserção com nota.
- **Menu Desenho** (`D`/`Shift+D` alternam):
  - **Polígono**, **Linhas conectadas** e **Nuvem** (`Q`), desenhados por cliques. Clique
    duplo termina, clicar no primeiro ponto fecha, `Backspace` tira o último vértice e `Esc`
    cancela.
  - **Borracha** do lápis: corta os traços sob o arraste, e cada passada é uma etapa do
    desfazer.
- **Texto explicativo:** arraste a caixa e clique na ponta da seta. A caixa, a ponta e o joelho
  se movem em separado.
- **Anexar arquivo como comentário** (`Shift+J` alterna com Carimbo): o arquivo vai no
  comentário (até 100 MB), com o ícone de clipe.
- **Edição dos comentários novos:**
  - alças em cada vértice;
  - Propriedades com preenchimento e estilo Nuvem 1/Nuvem 2;
  - o texto explicativo tem fonte e fundo como a caixa de texto.

### Corrigido
- A janela de nota abre com o cursor no campo (antes era preciso clicar nele).
- Polígono de outro programa com mais de 1000 vértices não perde vértices ao ser editado.
- Riscado agrupado de outro programa, com o par fora da página, volta a poder ser excluído.

## [0.16.0] — 2026-10-11 · Miniaturas, Camadas, Destinos e rótulos de página (v6c2)

### Adicionado
- **Painel Miniaturas:**
  - seleção com `Ctrl` e `Shift`, arrastar para reordenar e `Delete` com confirmação;
  - menu com as ações de página (girar, excluir, extrair, inserir, substituir, recortar),
    Rótulos de página…, Copiar para ▸, Imprimir páginas… e Ampliar/Reduzir miniaturas.
- **Copiar páginas entre PDFs:** arraste a miniatura até a aba de outro documento, ou use
  Copiar para ▸. As anotações vão junto.
- **Rótulos de página** (numeração lógica, como "i, ii, iii" no prefácio):
  - diálogo Rótulos de página;
  - a caixa de página mostra "iii (3 de 10)", e digitar um rótulo leva até a página;
  - preferência "Usar números lógicos de página".
- **Painel Destinos:** ordenar por nome (ordem natural) ou página, novo destino da vista atual,
  ir até, definir destino, renomear no lugar e excluir.
- **Painel Camadas:**
  - árvore como no arquivo, com grupos e camadas travadas;
  - o olho mostra ou esconde a camada só na tela (nunca vai para o arquivo);
  - Redefinir para visibilidade inicial e Propriedades da camada (nome e estado padrão).

### Corrigido
- Edições num PDF com senha de abertura (marcadores, destinos, rótulos, camadas, propriedades)
  saíam com os textos novos ilegíveis no arquivo salvo.
- Mover, inserir ou excluir páginas não cria mais rótulos de página errados ("1, 2, 1, 2…").
- O `Delete` agora funciona nos painéis Miniaturas e Marcadores.
- Ajustes da v6c:
  - soltar um marcador sobre o próprio pai;
  - renomear sem mudar o nome não cria etapa do desfazer;
  - a seleção de anexo segue o anexo;
  - XMP sem caracteres de controle;
  - a cópia de um anexo aberto é apagada ao fechar a aba.

### Segurança
- Leitura com tetos de rótulos, destinos e camadas: prefixo enorme, árvores que apontam para si
  mesmas e milhares de nomes não travam o app.
- Em documento assinado, o olho das camadas fica bloqueado, para não criar revisões depois da
  assinatura.

## [0.15.2] — 2026-10-11 · Ícone do app no sistema

### Corrigido
- O ícone do app aparece na barra de tarefas e no menu de aplicativos. No Wayland o sistema acha o
  ícone pelo atalho `.desktop` instalado, e o AppImage sozinho não instalava nenhum. Agora, ao rodar
  como AppImage, o app grava o próprio atalho (apontando para o AppImage) e os ícones em
  `~/.local/share` e os atualiza se o arquivo mudar de lugar.

## [0.15.1] — 2026-10-11 · Correções de segurança

### Segurança
- O XMP do documento é lido com teto de tamanho (uma "bomba" de compressão no XMP não trava mais
  o app).
- Anexos com cadeia de filtros longa demais, ou com filtros que não são nomes, são recusados em vez
  de lidos sem teto.
- Marcas de direção do texto (que podem disfarçar a extensão, como "fdp.exe" parecendo "exe.pdf")
  saem do nome do anexo na lista e no nome sugerido ao salvar.

## [0.15.0] — 2026-10-11 · Propriedades, Marcadores e Anexos (v6c)

### Adicionado
- **Propriedades do documento** (`Ctrl+D`) editáveis, em abas:
  - **Descrição:** título, autor, assunto e palavras-chave, com o XMP acompanhando (a
    identificação PDF/A e PDF/UA continua).
  - **Segurança** e **Fontes** (incorporada, subconjunto, tipo).
  - **Visualização inicial:** painel, layout, ampliação, página e opções de janela, como
    "Mostrar o título do documento" (a aba passa a mostrar o título).
  - **Personalizado** e **Avançado** (idioma e preferências de impressão).
- **Painel Marcadores** editável:
  - novo marcador com `Ctrl+B` (o nome vem do texto selecionado);
  - renomear no lugar, excluir e arrastar para reordenar ou aninhar;
  - estilo (cor, negrito e itálico), definir destino na vista atual e Expandir/Recolher tudo;
  - setas, `Enter`, `Delete` e `F2`.
- **Painel Anexos:**
  - abrir anexos PDF numa aba própria, depois de um aviso de segurança;
  - salvar qualquer anexo, adicionar, excluir e descrever;
  - lista também os anexos de comentários.
- **Ícone novo** do app (janela, AppImage e tema do sistema).

### Segurança
- Anexos são lidos com teto real de tamanho, sem descomprimir uma "bomba" de compressão.
- Só anexos PDF abrem no app. Os demais só podem ser salvos, com o nome saneado.
- A cópia de um anexo aberto vai para uma pasta privada (só o usuário acessa).
- Árvores de marcadores enormes ou com ciclos são lidas com limites e gravadas consertadas, sem perder
  o que passa dos limites.

### Mudado
- A meta de tamanho do binário passou a < 50 MB.

## [0.14.0] — 2026-10-11 · Ferramentas de tela, preferências, menus e busca (v6b)

### Adicionado
- **Mão** (arrastar move a página; `Espaço` segurado em qualquer ferramenta) e **Zoom com marca**
  (arrastar um retângulo amplia até ele; clique amplia um nível, `Ctrl` + clique reduz), na barra.
- **Preferências** (`Ctrl+K`), em categorias:
  - **Geral:** aceleradores de tecla única (`V`, `H`, `Z`, `S`, `U`/`Shift+U`, `D`, `X`, `K`).
  - **Exibição de página:** layout e zoom padrão.
  - **Documentos:** restaurar a última vista ao reabrir e número de recentes.
  - **Identidade**, **Segurança** (sites lembrados no aviso de links) e **Aparência**.
- **Recortar, Copiar e Colar** comentários (`Ctrl+X`/`C`/`V`), com uma etapa só do desfazer.
  - Colar texto cria uma caixa de texto.
  - Editar → Colar com uma imagem cria um carimbo.
  - **Desmarcar tudo** (`Ctrl+Shift+A`).
- **Arquivo:** Reverter (pergunta antes), Criar PDF, Exportar PDF e Proteger PDF.
- **Localizar:**
  - opções Somente palavras inteiras, Diferenciar maiúsculas e minúsculas, Incluir marcadores e
    Incluir comentários;
  - `F3`/`Ctrl+G` e `Shift+F3`/`Ctrl+Shift+G`;
  - `Enter` no campo vai ao próximo.
- **Menu de contexto da página** (botão direito): ferramentas, navegação, girar, imprimir, localizar
  e propriedades, com os atalhos.
- **Ajuda** (`F1`).

### Corrigido
- Arrastar o número da página não enche mais o histórico de vistas.
- Esc fecha primeiro as janelas abertas.
- Na tela cheia, a aba que vem para a frente também é apresentada.
- A pergunta de tela cheia pertence à aba que a fez.

### Segurança
- Copiar um comentário respeita a proibição de copiar.
- A cópia colada não compartilha objetos com o original e tem teto de objetos.
- Reverter descarta a senha pedida e ainda não salva.
- A imagem colada é reduzida e gravada numa pasta privada.

## [0.13.0] — 2026-10-10 · Navegar e exibir (v6a)

Primeira etapa da avaliação para a 1.0.0, que comparou o app com o Acrobat Pro
([análise](docs/superpowers/specs/2026-10-10-gera-pdf-editor-avaliacao-1.0.md)).

### Adicionado
- **Links** funcionam: o cursor vira a mão e a dica mostra o destino.
  - Link interno: vai à página e ao ponto.
  - Endereço web: abre o **Aviso de segurança** (Permitir/Bloquear), com "Lembrar desta ação para
    este site" e o site real em destaque.
  - Outro PDF: pergunta antes e abre na página pedida.
  - Programas e endereços não permitidos são bloqueados.
- **Vista anterior/seguinte** (`Alt+←` / `Alt+→`) e **Ir para página** (`Ctrl+Shift+N`).
- Menu **Exibir → Navegação de páginas**; `←`/`→`, `Ctrl+PgUp/PgDn` e `Ctrl+Shift+PgUp/PgDn` em
  qualquer modo.
- **Tela cheia** como no Acrobat (`Ctrl+L`): uma página por vez, clique avança, botão direito volta,
  barra que some, `Esc` sai e o modo de antes volta.
- **Modo leitura** (`Ctrl+H`): só as páginas, com uma barra flutuante.
- **Zoom**:
  - campo editável ("150" e Enter) e lista de níveis de 8,33 % a 6400 %;
  - Ajustar à altura, Ajustar ao conteúdo visível (`Ctrl+3`) e Zoom para (`Ctrl+Y`);
  - acima do teto da GPU, a página é desenhada em ladrilhos nítidos.
- **Visualização inicial** do arquivo: painel, modo de página, página e zoom de abertura. A tela
  cheia pedida pelo arquivo pergunta antes.

### Corrigido
- Arquivo → Exportar PDF e Assinar → Preencher e assinar / Certificados apareciam desativados.

### Segurança
- Os botões do aviso só aceitam clique depois de um instante (o clique que abriu o aviso não cai em
  Permitir).
- Endereço ambíguo nunca é lembrado.
- Marcas de direção e controles são escondidos.
- Endereços e caminhos gigantes são recusados.

## [0.12.0] — 2026-10-10 · Comparar arquivos (v5f)

### Adicionado
- **Comparar arquivos**: escolhe o arquivo antigo (o da aba ativa entra sozinho) e o novo, as
  páginas de cada um, trocar os dois, "Comparar somente texto" e "Incluir formatação"; roda em
  segundo plano com progresso e Cancelar.
- O resultado é um PDF, como o relatório do Acrobat:
  - resumo com a contagem por tipo, a legenda e o link para a primeira alteração;
  - páginas lado a lado, com o antigo à esquerda e o novo à direita;
  - cada alteração marcada na cor do tipo: Substituído, Inserido, Excluído, Formatação e Imagem;
  - páginas inseridas ou excluídas aparecem sozinhas, com o aviso;
  - marcadores por par de páginas.
- As alterações são comentários travados: aparecem no painel Comentários.
- A linha **Comparação** traz "Alteração i de N", **Anterior** (`Ctrl+,`) e **Próxima** (`Ctrl+.`).
- As páginas formam par pelo texto que as distingue; o cabeçalho e o rodapé repetidos não
  embaralham os pares.

### Segurança
- Arquivo protegido contra cópia é recusado.
- O resultado não grava por cima dos arquivos comparados.
- Há tetos de palavras, letras, imagens decodificadas e conteúdo copiado, contra PDFs maliciosos.

## [0.11.2] — 2026-10-10 · Redação de desenhos

### Corrigido
- Por padrão, todo desenho que passa sob a marca (texto convertido em curvas, assinatura,
  traços) sai inteiro ao aplicar a redação — antes, o que não estava totalmente coberto ficava
  no arquivo, escondido sob a caixa. Nas Propriedades há a opção de preservar desenhos que só
  tocam a marca (bordas de tabela).

## [0.11.1] — 2026-10-10 · Correções de segurança da redação

### Corrigido
- "Aplicar e salvar" só grava depois da confirmação da própria redação (antes, outra operação em
  andamento podia disparar a gravação).
- O texto alternativo (/ActualText, /Alt) também sai de formulários dentro de formulários, das
  propriedades nomeadas deles e da árvore de estrutura (PDF marcado) das páginas redigidas.

## [0.11.0] — 2026-10-10 · Redigir (v5e)

### Adicionado
- **Redigir**: marcar texto (arrastando) ou área; **Localizar texto e redigir** por frase ou
  padrões (CPF e CNPJ com dígito verificador, telefone, e-mail, cartão de crédito com Luhn, CEP,
  data); propriedades (cor da caixa e texto sobreposto).
- **Aplicar** (com confirmação) apaga de vez o texto, os pixels das imagens e os desenhos cobertos,
  e também o que repetiria o conteúdo: campos de formulário e comentários sob as marcas, texto
  alternativo (/ActualText), miniatura e dados privados da página. Marcas desfeitas não são
  aplicadas; salvar com marcas pendentes pergunta antes, e "Aplicar e salvar" só grava se a
  redação der certo.
- **Limpar documento**: metadados (do documento e das páginas), anexos, scripts e ações,
  comentários.

## [0.10.0] — 2026-10-10 · Marca d'água e Cabeçalho e rodapé (v2c)

### Adicionado
- **Marca d'água**: texto (fonte, negrito, tamanho, cor), imagem ou página de outro PDF; rotação,
  opacidade, escala relativa à página, atrás ou na frente do conteúdo, posição e páginas (todas,
  intervalo, pares, ímpares), com prévia. Adicionar (pergunta se substitui a existente),
  Atualizar e Remover — remove também as marcas feitas pelo Acrobat.
- **Cabeçalho e rodapé**: seis caixas, fonte e margens, número da página ("1", "1 de n",
  "Página 1 de n"), data e **numeração Bates** (prefixo, sufixo, dígitos, início), com prévia,
  Atualizar e Remover.
- A marca é um único objeto no arquivo, desenhado em todas as páginas; o caminho do arquivo de
  origem não é gravado no PDF.

## [0.9.0] — 2026-10-10 · Editar PDF: texto (v4b)

### Adicionado
- **Editar PDF** mostra o contorno dos blocos de texto; clicar abre a edição no lugar (Esc ou clicar
  fora confirmam, Ctrl+Z desfaz).
- O texto novo sai na mesma posição, tamanho, cor e direção (também em páginas giradas). A fonte
  original é reaproveitada quando tem todas as letras; senão, a padrão equivalente (Helvetica, Times
  ou Courier, com negrito e itálico).
- Parágrafos refluem na própria largura, com o recuo da primeira linha; linha solta (título) cresce
  até a margem.
- **Adicionar texto** cria um bloco novo onde se clica.
- Só as letras do bloco saem: linhas vizinhas, imagens, desenhos, links e comentários ficam; texto
  invisível de OCR continua invisível. Texto que não dá para trocar com segurança (formulário, fonte
  especial) é recusado sem mexer no documento.

## [0.8.0] — 2026-10-10 · Editar PDF: imagens (v4a)

### Adicionado
- Ferramenta **Editar PDF** com contorno das imagens da página.
- Mover (arrastar ou setas; `Shift` move 10 pt), redimensionar mantendo a proporção, girar 90°,
  substituir, adicionar e excluir imagens, com desfazer e refazer.
- A ordem de empilhamento é preservada; a imagem excluída ou substituída sai do arquivo ao salvar.
- Imagens repetidas, dentro de formulários ou embutidas aparecem com contorno, mas bloqueadas.
- Recusa em documentos assinados ou sem permissão de alteração.

## [0.7.0] — 2026-10-10 · Converter, OCR, proteger e comprimir (v5)

### Adicionado
- **Proteger PDF**: senha de abertura e de permissões (AES-256); as permissões de documentos
  protegidos são respeitadas em todo o app.
- **Comprimir PDF**: níveis baixa, média e alta.
- **Criar PDF** a partir de imagens (respeitando a orientação EXIF) e **Exportar PDF** para Word
  (.docx), texto e imagens.
- **Digitalizar e OCR** com Tesseract (português e inglês), deixando o texto pesquisável.

## [0.6.0] — 2026-10-10 · Formulários e assinaturas (v3)

### Adicionado
- Preencher formulários (texto, caixas de seleção, botões de opção, listas).
- **Preencher e assinar**: texto, marcas, assinatura e iniciais (digitadas, desenhadas ou de imagem).
- **Preparar formulário**: criar e editar campos.
- **Certificados**: assinatura digital com PKCS#12 ou ID digital criado no app, e validação.

## [0.5.0] — 2026-10-09 · Organizar páginas (v2)

### Adicionado
- **Organizar páginas**: reordenar, girar, excluir, extrair; inserir páginas; recortar margens.
- **Dividir PDF** por número de páginas, tamanho máximo ou marcadores; **Combinar arquivos**.

## [0.4.0] — 2026-10-09 · Anotações e comentários (v1.1-B2)

### Adicionado
- Anotações editáveis (mover, redimensionar, cor, espessura, opacidade).
- Caixa de texto, retângulo, elipse, linha, seta e carimbos.
- Painel **Comentários** com respostas, status, filtro, ordem e busca.

## [0.3.0] — 2026-10-09 · Texto, impressão e propriedades (v1.1-B1)

### Adicionado
- Seleção e cópia de texto, impressão e propriedades do documento.

## [0.2.0] — 2026-10-09 · Interface no estilo do Acrobat (v1.1-A)

### Adicionado
- Abas de documentos, Início, central de Ferramentas, painéis laterais, menus e atalhos do Acrobat.

## [0.1.0] — 2026-10-09 · Visualizador (v1)

### Adicionado
- Visualizador leve: abrir, rolar, zoom, miniaturas, busca, destaque, sublinhado, notas e desenho.

[Não lançado]: https://github.com/junior9283/gera-pdf-editor/compare/v0.11.2...HEAD
[0.11.2]: https://github.com/junior9283/gera-pdf-editor/compare/v0.11.1...v0.11.2
[0.11.1]: https://github.com/junior9283/gera-pdf-editor/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/junior9283/gera-pdf-editor/releases/tag/v0.1.0
