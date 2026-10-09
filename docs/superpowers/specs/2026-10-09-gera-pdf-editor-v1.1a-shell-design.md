# Gera PDF Editor — Subprojeto 1.1-A: Casca do Acrobat (menus, abas, painéis e exibição)

Data: 2026-10-09 · Status: aguardando revisão

## 1. Objetivo

A meta de longo prazo agora é cobrir os recursos do **Adobe Acrobat Pro DC**, com as mesmas barras,
abas e painéis, num app **original**: nenhum código, binário ou recurso visual da Adobe é usado.
Este spec trata só da **casca** da interface. Ela substitui o modelo atual de "uma janela por
documento" e serve de base para encaixar as fases seguintes.

Também resolve um problema da v1: **não há como abrir um arquivo pela interface**. Não existe botão
nem Ctrl+O, e arrastar e soltar não funciona no Wayland, porque o winit 0.30 não implementa
soltar arquivos nesse backend.

### Critérios de sucesso

- Abrir PDFs pelo menu, pelo `Ctrl+O`, pelo botão, pela aba Início (recentes) e por arrastar
  (em X11).
- Vários documentos em abas na mesma janela, cada um com zoom, rolagem, busca e anotações próprios.
- Barra de menus, barra de ferramentas, trilho e painéis laterais e hub de Ferramentas, no arranjo
  do Acrobat DC.
- Quatro modos de exibição, girar a vista, ajustar à largura e à página, e tela cheia.
- Nenhuma perda silenciosa de anotações: fechar com alterações pendentes pergunta antes.
- As metas da v1 continuam valendo: abertura < 300 ms, memória < 150 MB (medida agora com
  **3 abas** de 100 páginas), binário < 25 MB e rolagem sem travar.

### Fora de escopo

- **v1.1-B:** selecionar e copiar texto, imprimir, propriedades do documento, novas anotações
  (caixa de texto, formas, riscado, carimbos, respostas).
- **Fases seguintes:** v2 (organizar páginas), v3 (formulários e assinatura), v5 (converter,
  OCR, proteger, comprimir) e v4 (editar texto e imagens), nessa ordem.
- **Fora de alcance:** Adobe Sign e Document Cloud, JavaScript do Acrobat completo, 3D e
  multimídia, Preflight e PDF/A completo.

### Decisões já tomadas

- Casca fiel ao Acrobat DC, com abas fixas **Início** e **Ferramentas** além das abas de
  documento (opção A dos mockups).
- O hub de Ferramentas mostra o **catálogo completo**. O que ainda não existe aparece acinzentado
  com "Em breve · vN".
- Uma janela, N abas, **um `Worker` (thread do MuPDF) por aba de documento**.

## 2. Arquitetura

As 3 crates continuam. O `pdf-core` muda pouco (só a rotação no render). A maior parte da
mudança fica no `gera-ui`, e a lógica pura nova vai para o `app-state`.

```
GeraApp (gera-ui/app.rs: só a casca)
├── tabs: Vec<Tab>          // Tab::Home | Tab::Tools | Tab::Doc(DocTab)
├── active: usize
├── recents: RecentFiles    // app-state
├── prefs: Prefs            // tema, painéis abertos, larguras
└── open_dialog: OpenDialog // pedidos ao seletor de arquivos em outra thread

DocTab (gera-ui/doc_tab.rs: tudo o que hoje está no GeraApp e é de um documento)
├── worker: Worker          // thread própria; fechar a aba encerra a thread
├── view, edit, search, outline, saved_annots, password_prompt
└── view_mode, rotation, fit (largura | página | nenhum)
```

**Por que um `Worker` por aba:** o `Document` do MuPDF não é `Send`. Um worker por aba mantém o
protocolo atual intacto e isola documentos. A alternativa, um worker para todas as abas com um id
de documento em cada mensagem, complicaria cancelamento e prioridades sem ganho.

**Módulos novos ou alterados:**

