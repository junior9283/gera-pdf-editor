# v6c: Propriedades, Marcadores e Anexos (design)

Terceira etapa da avaliação para a 1.0. Cobre os itens D01, D02, P02, P03 e S07 (anexos). Os demais itens
dos painéis (P01 miniaturas com menu e arrastar, P04 Camadas, P05 Destinos, O01/V02 rótulos de página, O02
mover páginas entre PDFs) ficam para a **v6c2**, para a etapa caber num ciclo. Spec aprovada pelo próprio
Claude (autonomia total, ver CLAUDE.md).

## O que o Acrobat faz (referência)

- **Propriedades do documento (`Ctrl+D`)**, com estas abas:
  - **Descrição:** Título, Autor, Assunto e Palavras-chave são editáveis; o resto é só leitura.
  - **Segurança:** resumo da proteção e das permissões.
  - **Fontes:** fontes usadas, com tipo e se estão embutidas.
  - **Visualização inicial:**
    - painel ao abrir: só a página, Marcadores, Miniaturas ou Anexos;
    - layout e ampliação;
    - página de abertura;
    - opções de janela: redimensionar para a página, centralizar, tela cheia, mostrar o título do
      documento, ocultar barras.
  - **Personalizado:** propriedades extras, cada uma com nome e valor.
  - **Avançado:** idioma, além da escala de impressão, duplex, cópias e intervalo de páginas pedidos.
  - O Acrobat grava o `/Info` e os metadados XMP juntos.
- **Painel Marcadores:**
  - **Novo marcador (`Ctrl+B`):** entra depois do selecionado, com o texto selecionado na página como
    título e a vista atual como destino.
  - **Edição:** renomear (`F2`, duplo clique), excluir (`Delete`), arrastar para reordenar e aninhar,
    expandir e recolher (`→`/`←`), e Propriedades (cor, negrito, itálico, destino).
  - Clicar vai ao destino.
- **Painel Anexos:** lista os arquivos anexados ao documento, com nome, descrição, tamanho e datas.
  - Ações: Abrir (com aviso de segurança), Salvar, Adicionar, Excluir e editar a Descrição.
  - Os anexos de comentário aparecem com a página deles.

## Decisões

### 1. Propriedades (pdf-core `properties_edit.rs`)

- **Leitura:** `PdfDoc::properties()` passa a trazer também:
  - **`custom`:** as chaves do `/Info` fora das padrão, com valor string.
  - **`initial`:** o `InitialView` da v6a, mais `window: WindowOptions`. As opções de janela são
    `fit_window`, `center_window`, `display_title`, `hide_menubar`, `hide_toolbar` e `hide_window_ui`.
  - **`lang`:** o `/Lang` do catálogo.
  - **`print`:** `PrintPrefs { scaling_none, duplex: Option<Duplex>, copies: Option<u32>, ranges }`,
    tirado de `/ViewerPreferences`.
  - **`fonts`:** `Vec<FontInfo { name, kind, embedded, subset }>`, com as fontes de `/Resources /Font`
    de todas as páginas e dos XObjects de formulário. A varredura tem teto de 10 000 fontes e de 10
    níveis de XObject.
  - **`security`:** um resumo, que já existe (`SecurityInfo`).
