# v6a: Navegar e exibir. Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (execução nativa, escolhida pelo dono). Os passos usam checkbox (`- [ ]`).

**Objetivo:** seguir links com segurança. Também entram: histórico de vistas, navegação pelo teclado e pelo
menu como no Acrobat, tela cheia e modo leitura, zoom de 8,33 % a 6400 % (nítido, com ladrilhos),
Ajustar à altura e ao conteúdo visível, visualização inicial do arquivo e o defeito dos menus.

**Arquitetura:**
- O `pdf-core` lê os links, a visualização inicial e a caixa de conteúdo, e renderiza ladrilhos.
- O `app-state` guarda as funções puras: histórico, níveis de zoom, política de links e ladrilhos
  visíveis.
- O `gera-ui` liga tudo à aba, à casca e aos menus.

**Tecnologia:** Rust, mupdf 0.8, egui 0.36.

**Spec:** `docs/superpowers/specs/2026-10-10-gera-pdf-editor-v6a-navegar-design.md`

## Restrições globais

- Textos da interface em pt-BR e atalhos iguais aos do Acrobat (tabela da spec).
- Nenhum `unwrap` sobre dados do PDF. Um link malformado é ignorado.
- URL só abre depois do aviso ou da escolha lembrada; esquemas permitidos: `http`, `https`, `mailto`.
- `Launch` nunca executa programa.
- Desempenho:
  - abertura < 300 ms e memória < 150 MB;
  - a varredura de links vai junto com a das anotações;
  - um ladrilho tem 512 px e fica no mesmo cache LRU.
- Testes: `cargo test -q -p app-state -p gera-ui`; `cargo test -q -p pdf-core -- --test-threads=4`.

## Foco da revisão

1. PDF com milhares de links ou com `/Annots` corrompido: a abertura não pode travar nem cair. O teste vai
   na Tarefa 1.
2. URL com host em maiúsculas, com porta ou com `user@host`: a lembrança tem de casar pelo host
   normalizado. O teste vai na Tarefa 3.
3. Zoom de 6400 % numa página A0: nenhuma alocação da página inteira, só ladrilhos. O teste
   (`visible_tiles` limitado à janela) vai na Tarefa 3.
4. Setas com uma anotação selecionada: continuam movendo a anotação, sem trocar de página. Fica no
   cuidado da Tarefa 4 e no E2E.
5. Tela cheia com uma aba suspensa ou fechada: ao sair, o modo e o zoom restaurados são os da aba certa.
   Fica na Tarefa 7.

---

### Tarefa 1: links no pdf-core

**Arquivos:**
- Criar: `crates/pdf-core/src/links.rs`
- Alterar: `crates/pdf-core/src/types.rs`, `crates/pdf-core/src/doc.rs` (`summary`), `crates/pdf-core/src/lib.rs`
- Testes: `crates/pdf-core/tests/links.rs`

**Interfaces:**
- Produz:
  - `pub struct LinkInfo { pub page: i32, pub rect: PdfRect, pub target: LinkTarget }`;
  - `pub enum LinkTarget { Page { page: i32, at: Option<(f32, f32)> }, Uri(String), File { path: PathBuf, page: Option<i32> }, Blocked(String) }`;
  - `Doc::links(page) -> Result<Vec<LinkInfo>>`;
  - `DocSummary.links: Vec<LinkInfo>`.

- [ ] Testes RED em `tests/links.rs`. Um helper escreve o PDF à mão (objetos com `/Annots [<< /Subtype /Link /Rect [...] /Dest [p /XYZ x y 0] >>]`, `/A << /S /URI /URI (...) >>`, `/A << /S /GoToR /F (outro.pdf) /D [0 /Fit] >>`, `/A << /S /Launch /F (calc.exe) >>`, um destino nomeado em `/Names /Dests` e um `/A << >>` sem `/S`). Casos:
  - `link_to_page_resolves_point`;
  - `named_destination_resolves`;
  - `web_link_is_uri_and_javascript_is_blocked`;
  - `remote_pdf_link_is_absolute_file`;
  - `launch_of_program_is_blocked`;
  - `link_to_missing_page_is_dropped`;
  - `malformed_link_is_ignored`;
  - `summary_carries_links`.
- [ ] Rodar `cargo test -q -p pdf-core --test links`. Esperado: FAIL (não compila).
- [ ] Implementar `links.rs`:
  - iterar `/Annots` da página pelos objetos (`/Subtype /Link`);
  - destino: `/Dest`, ou `/A` com `/S /GoTo`;
  - nome ou string resolvidos por `Document::resolve_link("#nameddest=…")` ou por `pdf_lookup_dest` via
    `resolve_link`;
  - página do destino: `lookup_page_number`;
  - ponto `/XYZ`: convertido para as coordenadas do MuPDF (y para baixo) com a CTM da página;
  - `URI` passa pela lista branca;
  - `GoToR` e `Launch`: o `/F` é uma string ou um dicionário `/F` ou `/UF`;
  - `rect`: o `/Rect` transformado pela CTM da página e normalizado.