| Crate | Módulo | Responsabilidade |
|---|---|---|
| app-state | `tabs.rs` | regras de abas: abrir duplicado ativa a existente, fechar a ativa escolhe a vizinha, as fixas não fecham |
| app-state | `recents.rs` | lista de recentes: ordem, limite de 20, sem duplicatas, (des)serialização tolerante |
| app-state | `layout.rs` | um retângulo por página em 4 modos de exibição, considerando a rotação |
| app-state | `zoom.rs` | acrescenta `fit_page` |
| app-state | `transform.rs` | conversão tela ↔ PDF considerando zoom, deslocamento e rotação |
| app-state | `tools_catalog.rs` | tabela constante do catálogo de ferramentas (nome, categoria, ícone, status) |
| pdf-core | `doc.rs`/`worker.rs` | `render` e `Request::Render` ganham `rotation` |
| gera-ui | `doc_tab.rs` | estado e desenho de um documento (extraído do `app.rs`) |
| gera-ui | `shell/menus.rs` | barra de menus |
| gera-ui | `shell/tabs.rs` | barra de abas |
| gera-ui | `shell/toolbar.rs` | barra de ferramentas e linha "Comentar" |
| gera-ui | `shell/home.rs` | aba Início |
| gera-ui | `shell/tools_hub.rs` | aba Ferramentas e painel direito |
| gera-ui | `shell/panels.rs` | trilho esquerdo e painéis laterais (reaproveita o `sidebar.rs`) |
| gera-ui | `open_dialog.rs` | seletor de arquivos (`rfd`, xdg-portal) em outra thread |
| gera-ui | `prefs.rs` | preferências em `~/.config/gera-pdf-editor/` |

## 3. Funcionalidades

### 3.1 Barra de menus

Desenhada dentro da janela, porque o egui não tem menu nativo. Fica igual em X11 e Wayland. O
atalho aparece ao lado de cada item. Os itens indisponíveis ficam desativados, com a fase em que
chegam.

| Menu | Ativos na v1.1-A | Desativados |
|---|---|---|
| Arquivo | Abrir… `Ctrl+O`, Abrir recente ▸, Salvar `Ctrl+S`, Salvar como… `Ctrl+Shift+S`, Fechar aba `Ctrl+W`, Sair `Ctrl+Q` | Propriedades, Imprimir (v1.1-B), Exportar (v5) |
| Editar | Desfazer `Ctrl+Z`, Refazer `Ctrl+Shift+Z`, Localizar `Ctrl+F`, Preferências (tema) | Copiar, Selecionar tudo (v1.1-B) |
| Exibir | Zoom (ampliar, reduzir, 100%, ajustar à largura, ajustar à página), Exibição de página (4 modos), Girar vista (horário `Ctrl+Shift++`, anti-horário `Ctrl+Shift+-`), Tela cheia `F11`, Painéis (miniaturas, índice, anotações, ferramentas), Modo escuro | — |
| Assinar | — | tudo (v3) |
| Janela | lista de abas, Próxima aba `Ctrl+Tab`, Aba anterior `Ctrl+Shift+Tab`, Fechar todas | — |
| Ajuda | Atalhos de teclado (janela com a lista), Sobre (versão, AGPL, MuPDF, avisos de terceiros) | — |

Os atalhos ignoram o teclado quando o foco está num campo de texto, como o Ctrl+Z já faz na v1.
"Salvar como…" grava as anotações pendentes num novo caminho escolhido no seletor, e a aba passa
a apontar para ele.

### 3.2 Barra de abas

- Ordem: **Início** e **Ferramentas** fixas à esquerda, sem ✕. Depois as abas de documento,
  com o nome do arquivo, `•` quando há alterações pendentes e ✕. Por fim **＋**, que abre o
  seletor.
- Abrir um arquivo **já aberto** só ativa a aba dele, comparando caminhos canônicos.
- Fechar a aba ativa ativa a vizinha da direita, ou a da esquerda se não houver. Fechar a última
  aba de documento volta para Início.
- Nome muito longo é truncado com "…", e o caminho completo aparece na dica ao passar o mouse.