- **Gravação:** `PdfDoc::set_properties(&PropertiesEdit)` é uma etapa desfazível (snapshot, como as
  operações de páginas).
  - Grava no `/Info` a descrição (Título, Autor, Assunto, Palavras-chave) e o personalizado. Uma
    propriedade vazia apaga a chave. O personalizado não aceita nomes padrão (Title, Author, Subject,
    Keywords, Creator, Producer, CreationDate, ModDate, Trapped); os nomes são limitados a 127 bytes.
  - Grava no catálogo `/PageMode`, `/PageLayout`, `/OpenAction` (destino `[pág /Fit|/FitH|/XYZ]`), `/Lang`
    e `/ViewerPreferences` (`FitWindow`, `CenterWindow`, `DisplayDocTitle`, `HideMenubar`,
    `HideToolbar`, `HideWindowUI`, `PrintScaling`, `Duplex`, `NumCopies`, `PrintPageRange`). As
    chaves desligadas saem.
  - **XMP:** grava um pacote XMP novo em `/Metadata` com `dc:title`, `dc:creator`, `dc:description`,
    `pdf:Keywords`, `pdf:Producer`, `xmp:CreatorTool`, `xmp:CreateDate` e `xmp:ModifyDate`. O texto é
    escapado como XML.
    - Se o XMP antigo tinha `pdfaid:part`/`conformance` ou `pdfuaid:part`, eles são mantidos (a
      declaração de PDF/A e PDF/UA continua).
    - Ruling: outros esquemas XMP próprios do arquivo se perdem ao editar as propriedades.
- **Interface (`shell/properties.rs`):** a janela vira um diálogo com abas e OK/Cancelar. Ela edita uma
  cópia e manda `Request::SetProperties` no OK.
  - O app respeita "Mostrar o título do documento": a aba e o título da janela usam o Título.
  - As opções Ocultar barras e Centralizar valem para outros leitores; o app as grava, mas não esconde a
    própria interface. Ruling: igual ao modo leitura, que o usuário já tem.

### 2. Marcadores (pdf-core `outline.rs`)

- **Leitura:** `PdfDoc::outline_tree()` devolve uma `Vec<OutlineNode>` em ordem de exibição, com estes
  campos:
  - `id` (número do objeto), `parent: Option<i32>`, `depth` e `title`;
  - `page: Option<i32>` e `at: Option<(f32, f32)>`;
  - `open` (sinal de `/Count`);
  - `color: Option<[f32; 3]>`, `bold` e `italic` (`/C`, `/F`).
- **Proteções da leitura:**
  - um ciclo é cortado com um conjunto de visitados;
  - teto de 50 000 nós e de profundidade 64;
  - um título é cortado em 512 caracteres.
- **Operações** (`OutlineOp`), cada uma uma etapa desfazível (snapshot):
  - `Add { parent, after, title, page, at }`, que devolve o id;
  - `Rename { id, title }`;
  - `Delete { id }`, que leva os filhos;
  - `Move { id, parent, after }`, que recusa mover para dentro de si mesmo;
  - `Style { id, color, bold, italic }`;
  - `SetDest { id, page, at }`;
  - `SetOpen { id, open }`, que não entra no desfazer, como no Acrobat.
- **Regravação:** a árvore é montada em memória e as ligações são regravadas (`/First`, `/Last`, `/Next`,
  `/Prev`, `/Parent` e `/Count`) a partir dela. Cada item mantém o próprio dicionário e as outras
  chaves, como `/A` e `/SE`. Uma árvore quebrada (ciclo) sai consertada.
- **`DocSummary.outline`** passa a usar `OutlineNode`, e o `OutlineItem` sai.
- **Interface (`sidebar.rs`, painel renomeado "Marcadores", como no Acrobat):**
  - barra do painel: Novo marcador, Excluir, Expandir tudo e Recolher tudo;
  - itens com triângulo de expandir, cor e estilo;
  - um clique seleciona e vai ao destino; duplo clique ou `F2` renomeia no próprio lugar;
  - menu de contexto: Novo marcador, Renomear, Excluir, Definir destino (vista atual) e
    Propriedades… (cor, negrito, itálico);
  - teclas no painel focado: `↑`/`↓` selecionam, `→`/`←` expandem e recolhem, `Enter` vai, `Delete`
    exclui e `F2` renomeia;
  - `Ctrl+B` cria um marcador no documento, com o texto selecionado (até 128 caracteres) ou "Sem
    título", depois do selecionado (ou no fim), com destino na página e no ponto do topo da vista;
  - **Arrastar:** o terço de cima de um item põe antes dele, o do meio põe como último filho e o de baixo
    põe depois dele. Uma linha mostra onde o item vai cair.
  - O estado expandido de cada item é do arquivo (`open`).

