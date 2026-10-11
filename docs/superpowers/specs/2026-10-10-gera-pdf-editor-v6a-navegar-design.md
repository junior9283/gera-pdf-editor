# v6a: Navegar e exibir (design)

Esta é a primeira etapa da avaliação para a 1.0 (ver `2026-10-10-gera-pdf-editor-avaliacao-1.0.md`). Ela cobre
os itens V01–V05, V07, V08, V18, M01 e M06 dessa análise.

A spec foi aprovada pelo próprio Claude, com a autonomia total que o CLAUDE.md dá.

## O que o Acrobat faz (referência)

- **Links.** Com Selecionar ou Mão, o cursor vira a mão de apontar sobre o link e mostra o endereço numa
  dica. Ao clicar:
  - destino interno: vai à página e posição de destino;
  - URL: abre o aviso de segurança "O documento está tentando se conectar a…", com **Permitir**,
    **Bloquear** e "Lembrar desta ação para este site";
  - outro PDF (`GoToR`): abre o arquivo;
  - programa (`Launch`): bloqueado.
- **Navegação de páginas.** Fica em Exibir → Navegação de páginas, com: Primeira, Anterior, Próxima,
  Última, Ir para página (`Ctrl+Shift+N`), Vista anterior (`Alt+←`) e Vista seguinte (`Alt+→`).
  - `←`/`→` e `Ctrl+PgUp`/`Ctrl+PgDn` mudam de página em qualquer modo.
  - `Shift+Ctrl+PgUp/PgDn` e `Shift+Ctrl+↑/↓` vão para a primeira e a última página.
  - `Enter`/`Shift+Enter` avançam e voltam uma tela.
- **Tela cheia** (`Ctrl+L`):
  - mostra uma página por vez, ajustada à tela;
  - clicar avança; o botão direito volta;
  - `Esc` sai;
  - há uma barra de navegação opcional.
- **Modo leitura** (`Ctrl+H`): esconde as barras e os painéis. Uma barra flutuante semitransparente embaixo
  mostra a página, o zoom e um botão para sair.
- **Zoom:**
  - lista de níveis de 8,33 % a 6400 %;
  - porcentagem digitada no campo;
  - Zoom para… (`Ctrl+Y`);
  - Ajustar à altura;
  - Ajustar ao conteúdo visível (`Ctrl+3`), que amplia até as margens brancas saírem da tela.
- **Visualização inicial** do arquivo: o Acrobat respeita `/PageMode` (marcadores, miniaturas, anexos,
  tela cheia), `/PageLayout` (uma página, contínuo, duas colunas) e `/OpenAction` (página e ampliação de
  abertura).

Fontes: o documento de avaliação (helpx `keyboard-shortcuts`, `navigation`, `adjusting-pdf-views`,
`viewing-pdfs-viewing-preferences`, `security-warnings`).

## Decisões

1. **Links (pdf-core).**
   - `Doc::links(page) -> Vec<LinkInfo>` e `DocSummary.links`, lidos de todas as páginas na abertura,
     na mesma varredura das anotações.
   - `LinkInfo { page, rect: PdfRect, target: LinkTarget }`. Os alvos possíveis:
     - `Page { page, at: Option<(x, y)> }`;
     - `Uri(String)`;
     - `File { path, page: Option<i32> }`;
     - `Blocked(String)`: ação de lançar programa, ou esquema não permitido.
   - A leitura usa o iterador de links do MuPDF que devolve `Result`. O iterador do `Page` faz `unwrap` e
     derrubaria o app num PDF malformado; um link que não se resolve é ignorado.
   - Só os esquemas `http`, `https` e `mailto` viram `Uri`. `javascript:`, `file:` e os demais viram
     `Blocked`.
   - `Launch` para `.pdf` vira `File`; para qualquer outra coisa, `Blocked`.
   - Os caminhos relativos de `File` são resolvidos a partir da pasta do documento.
   - As páginas de destino fora do documento são descartadas.
2. **Links (interface).**
   - Com a ferramenta Selecionar, fora dos modos Organizar, Preparar formulário, Editar PDF e Redigir,
     e sem anotação sob o ponteiro:
     - o cursor vira `PointingHand`;
     - a dica mostra o endereço (a URL, "Página N" ou o nome do arquivo).
   - O clique (sem arrastar) segue o link:
     - **Página:** salva a vista atual no histórico e vai ao destino.
     - **Uri:** abre a janela "Aviso de segurança", com o texto "O documento está tentando se conectar
       a: <url>", os botões **Permitir** e **Bloquear** e a caixa "Lembrar desta ação para este site".
       A escolha lembrada fica em `prefs.link_sites` (host → permitir/bloquear) e vale para todos os PDFs.
       A URL abre no navegador padrão (`ctx.open_url`).
     - **File:** o mesmo aviso, com o texto "O documento está tentando abrir o arquivo: <caminho>" e sem a
       caixa de lembrar. Com Permitir, abre numa aba (ou ativa a aba, se já estiver aberto) e vai à
       página. Se o arquivo não existir, a barra de status diz "Arquivo não encontrado: <nome>".
     - **Blocked:** a barra de status diz "Este link tenta abrir um programa ou endereço não permitido e
       foi bloqueado."