### 3.3 Barra de ferramentas (só em abas de documento)

- À esquerda: 📂 Abrir · 💾 Salvar · 🖨 Imprimir (desativado) · 🔍 Buscar.
- Centro: ◀ campo da página / total ▶ · − zoom % ＋, com menu para ajustar à largura ou à página.
- À direita: **✏ Comentar** e **✍ Assinar** (desativado).
- **Comentar** liga e desliga uma segunda linha com as ferramentas da v1 (destaque, sublinhado,
  nota, tinta) e desfazer/refazer. Os botões de anotação da barra atual vão para lá.

### 3.4 Modos de exibição, zoom e rotação

O layout passa de "uma coluna com `page_top`" para **um retângulo (x, y, largura, altura) por
página**, nas dimensões já giradas:

- **Rolagem contínua:** o comportamento atual, centralizado.
- **Duas páginas contínuas:** pares 1–2, 3–4…, cada linha com a altura da maior página do par.
  "Mostrar capa" (página 1 sozinha) fica desligado por padrão.
- **Página única / Duas páginas:** mostram só a página ou o par atual. Rolar além do fim (ou
  PageDown, setas, espaço) passa para o próximo, e o contrário volta.
- A virtualização (`visible_range`) e o pedido sob demanda de render continuam iguais.

**Zoom:** "Ajustar à página" entra ao lado do `fit_width`. Enquanto um modo de ajuste estiver
ativo, redimensionar a janela recalcula o zoom. Mudar o zoom manualmente desliga o ajuste.

**Girar vista (0°, 90°, 180°, 270°, por aba):** só a tela, sem alterar o arquivo. O
`Request::Render` recebe `rotation`, aplicada à matriz do MuPDF. Largura e altura trocam a 90° e
270°. Toda conversão entre tela e página (clique, anotação, destaque de busca, palavras numa
área) passa por `transform.rs`. Assim, anotar com a vista girada grava no lugar certo do PDF.

**Tela cheia (F11):** esconde menus, abas, barras e painéis. `Esc` ou `F11` sai.

### 3.5 Painéis

- **Trilho esquerdo** (sempre visível): ▤ Miniaturas, ☰ Índice, 💬 Anotações. O ícone abre ou
  fecha o painel ao lado, que reaproveita o conteúdo do `sidebar.rs` da v1.
- **Painel direito, Ferramentas:** lista resumida do catálogo. Item ativo (Comentar) liga a linha
  de ferramentas. Item "Em breve" fica desativado.
- Larguras arrastáveis. Painéis abertos e larguras ficam salvos nas preferências.

### 3.6 Aba Início

- Botão grande **"Abrir arquivo…"** e a lista de **Recentes**: nome, pasta e data em que foi
  aberto.
- Clicar num recente abre o PDF, ou ativa a aba se ele já estiver aberto.
- Um recente que não existe mais aparece riscado, com "Remover da lista".
- Sem arquivo na linha de comando, o app abre na aba Início.

### 3.7 Aba Ferramentas (hub)

- Grade de cartões por categoria: **Criar e editar**, **Revisar e aprovar**, **Proteger e
  padronizar**, **Organizar**.
- Catálogo único em `tools_catalog.rs`, usado pelo hub, pelo painel direito e pelos menus. Cada
  fase futura só muda o status de um item.
- Ativo na v1.1-A: **Comentar**. Com um documento ativo, vai para a aba dele e liga a linha de
  ferramentas. Sem documento, abre o seletor primeiro.
- Os demais cartões ficam acinzentados com "Em breve · v2/v3/v4/v5".

### 3.8 Abrir arquivos

- **Seletor:** crate `rfd` com o backend **xdg-portal** (sem GTK, para manter o app sem runtime
  pesado). Roda numa thread separada para não travar a janela, e o resultado volta por canal.
  Filtro `*.pdf`, várias seleções, e cada arquivo vira uma aba.