### 3. Anexos (pdf-core `attachments.rs`)

- **`PdfDoc::attachments() -> Vec<AttachmentInfo>`:**
  - campos: `name`, `filename`, `description`, `size`, `created`, `modified` e
    `origin: Document | Comment { page, id }`;
  - os do documento vêm da árvore `/EmbeddedFiles`;
  - os de comentário vêm dos `FileAttachment` das páginas.
- **Operações** (cada uma uma etapa desfazível):
  - `add_attachment(path, description)`: até 100 MB, nome único ("arquivo (2).pdf");
  - `remove_attachment(name)`;
  - `set_attachment_description(name, text)`.
- **`attachment_bytes(origin, name) -> Vec<u8>`:** teto de 512 MB.
- **Interface:** um painel novo, "Anexos" (📎 no trilho).
  - Lista com nome, descrição, tamanho e página (no caso de comentário).
  - **Abrir:**
    - PDF: o aviso de segurança "O documento está tentando abrir o anexo: <nome>" (mesmo atraso dos
      botões). Com Permitir, o anexo é gravado na pasta privada do app (a da impressão) e abre numa aba
      nova, marcado como anexo (Salvar pede o destino).
    - Outros tipos: o app não abre programas. O botão vira "Salvar…", com a mensagem "Por segurança, só
      anexos PDF abrem aqui; salve o arquivo para abri-lo".
  - **Salvar…:** seletor de destino, com o nome do anexo como padrão.
  - **Adicionar arquivo…** e **Excluir:** só nos anexos do documento, e com permissão de alterar.
  - **Descrição:** edição no lugar.
  - O painel abre sozinho com `/PageMode /UseAttachments`.

## Segurança

- **Leituras com teto:** fontes, nós de marcador e tamanho de anexo. Um anexo maior que o teto não é
  aberto nem salvo pelo app.
- **Anexo aberto:**
  - só PDF;
  - depois do aviso;
  - a partir de uma cópia na pasta privada (0700), varrida no início do app.
- **Nome do arquivo ao salvar:** só o nome final (sem `/`, sem `..`), saneado como no Dividir
  (`safe_file_name`).
- **Permissões:** com o documento protegido contra alteração, as edições de propriedades, marcadores e
  anexos são recusadas (`require_modifiable`), como as outras.

## Testes

- **pdf-core:**
  - `tests/properties_edit.rs`:
    - descrição e personalizado gravados e relidos;
    - nomes padrão recusados no personalizado;
    - visualização inicial e preferências de impressão de ida e volta;
    - XMP com o título novo, mantendo `pdfaid`;
    - o desfazer volta tudo;
    - fontes listadas (embutida ou subconjunto).
  - `tests/outline.rs`:
    - leitura com estilo;
    - adicionar, renomear, excluir (com filhos), mover (também para filho, e recusa para dentro de si);
    - estilo, destino e aberto;
    - árvore com ciclo lida e consertada;
    - desfazer;
    - `/Count` correto depois das operações.
  - `tests/attachments.rs`:
    - listar (documento e comentário);
    - adicionar com nome único, remover, descrição;
    - bytes de volta iguais;
    - teto;
    - desfazer.
- **gera-ui:**
  - `Ctrl+B`;
  - rascunho das propriedades (Cancelar descarta);
  - regra do arrastar dos marcadores (terços);
  - política de abrir anexo (PDF pede aviso, o resto só salva);
  - nome saneado.
- **E2E:**
  - editar o título e reabrir;
  - criar, renomear, arrastar e excluir marcadores;
  - adicionar um anexo, abrir um PDF anexado com o aviso e salvar um anexo.