3. **Histórico de vistas** (`app_state::view_history`).
   - Entrada: `{ page, y_in_page, zoom, fit }`.
   - `push(atual)` grava a vista antes de cada salto: link, marcador, miniatura, Ir para página,
     primeira ou última página, resultado da busca, comentário e alteração da comparação. O push limpa o
     "seguinte". O teto é de 100 entradas.
   - `back(atual)` devolve a anterior e põe a atual no "seguinte"; `forward(atual)` faz o inverso.
   - `Alt+←` e `Alt+→`. Uma vista igual à anterior não é empilhada de novo.
4. **Teclado de navegação** (fora de campos de texto e sem objeto selecionado; com objeto selecionado,
   as setas continuam a movê-lo):
   - `←` e `→`: página anterior e próxima, em todos os modos. Nos modos contínuos, rola até o topo da
     página.
   - `Ctrl+PgUp` e `Ctrl+PgDn`: o mesmo.
   - `Shift+Ctrl+PgUp/PgDn` e `Shift+Ctrl+↑/↓`: primeira e última página.
   - `Enter` e `Shift+Enter`: uma tela abaixo ou acima, como `PgDn`/`PgUp`.
   - `Espaço` continua como está; a Mão temporária é da v6b.
5. **Exibir → Navegação de páginas:** Primeira página, Página anterior, Próxima página, Última página, Ir
   para página…, Vista anterior e Vista seguinte, com os atalhos ao lado.
   - Ir para página (`Ctrl+Shift+N`) põe o foco no campo de página da barra, com o número selecionado.
   - Ruling: o Acrobat novo também usa o campo da barra; o diálogo separado é do Acrobat antigo.
6. **Tela cheia** (`Ctrl+L`; `F11` continua como alternativo):
   - força uma página por vez e Ajustar à página, e restaura o modo e o zoom ao sair;
   - clique esquerdo: próxima página; clique direito: anterior; setas, PgUp/PgDn e roda também funcionam;
   - `Esc` sai;
   - uma barra pequena (◀ N/M ▶ ✕) aparece no canto inferior esquerdo quando o mouse se mexe e some
     depois de 2 s parado.
   - Os links continuam clicáveis; o clique num link não avança a página.
7. **Modo leitura** (`Ctrl+H`, também em Exibir):
   - esconde a barra de ferramentas, o painel da esquerda, o painel de ferramentas e as linhas de
     ferramenta. Ficam o menu e as abas, como no Acrobat;
   - uma barra flutuante semitransparente, no centro embaixo, traz ◀ N/M ▶, −, zoom % e +, e ✕ Sair do
     modo leitura;
   - `Ctrl+H` ou `Esc` saem.
   - O estado é do app, não da aba: trocar de aba mantém o modo.
8. **Zoom.**
   - `MAX_ZOOM` passa a 64,0 (6400 %) e `MIN_ZOOM`, a 0,0833 (8,33 %).
   - A lista de níveis, como a do Acrobat: 8,33, 12,5, 25, 33,33, 50, 66,67, 75, 100, 125, 150, 200, 300,
     400, 600, 800, 1200, 1600, 2400, 3200 e 6400. `zoom_in`/`zoom_out` passam a andar por essa lista.
     O Ctrl+roda continua contínuo.
   - Na barra, o campo de porcentagem fica editável: digitar "150" ou "150%" e Enter aplica o zoom; um
     valor inválido volta ao atual. `Ctrl+Y` põe o foco nele.
   - O ⏷ ao lado lista os níveis, mais Ajustar à página, Ajustar à largura, Ajustar à altura e Ajustar ao
     conteúdo visível.
   - `Fit::Height`: a altura da página cabe na janela.
   - `Fit::Visible` (`Ctrl+3`):
     - o worker calcula a caixa do conteúdo da página atual, numa renderização em baixa resolução
       (escala 0,25), procurando pixels não brancos;
     - o zoom faz a largura dessa caixa caber na janela, com 4 % de margem, e a rolagem horizontal vai até
       a borda esquerda dela;
     - continua ajustado se a janela muda de tamanho;
     - página em branco: usa a página inteira.
   - **Ladrilhos de alta resolução.** Acima do teto de textura e de memória (`scale_for` < `page_scale`),
     a página continua desenhada a partir da imagem limitada. Por cima, o canvas pede **ladrilhos**:
     - recortes de 512 × 512 px da página na escala exata, só dos que estão visíveis;
     - cada um é renderizado pelo MuPDF com clip, guardado no mesmo cache LRU e desenhado sobre a base;
     - assim, 6400 % fica nítido sem alocar a página inteira.