- **Arrastar e soltar:** continua, e funciona em X11. Cada arquivo solto vira uma **aba nova** (a
  v1 abria um processo novo). No Wayland continua sem efeito até o winit implementar o recurso.
- **Linha de comando:** cada argumento vira uma aba.
- Cada abertura bem-sucedida entra nos recentes.

## 4. Fluxo de dados

1. Um pedido de abrir (menu, atalho, ＋, Início, hub, soltar ou linha de comando) gera um
   caminho. As regras de abas decidem se ativam uma aba existente ou criam uma `DocTab` com um
   `Worker` novo.
2. A `DocTab` envia `Request::Open`. Até `Response::Opened`, a aba mostra "Abrindo…". Se a
   abertura falhar, a aba é descartada e o erro vai para a barra de status.
3. A cada quadro, `GeraApp` despacha a entrada para a aba ativa e desenha a casca. Cada
   `DocTab` drena **só o seu** canal de respostas.
4. **Abas inativas** liberam texturas e cache de render e guardam só tamanhos, zoom, modo,
   rotação, rolagem e anotações pendentes. Ao reativar, as páginas visíveis são pedidas de novo.
5. Recentes e preferências são lidos na inicialização e gravados quando mudam.

## 5. Erros e segurança

- **Fechar com alterações pendentes** (aba, Fechar todas, Ctrl+Q ou janela) mostra "Salvar
  alterações em *arquivo*?" com Salvar, Não salvar e Cancelar. Com várias abas pendentes, pergunta
  uma por vez. Se o salvamento falhar, a aba fica aberta.
- **Seletor indisponível** (portal ausente ou erro): "Não foi possível abrir o seletor de
  arquivos" na barra de status.
- **Senha:** a janela de senha da v1, presa à aba que a pediu.
- **Recentes e preferências:** gravados de forma segura (temporário, fsync e rename). Um JSON
  corrompido ou ilegível vira valor padrão, sem travar e sem apagar o arquivo antes de gravar um
  válido.
- **Caminhos:** recentes guardam caminhos canônicos. Os nomes são só exibidos como texto, nunca
  interpretados.
- O salvamento seguro e a preservação da criptografia da v1 não mudam.

## 6. Testes

**Unitários (`app-state`):**
- `tabs`: abrir duplicado, fechar a ativa (vizinha à direita ou à esquerda), fechar a última
  volta para Início, fixas não fecham.
- `recents`: ordem, limite de 20, duplicata sobe para o topo, JSON vazio, corrompido ou com
  campos extras.
- `layout`: os 4 modos, par com páginas de tamanhos diferentes, número ímpar de páginas, capa,
  `visible_range` em duas colunas e dimensões giradas.
- `zoom`: `fit_page` em página retrato e paisagem.
- `transform`: ida e volta tela → PDF → tela nos 4 ângulos, com zoom diferente de 1.
- `tools_catalog`: ids únicos e todo item com categoria e status.

**`pdf-core`:** render a 90° troca largura e altura, e a 0° continua igual ao da v1.

**Interface:** roteiro manual executando o app: abrir por cada caminho, abas duplicadas, fechar
com pendências, os 4 modos, girar e anotar, tela cheia, painéis e recentes com arquivo apagado.

**Metas:** o `scripts/measure.sh` passa a medir memória com 3 abas abertas, mais tamanho do
binário (com o `rfd`) e tempo de abertura.

## 7. Riscos

1. **Tamanho do `rfd` + xdg-portal** (traz `zbus` e um executor async). Medir logo na primeira
   tarefa. Se estourar a meta, a alternativa é chamar o portal direto via D-Bus, ou cair para
   `zenity`/`kdialog` quando presentes.
2. **Transformação de coordenadas com rotação:** é a parte com mais chance de bug. Os testes de
   `transform.rs` vêm antes de ligar a rotação na interface.
3. **Divisão do `app.rs` em `DocTab`:** é uma refatoração grande. Ela é feita primeiro, sem mudar
   comportamento, e validada com o app rodando antes de qualquer recurso novo.