- [ ] GREEN e commit `feat(links): leitura dos links das páginas`.

### Tarefa 2: visualização inicial, caixa de conteúdo e ladrilhos no pdf-core

**Arquivos:**
- Alterar: `doc.rs`, `types.rs`, `worker.rs`, `lib.rs`
- Testes: `crates/pdf-core/tests/links.rs` (visualização inicial) e `tests/render_tile.rs`

**Interfaces:**
- Produz:
  - `pub struct InitialView { pub panel: Option<InitialPanel>, pub full_screen: bool, pub layout: Option<InitialLayout>, pub page: Option<i32>, pub zoom: Option<InitialZoom> }`;
  - `InitialPanel { Bookmarks, Thumbnails }`;
  - `InitialLayout { Single, OneColumn, TwoColumn { cover: bool }, TwoPage { cover: bool } }`;
  - `InitialZoom { FitPage, FitWidth, Factor(f32) }`;
  - `DocSummary.initial_view`;
  - `Doc::content_box(page) -> Result<Option<PdfRect>>`;
  - `Doc::render_tile(page, scale, invert, rotation, tile: (u32, u32), size: u32) -> Result<RenderedPage>`;
  - `Request::ContentBox { page }` → `Response::ContentBox { page, rect: Option<PdfRect> }`;
  - `Request::RenderTile { page, scale, invert, rotation, tile, cancel }` → `Response::Tile { page, scale, invert, rotation, tile, img }`;
  - a constante pública `TILE: u32 = 512`.

- [ ] Testes RED:
  - `initial_view_reads_mode_layout_and_open_action`;
  - `content_box_finds_the_ink` (texto num canto);
  - `blank_page_has_no_content_box`;
  - `tile_is_the_matching_crop_of_the_full_render` (comparar pixels do ladrilho (1,0) com o recorte do render inteiro na mesma escala);
  - `tile_outside_the_page_is_empty_or_error`.
- [ ] Rodar. Esperado: FAIL.
- [ ] Implementar:
  - `render_tile`: a CTM é a mesma de `render_rotated`, com `pixmap` de `size × size`, transladado por `-tile*size`;
  - os ladrilhos da borda saem cortados no tamanho da página;
  - `content_box`: render a 0,25 sem anotações, procurando pixels < 250 em qualquer canal, e conversão
    de volta para pontos.
- [ ] GREEN e commit.

### Tarefa 3: funções puras no app-state

**Arquivos:**
- Criar: `crates/app-state/src/view_history.rs`, `crates/app-state/src/link_policy.rs`, `crates/app-state/src/tiles.rs`
- Alterar: `crates/app-state/src/zoom.rs`, `crates/app-state/src/prefs.rs`, `lib.rs`

**Interfaces:**
- Produz:
  - `ViewEntry { page: usize, y: f32, zoom: f32, fit: Fit }` e `ViewHistory { push, back, forward, can_back, can_forward }`, com teto de 100;
  - `zoom::LEVELS: [f32; 20]`, `zoom::zoom_in/zoom_out` por nível, `MIN_ZOOM = 0.0833`, `MAX_ZOOM = 64.0` e `zoom::parse_zoom(&str) -> Option<f32>`;
  - `Fit::{None, Width, Page, Height, Visible}` e `zoom::fit_height(page_h, avail_h)`;
  - `link_policy::site_key(url) -> Option<String>`; `LinkDecision { Allow, Block }`; `prefs.link_sites: BTreeMap<String, LinkDecision>` (serde default);
  - `tiles::visible_tiles(page_px: (f32, f32), visible_px: Rect, tile: f32) -> Vec<(u32, u32)>`, com teto de 64 ladrilhos.

- [ ] Testes RED:
  - `history_back_and_forward_round_trip`;
  - `push_clears_forward`;
  - `same_view_is_not_pushed_twice`;
  - `history_is_capped`;
  - `zoom_steps_follow_acrobat_levels`;
  - `parse_zoom_accepts_percent_and_comma` ("150", "150%", "66,67" → 0,6667; "abc", "0" e "99999" → None ou limitado);
  - `fit_height`;
  - `site_key_normalizes_host` (`HTTPS://Ex.com:8080/a` → `https://ex.com`; `mailto:x@y` → `mailto:`; `user@host` ignora o usuário);
  - `visible_tiles_cover_only_the_viewport_and_are_capped`.
- [ ] RED → GREEN → commit.

### Tarefa 4: navegação pelo teclado e pelo menu, histórico e defeito dos menus

**Arquivos:** `gera-ui/src/doc_tab.rs`, `shell/commands.rs`, `shell/menus.rs`, `shell/help.rs`, `app.rs`.