9. **Visualização inicial** (`DocSummary.initial_view`):
   - `/PageMode`:
     - `UseOutlines`: abre o painel Marcadores;
     - `UseThumbs`: Miniaturas;
     - `UseAttachments`: ignorado até o painel Anexos (v6c);
     - `FullScreen`: abre em tela cheia.
   - `/PageLayout`:
     - `SinglePage` → Página única;
     - `OneColumn` → Rolagem contínua;
     - `TwoColumnLeft/Right` → Duas páginas contínuas;
     - `TwoPageLeft/Right` → Duas páginas. As variantes `Right` mostram a capa sozinha.
   - `/OpenAction` com destino:
     - vai à página;
     - `/Fit` → Ajustar à página;
     - `/FitH` → largura;
     - `/XYZ` com zoom > 0 → esse zoom.
   - A visualização inicial só vale na primeira abertura do arquivo na sessão. Restaurar a última
     posição lida fica para as Preferências (v6b).
10. **Menus (defeito M01):** Arquivo → Exportar PDF e Assinar → Preencher e assinar / Certificados viram
    itens ativos que abrem a ferramenta (`Command::Tool(id)` → `run_tool`). `soon_tool` só aparece para
    ferramentas em "Em breve"; como não há nenhuma, sai do código.
11. **Ajuda → Atalhos de teclado** passa a listar também as teclas de navegação tratadas fora da tabela:
    setas, PgUp/PgDn, Home/End, Enter, Alt+setas e Esc.

## Segurança

- Uma URL só abre depois do aviso, ou da escolha lembrada para o host. Esquemas fora da lista branca nunca
  abrem.
- `Launch` de programa é sempre bloqueado.
- Um arquivo de `GoToR` ou `Launch` só abre se for `.pdf` (senão vira `Blocked`) e existir, depois do aviso.
  Ele passa pelo mesmo caminho de abertura dos outros documentos (senha, permissões). O caminho vem do PDF,
  que não é confiável, e por isso o aviso mostra o caminho absoluto.
- O host lembrado é comparado em minúsculas e sem porta. Uma URL sem host (`mailto:`) é lembrada pelo
  esquema.

## Fora do escopo (registrado)

- Ferramenta Mão, Zoom com marca, Zoom dinâmico e Lupa: v6b.
- Opções de tela cheia (transições, avanço automático): P2.
- Rótulos de página no campo: v6c.
- Criar e editar links: v6f.
- Links em JavaScript, ações de formulário e `Named` (`NextPage` etc.): só o `Named` simples (`NextPage`,
  `PrevPage`, `FirstPage`, `LastPage`) é seguido, se o MuPDF o resolver para uma página; o resto é
  ignorado.

## Testes

- **pdf-core** (`tests/links.rs`), com PDFs gerados no teste:
  - link para página com `/XYZ` resolve a página e o ponto;
  - destino nomeado resolve;
  - link `URI` http vira `Uri`, e `javascript:` vira `Blocked`;
  - `GoToR` relativo vira `File` com caminho absoluto;
  - `Launch` de `.exe` vira `Blocked`;
  - página de destino inexistente é descartada;
  - link malformado (`/A` sem `/S`) não derruba o app e é ignorado;
  - `initial_view` lê `PageMode`, `PageLayout` e `OpenAction`;
  - `content_box` acha a caixa do conteúdo, e uma página em branco devolve `None`;
  - render com clip devolve o recorte certo.
- **app-state:**
  - `view_history`: push, back e forward, teto, sem duplicata, push limpa o seguinte;
  - níveis de zoom: passo de nível, limites, porcentagem digitada (`parse_zoom`);
  - `link_policy`: esquema e host normalizados, lembrança;
  - `Fit::Height` e `Fit::Visible`;
  - ladrilhos visíveis: a função pura `visible_tiles(page_px, viewport_px)`.
- **gera-ui:**
  - atalhos novos na tabela (`Ctrl+L`, `Ctrl+H`, `Ctrl+Shift+N`, `Alt+←/→`, `Ctrl+3`, `Ctrl+Y`);
  - menus com os itens ativos.
- **E2E em Xvfb:**
  - abrir um PDF com link interno e clicar: a página muda; `Alt+←` volta;
  - clicar num link web: aparece o aviso, e Bloquear não abre nada;
  - `Ctrl+L`: tela cheia e clique avança; `Esc` sai;
  - `Ctrl+H`: modo leitura;
  - digitar 6400 no zoom: a página fica nítida;
  - `Ctrl+3`.