**Interfaces:**
- Consome: `ViewHistory`.
- Produz:
  - `DocCommand::{FirstPage, PrevPage, NextPage, LastPage, GoToPage, ViewBack, ViewForward}`;
  - `Command::Tool(&'static str)`;
  - `DocTab::jump(page, at)`, que empilha no histórico e é usado por todo salto.

- [ ] Testes RED (`commands.rs`):
  - `navigation_shortcuts_match_acrobat`: `Ctrl+Shift+N`, `Alt+←/→`, `Ctrl+PgUp/PgDn`, `Shift+Ctrl+PgUp/PgDn` e `Shift+Ctrl+↑/↓`;
  - um teste de menu, `ready_tools_are_enabled_in_menus`, sobre a função pura `menu_tools()` que lista os ids usados.
- [ ] Implementar:
  - as setas `←`/`→` em todos os modos, depois de `nudge_selected` (que consome as setas quando há seleção);
  - `Enter` e `Shift+Enter` como PgDn/PgUp;
  - o submenu Exibir → Navegação de páginas;
  - Ir para página foca o campo (`focus_page = true`);
  - marcadores, miniaturas, busca, comentários e alterações passam por `jump`;
  - `soon_tool` é removida e substituída por `item(Command::Tool(id))`;
  - a ajuda lista as teclas extras.
- [ ] GREEN, commit.

### Tarefa 5: seguir links e aviso de segurança

**Arquivos:** `gera-ui/src/doc_tab.rs` (hover e clique no `on_page`), `gera-ui/src/shell/link_warning.rs` (novo), `app.rs`.

**Interfaces:**
- Consome: `DocSummary.links`, `link_policy`, `prefs.link_sites`.
- Produz: `DocEvent::OpenLink(LinkTarget)`, tratado na casca, e `LinkWarning { url, remember }`.

- [ ] Testes RED (`link_warning.rs`, puro):
  - `remembered_block_never_opens`;
  - `remembered_allow_opens_without_asking`;
  - `unknown_site_asks`;
  - `link_at_finds_topmost_link_under_point` (função `link_at(links, page, pt)`).
- [ ] Implementar:
  - o hover usa `PointingHand` e a dica;
  - o clique só segue o link sem anotação sob o ponteiro e com a ferramenta Selecionar;
  - `Page` → `jump`; `Uri` → decisão; `File` → `open_path` e `jump`; `Blocked` → status.
- [ ] GREEN, commit.

### Tarefa 6: zoom (campo, lista, ajustes) e ladrilhos

**Arquivos:** `shell/toolbar.rs`, `view.rs`, `canvas.rs`, `doc_tab.rs`, `shell/commands.rs`.

- [ ] Testes RED:
  - `view.rs`: `fit_height_and_visible_follow_the_window`;
  - `tiles_are_requested_only_above_the_cap` (DocView com `max_side` pequeno: `wanted_tiles` não vazio; com zoom baixo, vazio);
  - `commands.rs`: `Ctrl+3`, `Ctrl+Y`.
- [ ] Implementar:
  - campo editável com `parse_zoom`, ⏷ com os níveis e os ajustes;
  - `Fit::Visible` pede `ContentBox` e aplica; o `canvas` aceita `jump_x`;
  - ladrilhos: o `DocView` ganha `tile_cache` (no mesmo LRU, com chave `TileKey`), `request_tiles` e
    `on_tile`; o canvas desenha os ladrilhos prontos sobre a base.
- [ ] GREEN, commit.

### Tarefa 7: tela cheia, modo leitura e visualização inicial

**Arquivos:** `app.rs`, `doc_tab.rs`, `shell/menus.rs`, `shell/commands.rs`.

**Interfaces:**
- Produz:
  - `Command::ReadMode`;
  - `DocTab::enter_presentation()` e `exit_presentation()`, que guardam e restauram o modo e o ajuste;
  - `DocTab::apply_initial_view(&InitialView) -> bool` (devolve se pede tela cheia).

- [ ] Testes RED:
  - `full_screen_is_ctrl_l_and_read_mode_ctrl_h`;
  - `presentation_restores_mode_and_fit` (`DocTab` sem janela, como os testes atuais da aba);
  - `initial_view_maps_layout_and_zoom` (função pura `initial_mode(&InitialView) -> (Option<ViewMode>, bool cover, Option<Fit>)`).
- [ ] Implementar:
  - clique e botão direito em tela cheia;
  - barra que some em 2 s;
  - modo leitura com a barra flutuante;
  - visualização inicial aplicada na primeira abertura.
- [ ] GREEN, commit.

### Fim da etapa

- `simplify` sobre `git diff main...HEAD -- crates scripts`.
- E2E em Xvfb (cenários da spec), `scripts/measure.sh` e revisão do Opus.
- Uma rodada de correções, merge `--no-ff`, AppImage, página e publicação da 0.13.0.
