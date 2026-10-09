# Gera PDF Editor v1.1-A — Casca do Acrobat — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Objetivo:** trocar o modelo "uma janela por documento" por uma casca no arranjo do Acrobat DC. Ela
traz barra de menus, abas (Início, Ferramentas e documentos), barra de ferramentas, trilho e
painéis laterais, quatro modos de exibição, girar a vista, tela cheia e abrir arquivos pela
interface.

**Arquitetura:** o `GeraApp` vira só a casca e guarda uma lista de `DocTab`. Cada `DocTab` tem o
seu `Worker` (thread do MuPDF) e todo o estado de um documento. A lógica pura e testável fica no
`app-state`: abas, recentes, preferências, layout por modo, transformação tela ↔ PDF com rotação e
catálogo de ferramentas. O `pdf-core` só ganha a rotação no render.

**Stack:** Rust 2021, egui/eframe 0.36 (glow), mupdf 0.8, rfd 0.17 (xdg-portal), serde +
serde_json.

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1.1a-shell-design.md`

## Global Constraints

- Tudo em pt-BR: textos da interface, comentários e mensagens de commit.
- Toda mensagem de commit termina com o trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (use um segundo `-m`).
- **Antes de rodar os testes que devem passar**, rode a skill `simplify` sobre o código criado ou
  alterado na tarefa e aplique o que ela apontar, salvo o que mudaria o comportamento (registre o
  que foi pulado). Esta é uma exigência do usuário.
- Ao fim de cada tarefa: `cargo fmt --all`, `cargo clippy --workspace --all-targets -q -- -D warnings` e `cargo test --workspace -q` limpos.
- Nada de `unwrap()` em caminho de produção que dependa de entrada do usuário ou do disco.
- Metas que não podem regredir: abertura < 300 ms, memória < 150 MB (agora com **3 abas** de 100
  páginas), binário release < 25 MB e rolagem sem travar.
- Nenhum código, binário, ícone ou texto da Adobe. Os nomes de função são descritivos e originais.
- `rfd` só com o backend xdg-portal (as features padrão da 0.17). **Não** ligar `gtk3`.
- **Desvio consciente do spec:** girar a vista usa `Ctrl+R` (horário) e `Ctrl+Shift+R`
  (anti-horário), não `Ctrl+Shift++`/`Ctrl+Shift+-`. No teclado ABNT2 o `+` já exige Shift, e
  como o egui aceita Shift a mais num atalho que não o pede, `Ctrl++` (zoom) e `Ctrl+Shift++`
  (girar) seriam a mesma tecla.

## Review Focus

1. **Anotar com a vista girada:** um destaque arrastado a 90° tem que cair sobre o texto
   selecionado no PDF salvo. Coberto pela tarefa 3, com o teste `rotation_matches_the_renderer`,
   que confere a fórmula do `PageMap` contra o render real do MuPDF.
2. **O mesmo arquivo por caminhos diferentes** (relativo, absoluto, link simbólico) abre **uma**
   aba só. Coberto pela tarefa 6 (`canonical_unifies_relative_and_symlinked_paths`).
3. **Fechar o app com alterações numa aba inativa:** precisa perguntar, não perder. Coberto pela
   tarefa 8 (testes do `CloseFlow`).
4. **Arquivo de recentes ou de preferências corrompido, ou pasta de configuração sem permissão:**
   o app abre normalmente com valores padrão e não apaga o arquivo antigo. Coberto pela tarefa 7
   (`corrupt_or_missing_file_gives_default_and_keeps_the_file`,
   `unwritable_location_is_an_error_not_a_panic`).
5. **Renders atrasados de uma aba suspensa** não podem reencher o cache (memória com várias abas).
   Coberto pela tarefa 5 (`renders_nobody_is_waiting_for_are_dropped`).

**Limitação conhecida:** com o portal XDG ausente, o rfd 0.17 pode devolver "cancelado" em vez de
erro, e aí o app não mostra mensagem. Só a falha da thread do seletor é detectável e vira
mensagem. Verifique no roteiro manual da tarefa 8.

---

## Estrutura de arquivos

| Arquivo | Ação | Responsabilidade |
|---|---|---|
| `crates/pdf-core/src/types.rs` | modificar | tipo `Rotation` |
| `crates/pdf-core/src/doc.rs` | modificar | `render_rotated` |
| `crates/pdf-core/src/worker.rs` | modificar | `rotation` em `Request::Render`/`Response::Rendered` |
| `crates/app-state/src/transform.rs` | criar | `PageMap`: tela ↔ PDF com zoom e rotação |
| `crates/app-state/src/layout.rs` | modificar | `ViewMode`, linhas (rows), `x` por página |
| `crates/app-state/src/zoom.rs` | modificar | `Fit`, `fit_page` |
| `crates/app-state/src/nav.rs` | modificar | `paged_step` |
| `crates/app-state/src/tabs.rs` | criar | `TabList`, `canonical`, `tab_title` |
| `crates/app-state/src/store.rs` | criar | JSON na pasta de configuração, gravação segura |
| `crates/app-state/src/recents.rs` | criar | `RecentFiles`, `ago` |
| `crates/app-state/src/prefs.rs` | criar | `Prefs`, `LeftPanel` |
| `crates/app-state/src/tools_catalog.rs` | criar | catálogo de ferramentas |
| `crates/gera-ui/src/doc_tab.rs` | criar | um documento: worker, vista, busca, edição, painel |
| `crates/gera-ui/src/app.rs` | reescrever | casca: abas, diálogos, fechamento, comandos |
| `crates/gera-ui/src/open_dialog.rs` | criar | seletor de arquivos em outra thread |
| `crates/gera-ui/src/shell/mod.rs` | criar | módulos da casca |
| `crates/gera-ui/src/shell/tabs.rs` | criar | barra de abas |
| `crates/gera-ui/src/shell/close.rs` | criar | `CloseFlow` e o diálogo "Salvar alterações?" |
| `crates/gera-ui/src/shell/commands.rs` | criar | `Command`, tabela de atalhos |
| `crates/gera-ui/src/shell/menus.rs` | criar | barra de menus |
| `crates/gera-ui/src/shell/toolbar.rs` | criar | barra de ferramentas e linha Comentar |
| `crates/gera-ui/src/shell/help.rs` | criar | janelas Atalhos e Sobre |
| `crates/gera-ui/src/shell/home.rs` | criar | aba Início |
| `crates/gera-ui/src/shell/tools_hub.rs` | criar | aba Ferramentas e lista do painel direito |
| `crates/gera-ui/src/shell/panels.rs` | criar | trilho esquerdo |
| `crates/gera-ui/src/view.rs`, `canvas.rs`, `edit.rs`, `sidebar.rs`, `bench.rs`, `main.rs` | modificar | modos, rotação, `PageMap`, várias abas |
| `scripts/measure.sh` | modificar | memória com 3 abas |

---

### Tarefa 1: Extrair `DocTab` do `GeraApp` (refatoração sem mudar comportamento)

**Arquivos:**
- Criar: `crates/gera-ui/src/doc_tab.rs`
- Modificar: `crates/gera-ui/src/app.rs` (quase inteiro), `crates/gera-ui/src/main.rs:1-7`

**Interfaces:**
- Produz:
  - `pub struct DocTab`, com os campos `pub(crate) worker: Worker`, `view: Option<DocView>`,
    `status: String`, `last: CanvasFrame`, `jump_to: Option<f32>`, `search: SearchState`,
    `edit: EditState` e `saved_annots: Vec<AnnotInfo>`.
  - `DocTab::new(ctx: &egui::Context) -> Self`
  - `DocTab::open(&mut self, path: PathBuf)`
  - `DocTab::is_dirty(&self) -> bool`
  - `DocTab::poll(&mut self, ctx: &egui::Context, bench: Option<&mut Bench>)`
  - `DocTab::handle_input(&mut self, ctx: &egui::Context)`
  - `DocTab::show(&mut self, ui: &mut egui::Ui)`

Nesta tarefa nenhum comportamento muda. Ela só move código, para que as tarefas seguintes
consigam ter várias abas.

- [ ] **Passo 1: Criar `doc_tab.rs` com a struct e os métodos movidos**

Crie `crates/gera-ui/src/doc_tab.rs` com este cabeçalho, struct e construtor:

```rust
use crate::bench::Bench;
use crate::canvas::{self, CanvasFrame};
use crate::edit::{screen_rect, EditState};
use crate::search::SearchState;
use crate::sidebar::{self, SidebarAction, SidebarTab};
use crate::view::DocView;
use app_state::nav::{nav_target, NavKey};
use app_state::tools::Tool;
use app_state::zoom;
use pdf_core::{AnnotInfo, DocSummary, OutlineItem, PdfError, Request, Response, Worker};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

/// Texturas novas por quadro (cada uma custa conversão e envio à GPU na thread da UI).
const MAX_RENDERS_PER_FRAME: usize = 2;

/// Um documento aberto: worker próprio, vista, busca, edição e painel lateral.
pub struct DocTab {
    pub(crate) worker: Worker,
    pub(crate) view: Option<DocView>,
    pub(crate) status: String,
    /// Último quadro do canvas (rolagem e página atual).
    pub(crate) last: CanvasFrame,
    pub(crate) jump_to: Option<f32>,
    pub(crate) search: SearchState,
    pub(crate) edit: EditState,
    /// Anotações já gravadas no PDF aberto.
    pub(crate) saved_annots: Vec<AnnotInfo>,
    /// O motor de PDF (thread do worker) parou; já avisado.
    engine_stopped: bool,
    tab: SidebarTab,
    outline: Vec<OutlineItem>,
    /// Caminho que está sendo aberto (para pedir a senha de um PDF protegido).
    opening: Option<PathBuf>,
    /// Pedido de senha: (caminho, senha digitada).
    password_prompt: Option<(PathBuf, String)>,
}

impl DocTab {
    pub fn new(ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        Self {
            worker: Worker::spawn(move || ctx.request_repaint()),
            view: None,
            status: "Arraste um PDF para cá ou execute: gera-pdf-editor arquivo.pdf".into(),
            last: CanvasFrame::default(),
            jump_to: None,
            search: SearchState::default(),
            edit: EditState::default(),
            saved_annots: Vec::new(),
            engine_stopped: false,
            tab: SidebarTab::default(),
            outline: Vec::new(),
            opening: None,
            password_prompt: None,
        }
    }

    /// Há anotações ainda não gravadas no PDF?
    pub fn is_dirty(&self) -> bool {
        self.edit.pending.is_dirty()
    }
}
```

Mova do `app.rs` atual para um segundo `impl DocTab`, **sem mudar o corpo**, os métodos `open`,
`open_with`, `on_opened`, `poll`, `toolbar`, `sidebar`, `password_window` e `canvas`, e a
função livre `paint_search_hits`. Ajustes obrigatórios:

- `open` passa a ser `pub fn open`, e `poll` passa a ser
  `pub fn poll(&mut self, ctx: &egui::Context, mut bench: Option<&mut Bench>)`. Dentro do `poll`,
  troque `if let Some(bench) = self.bench.as_mut() {` por
  `if let Some(bench) = bench.as_deref_mut() {`.
- Acrescente o método que recebe a parte de teclado e zoom do antigo `logic` (linhas 404–447 do
  `app.rs` original, do comentário `// Ctrl+Z/Ctrl+Shift+Z` até o `if let (true, Some(view)) = (save, …)`),
  com o mesmo corpo:

```rust
    /// Teclado e zoom do documento: navegação, Ctrl+roda, desfazer/refazer e salvar.
    pub fn handle_input(&mut self, ctx: &egui::Context) {
        // (corpo movido sem mudança do antigo `GeraApp::logic`, a partir de `let typing = …`)
    }
```

- Acrescente `show`, com o corpo do antigo `ui` **menos** a chamada a `confirm_close_window`:

```rust
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(view) = self.view.as_mut() {
            view.begin_frame();
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            let unsaved = self.edit.pending.items().len();
            let text = match unsaved {
                0 => self.status.clone(),
                n => format!("{} — {n} alterações não salvas", self.status),
            };
            ui.label(text);
        });
        if self.view.is_some() {
            self.sidebar(ui);
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.edit.note_window(ui);
        self.password_window(ui);
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
        }
    }
```

- [ ] **Passo 2: Reescrever `app.rs` como casca de um único `DocTab`**

Substitua todo o `crates/gera-ui/src/app.rs` por:

```rust
use crate::bench::Bench;
use crate::doc_tab::DocTab;
use std::path::PathBuf;

pub struct GeraApp {
    doc: DocTab,
    /// Janela "descartar alterações?" aberta (o usuário tentou fechar com anotações pendentes).
    confirm_close: bool,
    /// Modo de medição (`--bench-open` / `--bench-scroll`).
    bench: Option<Bench>,
}

impl GeraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        bench: Option<Bench>,
    ) -> Self {
        let mut doc = DocTab::new(&cc.egui_ctx);
        if let Some(path) = path {
            doc.open(path);
        }
        Self {
            doc,
            confirm_close: false,
            bench,
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.iter().map(|f| f.path().to_path_buf()) {
            if self.doc.view.is_none() {
                self.doc.open(file);
            } else if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(file).spawn(); // uma janela por documento
            }
        }
    }

    /// Confirmação de saída com anotações não salvas.
    fn confirm_close_window(&mut self, ui: &mut egui::Ui) {
        if !self.confirm_close {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Window::new("Alterações não salvas")
            .collapsible(false)
            .show(ui, |ui| {
                ui.label(format!(
                    "Há {} anotação(ões) que ainda não foram salvas no PDF.",
                    self.doc.edit.pending.items().len()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Sair sem salvar").clicked() {
                        self.doc.edit.reset(); // sem pendentes, o fechamento não pede confirmação de novo
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Voltar").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.doc.poll(ctx, self.bench.as_mut());
        self.handle_drops(ctx);
        if let (Some(bench), Some(view)) = (self.bench.as_mut(), self.doc.view.as_mut()) {
            if let Some(y) = bench.scroll_step(ctx, view) {
                self.doc.jump_to = Some(y);
            }
        }
        // Fechar com anotações pendentes pede confirmação, em vez de perdê-las em silêncio.
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        self.doc.handle_input(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.doc.show(ui);
        self.confirm_close_window(ui);
    }
}
```

Em `main.rs`, acrescente `mod doc_tab;` à lista de módulos (em ordem alfabética, depois de
`mod canvas;`).

- [ ] **Passo 3: Rodar a skill `simplify` sobre `doc_tab.rs` e `app.rs`**

Aplique o que ela apontar, salvo mudanças de comportamento.

- [ ] **Passo 4: Compilar, testar e validar rodando o app**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: sem avisos, todos os testes passam (a contagem é igual à de antes da tarefa).

Run: `cargo run --release -q -p gera-ui -- <cópia de um PDF>`
Esperado: abre, rola, busca (Ctrl+F), destaca, desfaz (Ctrl+Z), salva (Ctrl+S) e pede confirmação
ao fechar com anotação pendente. Tudo como antes.

- [ ] **Passo 5: Commit**

```bash
git add crates/gera-ui/src
git commit -m "refactor(gera-ui): extrai DocTab do GeraApp (base para abas)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 2: `pdf-core` — rotação da vista no render

**Arquivos:**
- Modificar: `crates/pdf-core/src/types.rs` (fim do arquivo), `crates/pdf-core/src/doc.rs:2-4,79-82`, `crates/pdf-core/src/worker.rs:25-83,139-160`
- Modificar: `crates/gera-ui/src/view.rs:168-173` e `crates/gera-ui/src/doc_tab.rs` (o `match` de `Response::Rendered`)
- Testes: `crates/pdf-core/tests/render.rs`, `crates/pdf-core/tests/worker.rs`

**Interfaces:**
- Produz:
  - `pub enum Rotation { R0, R90, R180, R270 }` (`Default` = `R0`, com `Copy`, `Eq` e `Hash`),
    com os métodos `degrees(self) -> u16`, `cw(self) -> Self`, `ccw(self) -> Self` e
    `is_sideways(self) -> bool`.
  - `PdfDoc::render_rotated(&self, page: i32, scale: f32, invert: bool, rotation: Rotation) -> Result<RenderedPage>`
  - `Request::Render { page, scale, invert, rotation, cancel }`
  - `Response::Rendered { page, scale, invert, rotation, image }`
- Convenção: a rotação é **horária**. Um ponto `(x, y)` da página de largura `w` e altura `h`
  (pontos, sem a rotação da vista) vai, antes do zoom, para:
  - `R0`: `(x, y)`
  - `R90`: `(h − y, x)`
  - `R180`: `(w − x, h − y)`
  - `R270`: `(y, w − x)`

  A tarefa 3 usa exatamente essas fórmulas.

- [ ] **Passo 1: Escrever os testes que falham**

Acrescente a `crates/pdf-core/tests/render.rs` (o `use` vai junto aos outros, no topo):

```rust
use pdf_core::{RenderedPage, Rotation};

/// Pixel mais escuro da imagem (soma dos 3 canais).
fn darkest(img: &RenderedPage) -> (usize, usize) {
    let w = img.width as usize;
    let i = img
        .rgb
        .chunks(3)
        .enumerate()
        .min_by_key(|(_, p)| p.iter().map(|&b| b as u32).sum::<u32>())
        .map(|(i, _)| i)
        .unwrap();
    (i % w, i / w)
}

/// Há tinta escura em volta de (x, y)? (vizinhança 3x3, por causa da suavização das bordas)
fn dark_near(img: &RenderedPage, x: usize, y: usize) -> bool {
    let (w, h) = (img.width as usize, img.height as usize);
    (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| {
        (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| {
            let i = (yy * w + xx) * 3;
            img.rgb[i..i + 3].iter().map(|&b| b as u32).sum::<u32>() < 300
        })
    })
}

#[test]
fn view_rotation_swaps_dimensions_and_moves_content_clockwise() {
    let (_d, doc) = doc(0);
    let r0 = doc.render_rotated(0, 0.5, false, Rotation::R0).unwrap();
    assert_eq!(r0, doc.render(0, 0.5, false).unwrap(), "R0 é o render de sempre");
    let r90 = doc.render_rotated(0, 0.5, false, Rotation::R90).unwrap();
    let r180 = doc.render_rotated(0, 0.5, false, Rotation::R180).unwrap();
    let r270 = doc.render_rotated(0, 0.5, false, Rotation::R270).unwrap();
    assert_eq!((r0.width, r0.height), (306, 396));
    assert_eq!((r90.width, r90.height), (396, 306));
    assert_eq!((r180.width, r180.height), (306, 396));
    assert_eq!((r270.width, r270.height), (396, 306));
    let (x, y) = darkest(&r0);
    let (w, h) = (306usize, 396usize);
    assert!(dark_near(&r90, h - 1 - y, x), "90° horário: (x, y) -> (h-y, x)");
    assert!(dark_near(&r180, w - 1 - x, h - 1 - y), "180°: (x, y) -> (w-x, h-y)");
    assert!(dark_near(&r270, y, w - 1 - x), "270°: (x, y) -> (y, w-x)");
}

#[test]
fn rotation_steps_and_degrees() {
    assert_eq!(Rotation::default(), Rotation::R0);
    assert_eq!(Rotation::R0.cw(), Rotation::R90);
    assert_eq!(Rotation::R270.cw(), Rotation::R0);
    assert_eq!(Rotation::R0.ccw(), Rotation::R270);
    assert_eq!(Rotation::R180.degrees(), 180);
    assert!(Rotation::R90.is_sideways() && Rotation::R270.is_sideways());
    assert!(!Rotation::R0.is_sideways() && !Rotation::R180.is_sideways());
}
```

Em `crates/pdf-core/tests/worker.rs`, acrescente `rotation: Rotation::R0,` a **cada**
`Request::Render { … }` (hoje nas linhas 34, 40 e 137) e `Rotation` ao `use pdf_core::{…}` do
arquivo.

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test render -q 2>&1 | tail -5`
Esperado: erro de compilação "unresolved import `pdf_core::Rotation`".

- [ ] **Passo 3: Implementar**

No fim de `crates/pdf-core/src/types.rs`:

```rust
/// Rotação da vista, no sentido horário. Só muda a tela, nunca o arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn degrees(self) -> u16 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    /// Um quarto de volta no sentido horário.
    pub fn cw(self) -> Self {
        match self {
            Rotation::R0 => Rotation::R90,
            Rotation::R90 => Rotation::R180,
            Rotation::R180 => Rotation::R270,
            Rotation::R270 => Rotation::R0,
        }
    }

    /// Um quarto de volta no sentido anti-horário.
    pub fn ccw(self) -> Self {
        self.cw().cw().cw()
    }

    /// A 90° e 270° largura e altura trocam de lugar.
    pub fn is_sideways(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}
```

Em `crates/pdf-core/src/doc.rs`, acrescente `Rotation` ao `use crate::types::{…}` e troque o
início de `render` por:

```rust
    pub fn render(&self, page: i32, scale: f32, invert: bool) -> Result<RenderedPage> {
        self.render_rotated(page, scale, invert, Rotation::R0)
    }

    /// Como `render`, com a vista girada (horário). O MuPDF posiciona a imagem pela caixa da
    /// página já transformada, então a origem continua no canto superior esquerdo.
    pub fn render_rotated(
        &self,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
    ) -> Result<RenderedPage> {
        let page = self.load(page)?;
        let size = size_of(&page)?;
        if !scale.is_finite() || scale <= 0.0 || scale > max_render_scale(&size) {
            return Err(PdfError::InvalidScale(scale));
        }
        let mut ctm = Matrix::new_rotate(f32::from(rotation.degrees()));
        ctm.scale(scale, scale); // escala uniforme: comuta com a rotação
```

O resto do corpo continua igual, a partir de `let pix = page.to_pixmap(&ctm, …)`. O limite de
escala não muda porque largura × altura e o maior lado não dependem da rotação.

Em `crates/pdf-core/src/worker.rs`:
- Acrescente `Rotation` ao `use crate::types::{…}`.
- Acrescente o campo `rotation: Rotation,` a `Request::Render` (depois de `invert`) e a
  `Response::Rendered` (depois de `invert`).
- Em `State::handle`, no braço `Request::Render`, desestruture `rotation`, chame
  `d.render_rotated(page, scale, invert, rotation)` e envie `rotation` no `Response::Rendered`.

No `gera-ui`, para compilar sem mudar comportamento ainda:
- `view.rs`, `request_at`: acrescente `rotation: pdf_core::Rotation::R0,` ao `Request::Render`.
- `doc_tab.rs`, `poll`: no braço `Response::Rendered { page, scale, invert, image }`, acrescente
  `..` ao padrão. A rotação passa a ser usada na tarefa 5.

- [ ] **Passo 4: Rodar a skill `simplify` sobre os arquivos alterados**

- [ ] **Passo 5: Rodar os testes**

Run: `cargo test -p pdf-core -q 2>&1 | grep -E "test result|FAILED"`
Esperado: todos passam, inclusive `view_rotation_swaps_dimensions_and_moves_content_clockwise`.
Se as asserções de direção falharem, **não** troque as fórmulas do teste. Corrija a montagem da
matriz (por exemplo, `Matrix::new_scale` seguido de `.rotate(...)`), porque a tarefa 3 depende
dessa convenção.

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep FAILED`
Esperado: sem saída.

- [ ] **Passo 6: Commit**

```bash
git add crates/pdf-core crates/gera-ui/src/view.rs crates/gera-ui/src/doc_tab.rs
git commit -m "feat(pdf-core): rotação da vista no render (horária, 0/90/180/270)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 3: `app-state` — `PageMap` (tela ↔ PDF com zoom e rotação)

**Arquivos:**
- Criar: `crates/app-state/src/transform.rs`
- Modificar: `crates/app-state/src/lib.rs` (acrescentar `pub mod transform;`), `crates/app-state/Cargo.toml` (`[dev-dependencies] tempfile = "3"`)

**Interfaces:**
- Consome: `pdf_core::{PdfRect, Rotation}` e a convenção de rotação da tarefa 2.
- Produz:
  - `pub fn rotated(size: (f32, f32), rotation: Rotation) -> (f32, f32)`
  - `pub struct PageMap { pub origin: (f32, f32), pub zoom: f32, pub size: (f32, f32), pub rotation: Rotation }`
  - `PageMap::to_screen(&self, p: (f32, f32)) -> (f32, f32)`
  - `PageMap::to_page(&self, s: (f32, f32)) -> (f32, f32)`
  - `PageMap::rect_to_screen(&self, r: &PdfRect) -> ((f32, f32), (f32, f32))`, que devolve
    `(min, max)` normalizados.

- [ ] **Passo 1: Escrever os testes que falham**

Crie `crates/app-state/src/transform.rs` só com os testes:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::{sample::sample_pdf, PdfDoc, RenderedPage};

    const ALL: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

    fn map(rotation: Rotation) -> PageMap {
        PageMap {
            origin: (100.0, 50.0),
            zoom: 2.0,
            size: (612.0, 792.0),
            rotation,
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn page_to_screen_and_back_is_identity_at_every_angle() {
        for r in ALL {
            let m = map(r);
            for p in [(0.0, 0.0), (30.0, 40.0), (612.0, 792.0), (500.5, 12.25)] {
                assert!(close(m.to_page(m.to_screen(p)), p), "{r:?} {p:?}");
            }
        }
    }

    #[test]
    fn page_corners_land_where_a_clockwise_turn_puts_them() {
        // canto superior esquerdo da página
        assert!(close(map(Rotation::R0).to_screen((0.0, 0.0)), (100.0, 50.0)));
        assert!(close(map(Rotation::R90).to_screen((0.0, 0.0)), (100.0 + 792.0 * 2.0, 50.0)));
        assert!(close(
            map(Rotation::R180).to_screen((0.0, 0.0)),
            (100.0 + 612.0 * 2.0, 50.0 + 792.0 * 2.0)
        ));
        assert!(close(map(Rotation::R270).to_screen((0.0, 0.0)), (100.0, 50.0 + 612.0 * 2.0)));
    }

    #[test]
    fn rects_come_out_normalized() {
        let r = PdfRect::new(10.0, 20.0, 30.0, 40.0);
        for rot in ALL {
            let (min, max) = map(rot).rect_to_screen(&r);
            assert!(min.0 < max.0 && min.1 < max.1, "{rot:?}");
            let area = (max.0 - min.0) * (max.1 - min.1);
            assert!((area - 20.0 * 20.0 * 4.0).abs() < 1e-2, "{rot:?}: {area}");
        }
    }

    #[test]
    fn rotated_size_swaps_sideways() {
        assert_eq!(rotated((612.0, 792.0), Rotation::R90), (792.0, 612.0));
        assert_eq!(rotated((612.0, 792.0), Rotation::R180), (612.0, 792.0));
    }

    #[test]
    fn bad_zoom_does_not_produce_non_finite_points() {
        let mut m = map(Rotation::R90);
        m.zoom = 0.0;
        let p = m.to_page((10.0, 10.0));
        assert!(p.0.is_finite() && p.1.is_finite());
    }

    fn darkest(img: &RenderedPage) -> (usize, usize) {
        let w = img.width as usize;
        let i = img
            .rgb
            .chunks(3)
            .enumerate()
            .min_by_key(|(_, p)| p.iter().map(|&b| b as u32).sum::<u32>())
            .map(|(i, _)| i)
            .unwrap();
        (i % w, i / w)
    }

    fn dark_near(img: &RenderedPage, x: usize, y: usize) -> bool {
        let (w, h) = (img.width as usize, img.height as usize);
        (x.saturating_sub(1)..(x + 2).min(w)).any(|xx| {
            (y.saturating_sub(1)..(y + 2).min(h)).any(|yy| {
                let i = (yy * w + xx) * 3;
                img.rgb[i..i + 3].iter().map(|&b| b as u32).sum::<u32>() < 300
            })
        })
    }

    /// O `PageMap` tem que concordar com o render girado do MuPDF; senão as anotações feitas com a
    /// vista girada cairiam fora do lugar.
    #[test]
    fn rotation_matches_the_renderer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.pdf");
        std::fs::write(&path, sample_pdf(1, 0)).unwrap();
        let doc = PdfDoc::open(&path, None).unwrap();
        let zoom = 0.5;
        let flat = doc.render_rotated(0, zoom, false, Rotation::R0).unwrap();
        let (x, y) = darkest(&flat);
        let point = ((x as f32 + 0.5) / zoom, (y as f32 + 0.5) / zoom);
        for r in [Rotation::R90, Rotation::R180, Rotation::R270] {
            let img = doc.render_rotated(0, zoom, false, r).unwrap();
            let m = PageMap { origin: (0.0, 0.0), zoom, size: (612.0, 792.0), rotation: r };
            let (sx, sy) = m.to_screen(point);
            assert!(dark_near(&img, sx as usize, sy as usize), "{r:?}: ({sx}, {sy})");
        }
    }
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state transform -q 2>&1 | tail -5`
Esperado: erro de compilação (`PageMap` não existe).

- [ ] **Passo 3: Implementar (acima do módulo de testes)**

```rust
//! Conversão entre coordenadas da página (pontos do PDF) e da tela, com zoom e rotação da vista.
use crate::zoom::finite_positive;
use pdf_core::{PdfRect, Rotation};

/// Tamanho que a página ocupa depois de girada.
pub fn rotated(size: (f32, f32), rotation: Rotation) -> (f32, f32) {
    if rotation.is_sideways() {
        (size.1, size.0)
    } else {
        size
    }
}

/// Onde uma página está na tela e como ela foi girada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMap {
    /// Canto superior esquerdo da página na tela.
    pub origin: (f32, f32),
    pub zoom: f32,
    /// Tamanho da página em pontos, sem a rotação da vista.
    pub size: (f32, f32),
    pub rotation: Rotation,
}

impl PageMap {
    fn zoom(&self) -> f32 {
        finite_positive(self.zoom).unwrap_or(1.0)
    }

    /// Ponto da página (pontos) -> ponto na tela. Mesma convenção do render girado do `pdf-core`.
    pub fn to_screen(&self, p: (f32, f32)) -> (f32, f32) {
        let ((w, h), (x, y)) = (self.size, p);
        let (dx, dy) = match self.rotation {
            Rotation::R0 => (x, y),
            Rotation::R90 => (h - y, x),
            Rotation::R180 => (w - x, h - y),
            Rotation::R270 => (y, w - x),
        };
        let z = self.zoom();
        (self.origin.0 + dx * z, self.origin.1 + dy * z)
    }

    /// Ponto na tela -> ponto da página (pontos). Inverso de `to_screen`.
    pub fn to_page(&self, s: (f32, f32)) -> (f32, f32) {
        let (w, h) = self.size;
        let z = self.zoom();
        let (dx, dy) = ((s.0 - self.origin.0) / z, (s.1 - self.origin.1) / z);
        match self.rotation {
            Rotation::R0 => (dx, dy),
            Rotation::R90 => (dy, h - dx),
            Rotation::R180 => (w - dx, h - dy),
            Rotation::R270 => (w - dy, dx),
        }
    }

    /// Retângulo da página -> retângulo na tela, como `(mínimo, máximo)`.
    pub fn rect_to_screen(&self, r: &PdfRect) -> ((f32, f32), (f32, f32)) {
        let a = self.to_screen((r.x0, r.y0));
        let b = self.to_screen((r.x1, r.y1));
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }
}
```

Acrescente `pub mod transform;` a `crates/app-state/src/lib.rs` e, em
`crates/app-state/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Passo 4: Rodar a skill `simplify` sobre `transform.rs`**

- [ ] **Passo 5: Rodar os testes**

Run: `cargo test -p app-state transform -q 2>&1 | grep -E "test result|FAILED"`
Esperado: 6 testes passam.

- [ ] **Passo 6: Commit**

```bash
git add crates/app-state
git commit -m "feat(app-state): PageMap converte tela e PDF com zoom e rotação" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 4: `app-state` — modos de exibição, ajuste à página e virada de página

**Arquivos:**
- Modificar: `crates/app-state/src/layout.rs` (reescrever o corpo e manter os testes antigos), `crates/app-state/src/zoom.rs`, `crates/app-state/src/nav.rs`

**Interfaces:**
- Produz:
  - `pub enum ViewMode { Single, Continuous, TwoUp, TwoUpContinuous }`, com `Default` =
    `Continuous`, `ALL: [ViewMode; 4]`, `label(self) -> &'static str`,
    `columns(self) -> usize` e `is_paged(self) -> bool`.
  - `Layout::new(sizes, zoom)`, igual a antes (rolagem contínua).
  - `Layout::with_mode(sizes: &[(f32, f32)], zoom: f32, mode: ViewMode, cover: bool) -> Layout`
  - Novos métodos de `Layout`: `page_x(i)`, `row_count()`, `row_of(page) -> usize` (limitado à
    última linha), `row_pages(row) -> Range<usize>`, `row_top(row)`, `row_height(row)` e
    `max_row_height()`. Os métodos atuais continuam iguais.
  - `pub enum Fit { None, Width, Page }`, com `Default` = `Width`.
  - `pub fn fit_page(page: (f32, f32), avail: (f32, f32)) -> f32`
  - `pub fn paged_step(offset: f32, max_offset: f32, wheel_y: f32) -> i32`, em `nav.rs`.

**Regras do layout:** com uma coluna, cada linha tem uma página. Com duas, as linhas têm pares
(1–2, 3–4…). Com `cover`, a página 1 fica sozinha e os pares começam em 2–3. Cada linha mede
a soma das larguras mais `PAGE_GAP` entre as páginas, e a altura é a da maior página dela. A
linha é centralizada na largura máxima, e as páginas ficam alinhadas pelo topo. As linhas são
empilhadas com `PAGE_GAP` entre elas, também nos modos página a página: o canvas mostra só uma
linha por vez.

- [ ] **Passo 1: Escrever os testes que falham**

Acrescente ao `mod tests` de `layout.rs`:

```rust
    #[test]
    fn two_up_puts_pairs_side_by_side_and_centers_rows() {
        let sizes = [(100.0, 200.0), (100.0, 300.0), (100.0, 200.0)];
        let l = Layout::with_mode(&sizes, 1.0, ViewMode::TwoUpContinuous, false);
        assert_eq!(l.row_count(), 2);
        assert_eq!(l.row_pages(0), 0..2);
        assert_eq!(l.row_pages(1), 2..3);
        assert_eq!(l.max_width(), 200.0 + PAGE_GAP);
        assert_eq!((l.page_x(0), l.page_x(1)), (0.0, 100.0 + PAGE_GAP));
        assert_eq!(l.page_top(0), l.page_top(1), "o par começa na mesma altura");
        assert_eq!(l.row_height(0), 300.0, "a linha tem a altura da maior página");
        assert_eq!(l.page_top(2), PAGE_GAP + 300.0 + PAGE_GAP);
        assert_eq!(l.page_x(2), (200.0 + PAGE_GAP - 100.0) / 2.0, "página sozinha centralizada");
        assert_eq!(l.max_row_height(), 300.0);
    }

    #[test]
    fn cover_leaves_the_first_page_alone() {
        let l = Layout::with_mode(&[(100.0, 100.0); 4], 1.0, ViewMode::TwoUp, true);
        assert_eq!(l.row_count(), 3);
        assert_eq!(l.row_pages(0), 0..1);
        assert_eq!(l.row_pages(1), 1..3);
        assert_eq!(l.row_pages(2), 3..4);
        assert_eq!(l.row_of(2), 1);
        assert_eq!(l.row_of(99), 2, "página fora do documento vai para a última linha");
    }

    #[test]
    fn visible_range_in_two_columns_uses_whole_rows() {
        // par com páginas de alturas diferentes: a baixa termina antes, mas a linha continua visível
        let l = Layout::with_mode(&[(100.0, 300.0), (100.0, 100.0), (100.0, 100.0)], 1.0, ViewMode::TwoUpContinuous, false);
        assert_eq!(l.visible_range(200.0, 50.0), 0..2);
        assert_eq!(l.visible_range(PAGE_GAP + 300.0 + PAGE_GAP + 1.0, 10.0), 2..3);
        assert_eq!(l.page_at(PAGE_GAP + 300.0 + PAGE_GAP + 1.0), 2);
    }

    #[test]
    fn single_column_modes_keep_the_old_layout() {
        let sizes = [(100.0, 200.0); 10];
        for mode in [ViewMode::Single, ViewMode::Continuous] {
            let l = Layout::with_mode(&sizes, 1.0, mode, false);
            assert_eq!(l.row_count(), 10);
            assert_eq!(l.page_top(1), ten().page_top(1));
            assert_eq!(l.total_height(), ten().total_height());
            assert_eq!(l.page_x(0), 0.0);
        }
    }

    #[test]
    fn empty_layout_row_queries_do_not_panic() {
        let l = Layout::with_mode(&[], 1.0, ViewMode::TwoUp, true);
        assert_eq!(l.row_count(), 0);
        assert_eq!(l.row_of(0), 0);
        assert_eq!(l.row_pages(5), 0..0);
        assert_eq!(l.row_top(5), PAGE_GAP);
        assert_eq!(l.row_height(5), 0.0);
    }

    #[test]
    fn view_modes_describe_columns_and_paging() {
        assert_eq!(ViewMode::default(), ViewMode::Continuous);
        assert_eq!(ViewMode::TwoUp.columns(), 2);
        assert!(ViewMode::Single.is_paged() && ViewMode::TwoUp.is_paged());
        assert!(!ViewMode::Continuous.is_paged() && !ViewMode::TwoUpContinuous.is_paged());
        let labels: std::collections::HashSet<_> = ViewMode::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(labels.len(), 4);
    }
```

Acrescente ao `mod tests` de `zoom.rs`:

```rust
    #[test]
    fn fit_page_uses_the_tighter_side() {
        // retrato 600x800 numa área 1200x800: a altura manda (1.0)
        assert!((fit_page((600.0, 800.0), (1200.0, 800.0)) - 1.0).abs() < 1e-6);
        // paisagem 800x600 numa área 400x900: a largura manda (0.5)
        assert!((fit_page((800.0, 600.0), (400.0, 900.0)) - 0.5).abs() < 1e-6);
        assert_eq!(fit_page((0.0, 800.0), (100.0, 100.0)), 1.0);
        assert_eq!(fit_page((600.0, 800.0), (f32::NAN, 100.0)), 1.0);
        assert_eq!(Fit::default(), Fit::Width);
    }
```

Acrescente ao `mod tests` de `nav.rs`:

```rust
    #[test]
    fn paged_step_turns_only_past_the_edges() {
        // roda para baixo (delta negativo) no fim da tela: próxima
        assert_eq!(paged_step(100.0, 100.0, -1.0), 1);
        // roda para cima no topo: anterior
        assert_eq!(paged_step(0.0, 100.0, 1.0), -1);
        // no meio da tela a roda só rola
        assert_eq!(paged_step(50.0, 100.0, -1.0), 0);
        assert_eq!(paged_step(50.0, 100.0, 1.0), 0);
        // tela que cabe inteira (sem rolagem): qualquer giro vira a página
        assert_eq!(paged_step(0.0, 0.0, -3.0), 1);
        assert_eq!(paged_step(0.0, 0.0, 0.0), 0);
        assert_eq!(paged_step(f32::NAN, 100.0, -1.0), 0);
    }
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state -q 2>&1 | tail -5`
Esperado: erros de compilação (`ViewMode`, `fit_page`, `paged_step` não existem).

- [ ] **Passo 3: Implementar**

Em `zoom.rs`, depois de `fit_width`:

```rust
/// Ajuste automático do zoom à área visível (refeito quando a janela muda de tamanho).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Zoom escolhido pelo usuário.
    None,
    #[default]
    Width,
    Page,
}

/// Zoom em que `page` (largura, altura em pontos) cabe inteira em `avail`.
pub fn fit_page(page: (f32, f32), avail: (f32, f32)) -> f32 {
    match (
        finite_positive(page.0),
        finite_positive(page.1),
        finite_positive(avail.0),
        finite_positive(avail.1),
    ) {
        (Some(w), Some(h), Some(aw), Some(ah)) => clamp_zoom((aw / w).min(ah / h)),
        _ => DEFAULT_ZOOM,
    }
}
```

Em `nav.rs`, depois de `nav_target`:

```rust
/// Modos página a página: a roda passou da borda da tela atual? `+1` = próxima, `-1` = anterior,
/// `0` = só rola. `wheel_y` segue o egui: negativo é rolar para baixo.
pub fn paged_step(offset: f32, max_offset: f32, wheel_y: f32) -> i32 {
    const EDGE: f32 = 1.0;
    if !(offset.is_finite() && max_offset.is_finite() && wheel_y.is_finite()) {
        return 0;
    }
    if wheel_y < 0.0 && offset >= max_offset - EDGE {
        1
    } else if wheel_y > 0.0 && offset <= EDGE {
        -1
    } else {
        0
    }
}
```

Em `layout.rs`, troque tudo acima de `#[cfg(test)]` por:

```rust
use crate::zoom::{finite_positive, DEFAULT_ZOOM};
use std::ops::Range;

pub const PAGE_GAP: f32 = 12.0;

/// Como as páginas são dispostas na tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Uma página por vez.
    Single,
    #[default]
    Continuous,
    /// Duas páginas lado a lado, um par por vez.
    TwoUp,
    TwoUpContinuous,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [
        ViewMode::Single,
        ViewMode::Continuous,
        ViewMode::TwoUp,
        ViewMode::TwoUpContinuous,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Single => "Página única",
            ViewMode::Continuous => "Rolagem contínua",
            ViewMode::TwoUp => "Duas páginas",
            ViewMode::TwoUpContinuous => "Duas páginas contínuas",
        }
    }

    pub fn columns(self) -> usize {
        match self {
            ViewMode::TwoUp | ViewMode::TwoUpContinuous => 2,
            ViewMode::Single | ViewMode::Continuous => 1,
        }
    }

    /// Mostra uma linha (página ou par) por vez, em vez de rolar o documento inteiro.
    pub fn is_paged(self) -> bool {
        matches!(self, ViewMode::Single | ViewMode::TwoUp)
    }
}

#[derive(Debug, Clone, Copy)]
struct PageBox {
    x: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone)]
struct RowBox {
    top: f32,
    height: f32,
    pages: Range<usize>,
}

/// Posição de cada página (em pontos já multiplicados pelo zoom), agrupadas em linhas.
#[derive(Debug, Default, Clone)]
pub struct Layout {
    pages: Vec<PageBox>,
    rows: Vec<RowBox>,
    max_width: f32,
}

impl Layout {
    /// Uma coluna contínua (o layout da v1).
    pub fn new(sizes: &[(f32, f32)], zoom: f32) -> Self {
        Self::with_mode(sizes, zoom, ViewMode::Continuous, false)
    }

    /// `sizes` já vêm girados (largura e altura como aparecem na tela).
    pub fn with_mode(sizes: &[(f32, f32)], zoom: f32, mode: ViewMode, cover: bool) -> Self {
        let zoom = finite_positive(zoom).unwrap_or(DEFAULT_ZOOM);
        let dim = |v: f32| finite_positive(v).unwrap_or(0.0) * zoom;
        let dims: Vec<(f32, f32)> = sizes.iter().map(|&(w, h)| (dim(w), dim(h))).collect();
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < dims.len() {
            let alone = mode.columns() == 1 || (cover && start == 0);
            let end = if alone { start + 1 } else { (start + 2).min(dims.len()) };
            ranges.push(start..end);
            start = end;
        }
        let row_width = |r: &Range<usize>| -> f32 {
            dims[r.clone()].iter().map(|d| d.0).sum::<f32>() + PAGE_GAP * (r.len() - 1) as f32
        };
        let max_width = ranges.iter().map(row_width).fold(0.0, f32::max);
        let mut pages = Vec::with_capacity(dims.len());
        let mut rows = Vec::with_capacity(ranges.len());
        let mut y = PAGE_GAP;
        for r in ranges {
            let height = dims[r.clone()].iter().map(|d| d.1).fold(0.0, f32::max);
            let mut x = (max_width - row_width(&r)) / 2.0;
            for &(width, h) in &dims[r.clone()] {
                pages.push(PageBox { x, top: y, width, height: h });
                x += width + PAGE_GAP;
            }
            rows.push(RowBox { top: y, height, pages: r });
            y += height + PAGE_GAP;
        }
        Self { pages, rows, max_width }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    pub fn total_height(&self) -> f32 {
        self.rows.last().map_or(PAGE_GAP, |r| r.top + r.height + PAGE_GAP)
    }
    pub fn max_width(&self) -> f32 {
        self.max_width
    }
    pub fn max_row_height(&self) -> f32 {
        self.rows.iter().map(|r| r.height).fold(0.0, f32::max)
    }
    /// Distância da borda esquerda do conteúdo (largura `max_width`) até a página.
    pub fn page_x(&self, i: usize) -> f32 {
        self.pages[i].x
    }
    pub fn page_top(&self, i: usize) -> f32 {
        self.pages[i].top
    }
    pub fn page_width(&self, i: usize) -> f32 {
        self.pages[i].width
    }
    pub fn page_height(&self, i: usize) -> f32 {
        self.pages[i].height
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    /// Linha da página `page` (a última, se a página não existir).
    pub fn row_of(&self, page: usize) -> usize {
        self.rows
            .partition_point(|r| r.pages.end <= page)
            .min(self.rows.len().saturating_sub(1))
    }
    pub fn row_pages(&self, row: usize) -> Range<usize> {
        self.rows.get(row).map_or(0..0, |r| r.pages.clone())
    }
    pub fn row_top(&self, row: usize) -> f32 {
        self.rows.get(row).map_or(PAGE_GAP, |r| r.top)
    }
    pub fn row_height(&self, row: usize) -> f32 {
        self.rows.get(row).map_or(0.0, |r| r.height)
    }

    /// Páginas das linhas que cruzam a janela `[scroll_y, scroll_y + viewport_h)`.
    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> Range<usize> {
        if viewport_h.is_nan() || viewport_h <= 0.0 || !scroll_y.is_finite() {
            return 0..0;
        }
        let bottom = scroll_y + viewport_h;
        let first = self.rows.partition_point(|r| r.top + r.height <= scroll_y);
        let last = self.rows.partition_point(|r| r.top < bottom);
        if first >= last {
            return 0..0;
        }
        self.rows[first].pages.start..self.rows[last - 1].pages.end
    }

    /// Primeira página da linha que contém `y`.
    pub fn page_at(&self, y: f32) -> usize {
        let row = self.rows.partition_point(|r| r.top <= y).saturating_sub(1);
        self.rows.get(row).map_or(0, |r| r.pages.start)
    }

    pub fn expand(&self, r: Range<usize>, by: usize) -> Range<usize> {
        if self.pages.is_empty() {
            return 0..0;
        }
        r.start.saturating_sub(by)..(r.end + by).min(self.pages.len())
    }
}
```

Atenção ao teste antigo `visible_range_edges_do_not_panic`. Ele espera
`l.visible_range(1.0e9, 100.0) == 10..10`, e a nova versão devolve `0..0` para "nada visível".
Ajuste **essa** asserção para `assert!(l.visible_range(1.0e9, 100.0).is_empty());`, porque o que
importa é estar vazio. Os outros testes antigos continuam sem mudança.

- [ ] **Passo 4: Rodar a skill `simplify` sobre `layout.rs`, `zoom.rs` e `nav.rs`**

- [ ] **Passo 5: Rodar os testes**

Run: `cargo test -p app-state -q 2>&1 | grep -E "test result|FAILED"`
Esperado: todos passam.

Run: `cargo test --workspace -q 2>&1 | grep FAILED`
Esperado: sem saída (o `gera-ui` continua usando `Layout::new`).

- [ ] **Passo 6: Commit**

```bash
git add crates/app-state
git commit -m "feat(app-state): modos de exibição, ajuste à página e virada de página" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 5: `gera-ui` — modos, rotação e ajuste no documento

**Arquivos:**
- Modificar: `crates/gera-ui/src/view.rs`, `crates/gera-ui/src/canvas.rs` (reescrever), `crates/gera-ui/src/edit.rs`, `crates/gera-ui/src/doc_tab.rs`, `crates/gera-ui/src/bench.rs:88`

**Interfaces:**
- Consome: `Rotation` (tarefa 2), `PageMap`/`rotated` (tarefa 3), `ViewMode`/`Layout::with_mode`/`Fit`/`fit_page`/`paged_step` (tarefa 4).
- Produz:
  - Campos de `DocView`: `pub fit: Fit` (substitui `fit_pending`), `pub rotation: Rotation`,
    `pub mode: ViewMode`, `pub cover: bool` e `pub spread: usize`.
  - Métodos de `DocView`:
    - `set_manual_zoom(z) -> f32`
    - `apply_fit(avail_w, avail_h) -> Option<f32>`
    - `set_rotation(r)`
    - `set_mode(mode, current_page)`
    - `turn(step: i32) -> bool`
    - `turn_to(row: usize)`
    - `window() -> (f32, f32)`
    - `go_to(page: i32, at: Option<(f32, f32)>) -> Option<f32>`, que substitui `scroll_target`
    - `page_map(page: usize, origin: (f32, f32)) -> PageMap`
    - `on_rendered(ctx, page, scale, invert, rotation, img)`
  - `CanvasFrame.turn: i32`
  - O callback de `canvas::show_pages` passa a receber `&PageMap`:
    `&mut dyn FnMut(&mut egui::Ui, usize, egui::Rect, &PageMap)`.
  - `edit::screen_rect(r: &PdfRect, map: &PageMap) -> egui::Rect`
  - `EditState::on_page(ui, worker, page, rect, map: &PageMap)`
  - `pub enum DocCommand { Save, Undo, Redo, Find, ZoomIn, ZoomOut, ZoomReset, Fit(Fit), Mode(ViewMode), ToggleCover, Rotate { clockwise: bool } }`
    (com `Debug`, `Clone`, `Copy` e `PartialEq`), mais `DocTab::run(&mut self, cmd: DocCommand)` e
    o campo `pub(crate) focus_search: bool` em `DocTab`.

- [ ] **Passo 1: Escrever os testes que falham (`view.rs`)**

No `mod tests` de `view.rs`, troque os testes `scroll_target_*` por estes e acrescente os novos:

```rust
    #[test]
    fn go_to_points_slightly_above_the_position_in_the_page() {
        let mut v = view();
        v.set_zoom(2.0);
        // página 1 começa em 12 + 400 + 12 = 424; y = 50pt * 2 = 100 => 524 - 80 de folga
        assert_eq!(v.go_to(1, Some((0.0, 50.0))), Some(444.0));
        assert_eq!(v.go_to(0, None), Some(0.0), "nunca negativo");
    }

    #[test]
    fn go_to_rejects_pages_outside_the_document() {
        let mut v = view();
        assert_eq!(v.go_to(-1, None), None);
        assert_eq!(v.go_to(3, None), None);
    }

    #[test]
    fn rotation_swaps_the_layout_and_points_follow_it() {
        let mut v = view(); // 3 páginas 100x200
        v.set_rotation(Rotation::R90);
        assert_eq!((v.layout.page_width(0), v.layout.page_height(0)), (200.0, 100.0));
        // a 90°, o y do PDF vira x na tela: o ponto (30, 50) fica 30pt abaixo do topo
        let m = v.page_map(0, (0.0, 0.0));
        assert_eq!(m.to_screen((30.0, 50.0)), (150.0, 30.0));
    }

    #[test]
    fn paged_modes_turn_one_row_at_a_time() {
        let mut v = view();
        v.set_mode(ViewMode::Single, 1);
        assert_eq!(v.spread, 1, "abre na página que estava na tela");
        assert!(v.turn(1));
        assert!(!v.turn(1), "não passa da última");
        assert_eq!(v.spread, 2);
        let (top, h) = v.window();
        assert_eq!((top, h), (v.layout.row_top(2) - PAGE_GAP, 200.0 + 2.0 * PAGE_GAP));
        assert_eq!(v.go_to(0, None), Some(0.0));
        assert_eq!(v.spread, 0, "ir para uma página troca a tela");
        v.turn_to(usize::MAX);
        assert_eq!(v.spread, 2);
    }

    #[test]
    fn fit_follows_the_window_until_the_user_zooms() {
        let mut v = view(); // páginas de 100pt de largura
        assert_eq!(v.fit, Fit::Width);
        assert!(v.apply_fit(400.0, 1000.0).is_some());
        assert!((v.zoom - 4.0).abs() < 1e-3);
        assert!(v.apply_fit(400.0, 1000.0).is_none(), "mesmo tamanho: nada muda");
        v.fit = Fit::Page; // página 100x200 em 400x300: a altura manda (1.5)
        v.apply_fit(400.0, 300.0);
        assert!((v.zoom - 1.5).abs() < 1e-3);
        v.set_manual_zoom(2.0);
        assert_eq!(v.fit, Fit::None);
        assert!(v.apply_fit(800.0, 800.0).is_none());
    }

    #[test]
    fn renders_nobody_is_waiting_for_are_dropped() {
        let ctx = egui::Context::default();
        let mut v = view();
        let img = RenderedPage { width: 1, height: 1, rgb: vec![255, 255, 255] };
        // nenhum pedido feito: a resposta (atrasada, de uma aba suspensa) não entra no cache
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R0, img.clone());
        assert!(v.texture_at(0, 1.0).is_none());
        let worker = Worker::spawn(|| {});
        v.request_at(&worker, 0, 1.0);
        // resposta de outra rotação (anterior a girar a vista) também é descartada
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R90, img.clone());
        assert!(v.texture_at(0, 1.0).is_none());
        v.on_rendered(&ctx, 0, 1.0, false, Rotation::R0, img);
        assert!(v.texture_at(0, 1.0).is_some());
    }
```

Acrescente ao `use` do módulo de testes:
`use app_state::layout::{ViewMode, PAGE_GAP}; use app_state::zoom::Fit; use pdf_core::{RenderedPage, Rotation};`.

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p gera-ui view -q 2>&1 | tail -5`
Esperado: erros de compilação (`go_to`, `set_rotation` e outros não existem).

- [ ] **Passo 3: Implementar `view.rs`**

Imports novos:

```rust
use app_state::layout::{Layout, ViewMode, PAGE_GAP};
use app_state::transform::{self, PageMap};
use app_state::zoom::{self, Fit};
use pdf_core::{CancelToken, PageSize, RenderedPage, Request, Rotation, Worker};
```

`RenderKey` passa a ser `(i32, u32, bool, Rotation)`, com o comentário
`/// (página, escala quantizada, invertido, rotação)`.

Na struct, troque `pub fit_pending: bool,` por:

```rust
    pub fit: Fit,
    pub rotation: Rotation,
    pub mode: ViewMode,
    /// Modo duas páginas: a primeira fica sozinha (capa).
    pub cover: bool,
    /// Modos página a página: linha (página ou par) mostrada.
    pub spread: usize,
    /// Largura e altura da maior linha com zoom 1 (base dos ajustes).
    base: (f32, f32),
```

E no `new`: `fit: Fit::default(), rotation: Rotation::R0, mode: ViewMode::default(), cover: false, spread: 0, base: (1.0, 1.0),`.

Troque `relayout`, `fit_width` e `scroll_target` por:

```rust
    fn relayout(&mut self) {
        let dims: Vec<(f32, f32)> = self
            .sizes
            .iter()
            .map(|s| transform::rotated((s.width, s.height), self.rotation))
            .collect();
        self.layout = Layout::with_mode(&dims, self.zoom, self.mode, self.cover);
        let unit = Layout::with_mode(&dims, 1.0, self.mode, self.cover);
        self.base = (unit.max_width(), unit.max_row_height());
        self.spread = self.spread.min(self.layout.row_count().saturating_sub(1));
    }

    /// Zoom escolhido pelo usuário: desliga o ajuste automático. Devolve a razão novo/antigo.
    pub fn set_manual_zoom(&mut self, z: f32) -> f32 {
        self.fit = Fit::None;
        self.set_zoom(z)
    }

    /// Aplica o ajuste ativo à área visível; devolve a razão do zoom se ele mudou.
    pub fn apply_fit(&mut self, avail_w: f32, avail_h: f32) -> Option<f32> {
        let z = match self.fit {
            Fit::None => return None,
            Fit::Width => zoom::fit_width(self.base.0, avail_w),
            Fit::Page => zoom::fit_page(self.base, (avail_w, avail_h)),
        };
        (zoom::quantize(z) != zoom::quantize(self.zoom)).then(|| self.set_zoom(z))
    }

    pub fn set_rotation(&mut self, rotation: Rotation) {
        if rotation != self.rotation {
            self.rotation = rotation;
            self.clear_cache(); // as texturas da rotação antiga não servem mais
            self.relayout();
        }
    }

    /// Troca o modo de exibição mantendo `current_page` na tela.
    pub fn set_mode(&mut self, mode: ViewMode, current_page: usize) {
        self.mode = mode;
        self.relayout();
        self.spread = self.layout.row_of(current_page);
    }

    /// Modos página a página: avança (`+1`) ou volta (`-1`) uma tela; devolve se mudou.
    pub fn turn(&mut self, step: i32) -> bool {
        let last = self.layout.row_count().saturating_sub(1) as i64;
        let new = (self.spread as i64 + i64::from(step)).clamp(0, last) as usize;
        std::mem::replace(&mut self.spread, new) != new
    }

    /// Modos página a página: vai para a linha `row` (limitada à última).
    pub fn turn_to(&mut self, row: usize) {
        self.spread = row.min(self.layout.row_count().saturating_sub(1));
    }

    /// Faixa vertical do layout mostrada na área de rolagem: `(topo, altura)`. Tudo, ou só a
    /// linha atual nos modos página a página.
    pub fn window(&self) -> (f32, f32) {
        if self.mode.is_paged() && self.layout.row_count() > 0 {
            (
                self.layout.row_top(self.spread) - PAGE_GAP,
                self.layout.row_height(self.spread) + 2.0 * PAGE_GAP,
            )
        } else {
            (0.0, self.layout.total_height())
        }
    }

    /// Posição de rolagem que mostra a página `page` (no ponto `at` dela, ou no topo), com folga
    /// acima. Nos modos página a página também troca a tela.
    pub fn go_to(&mut self, page: i32, at: Option<(f32, f32)>) -> Option<f32> {
        let i = usize::try_from(page)
            .ok()
            .filter(|&i| i < self.layout.page_count())?;
        if self.mode.is_paged() {
            self.spread = self.layout.row_of(i);
        }
        let inside = at.map_or(0.0, |p| self.page_map(i, (0.0, 0.0)).to_screen(p).1);
        Some((self.layout.page_top(i) - self.window().0 + inside - 80.0).max(0.0))
    }

    /// Mapa tela ↔ PDF da página `page` desenhada a partir de `origin`.
    pub fn page_map(&self, page: usize, origin: (f32, f32)) -> PageMap {
        let size = self.sizes.get(page).map_or((1.0, 1.0), |s| (s.width, s.height));
        PageMap {
            origin,
            zoom: self.zoom,
            size,
            rotation: self.rotation,
        }
    }
```

Em `key`: `(page, zoom::quantize(scale), self.dark, self.rotation)`. Em `any_texture`, troque o
filtro por `.find(|k| k.0 == page && k.2 == dark && k.3 == self.rotation)`, guardando
`let rotation = self.rotation;` como já se faz com `dark`. Em `request_at`, envie
`rotation: self.rotation` no lugar do `Rotation::R0` da tarefa 2.

Troque `on_rendered` por:

```rust
    pub fn on_rendered(
        &mut self,
        ctx: &egui::Context,
        page: i32,
        scale: f32,
        invert: bool,
        rotation: Rotation,
        img: RenderedPage,
    ) {
        let key = (page, zoom::quantize(scale), invert, rotation);
        // Só entra no cache o que ainda está sendo esperado: respostas de pedidos cancelados
        // (outra rotação, outro tema, aba suspensa) seriam memória desperdiçada.
        if self.requested.remove(&key).is_none() || invert != self.dark || rotation != self.rotation {
            return;
        }
        let image = egui::ColorImage::from_rgb([img.width as usize, img.height as usize], &img.rgb);
        let handle = ctx.load_texture(
            format!("p{}-{}", key.0, key.1),
            image,
            egui::TextureOptions::LINEAR,
        );
        let bytes = img.width as usize * img.height as usize * 4;
        self.cache.insert(key, CachedTexture { handle, bytes });
    }
```

- [ ] **Passo 4: Reescrever `canvas.rs`**

```rust
use crate::view::DocView;
use app_state::nav::paged_step;
use app_state::transform::PageMap;
use pdf_core::Worker;

#[derive(Default)]
pub struct CanvasFrame {
    pub scroll_y: f32,
    pub current_page: usize,
    /// Altura visível da área de rolagem (para PageUp/PageDown).
    pub viewport_h: f32,
    /// Modos página a página: a roda pediu a tela seguinte (`+1`) ou a anterior (`-1`).
    pub turn: i32,
}

/// Rola e desenha só as páginas visíveis (mais as vizinhas, pré-carregadas). Nos modos página a
/// página mostra só a linha atual. `on_page` roda depois de pintar cada página visível, para
/// sobrepor anotações e tratar o mouse.
pub fn show_pages(
    ui: &mut egui::Ui,
    view: &mut DocView,
    worker: &Worker,
    jump_to: Option<f32>,
    on_page: &mut dyn FnMut(&mut egui::Ui, usize, egui::Rect, &PageMap),
) -> CanvasFrame {
    view.set_ppp(ui.ctx().pixels_per_point());
    view.set_max_side(ui.ctx().input(|i| i.max_texture_side) as f32);
    let paged = view.mode.is_paged();
    let (win_top, win_h) = view.window();
    let mut area = egui::ScrollArea::both().auto_shrink(false);
    if let Some(y) = jump_to {
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    let out = area.show_viewport(ui, |ui, viewport| {
        let total = egui::vec2(view.layout.max_width(), win_h);
        ui.set_min_size(total);
        let origin = ui.max_rect().min;
        let left = origin.x + (ui.max_rect().width() - total.x).max(0.0) / 2.0;
        let (visible, around) = if paged {
            let row = view.layout.row_pages(view.spread);
            let around = view.layout.expand(row.clone(), view.mode.columns());
            (row, around)
        } else {
            let visible = view.layout.visible_range(viewport.min.y, viewport.height());
            let around = view.layout.expand(visible.clone(), 1);
            (visible, around)
        };
        for i in around {
            let scale = view.scale_for(i);
            view.request_at(worker, i as i32, scale);
        }
        for i in visible.clone() {
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    left + view.layout.page_x(i),
                    origin.y + view.layout.page_top(i) - win_top,
                ),
                egui::vec2(view.layout.page_width(i), view.layout.page_height(i)),
            );
            let scale = view.scale_for(i);
            paint_page(ui, view, i as i32, rect, scale);
            let map = view.page_map(i, (rect.min.x, rect.min.y));
            on_page(ui, i, rect, &map);
        }
        if paged {
            visible.start
        } else {
            view.layout
                .page_at(viewport.min.y + viewport.height() * 0.3)
        }
    });
    let turn = if paged && ui.rect_contains_pointer(out.inner_rect) {
        let max_offset = (out.content_size.y - out.inner_rect.height()).max(0.0);
        paged_step(out.state.offset.y, max_offset, wheel_y(ui))
    } else {
        0
    };
    CanvasFrame {
        scroll_y: out.state.offset.y,
        current_page: out.inner,
        viewport_h: out.inner_rect.height(),
        turn,
    }
}

/// Giro da roda neste quadro (sem Ctrl, que é zoom). Lido dos eventos brutos porque a área de
/// rolagem consome o delta suavizado.
fn wheel_y(ui: &egui::Ui) -> f32 {
    ui.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, modifiers, .. } if !modifiers.command => {
                    Some(delta.y)
                }
                _ => None,
            })
            .sum()
    })
}
```

A função `paint_page` continua como está.

- [ ] **Passo 5: Ajustar `edit.rs` para o `PageMap`**

- Acrescente `use app_state::transform::PageMap;`.
- Remova `page_point` e `screen_point`.
- Troque `screen_rect` e `paint_stroke` por:

```rust
fn to_pos(p: (f32, f32)) -> egui::Pos2 {
    egui::pos2(p.0, p.1)
}

/// Retângulo da página (pontos) -> retângulo na tela.
pub(crate) fn screen_rect(r: &PdfRect, map: &PageMap) -> egui::Rect {
    let (min, max) = map.rect_to_screen(r);
    egui::Rect::from_min_max(to_pos(min), to_pos(max))
}

fn paint_stroke(painter: &egui::Painter, points: &[(f32, f32)], map: &PageMap) {
    let pts = points.iter().map(|&p| to_pos(map.to_screen(p))).collect();
    painter.line(pts, egui::Stroke::new(2.0, egui::Color32::RED));
}
```

- Mudanças de assinatura (corpos iguais, trocando `rect.min, zoom` por `map`):
  - `on_page(&mut self, ui, worker, page: i32, rect: egui::Rect, map: &PageMap)`. O `pointer`
    passa a ser `.map(|p| map.to_page((p.x, p.y)))`.
  - `drag_select(&mut self, ui, worker, response, pointer, page, map: &PageMap)`, com
    `screen_rect(&area, map)` e sem o `#[allow(clippy::too_many_arguments)]`.
  - `draw_ink(&mut self, ui, response, pointer, page, map: &PageMap)`
  - `paint_pending(&self, painter, page, map: &PageMap)`. A nota usa
    `egui::Rect::from_min_size(to_pos(map.to_screen(*at)), egui::vec2(24.0 * map.zoom, 24.0 * map.zoom))`.
- Nos testes, remova `screen_and_page_coordinates_round_trip` (agora coberto em `transform.rs`) e
  troque o teste de `screen_rect` por:

```rust
    #[test]
    fn screen_rect_scales_and_offsets_a_page_rect() {
        let map = PageMap {
            origin: (100.0, 50.0),
            zoom: 2.0,
            size: (612.0, 792.0),
            rotation: pdf_core::Rotation::R0,
        };
        let r = screen_rect(&PdfRect::new(10.0, 20.0, 30.0, 40.0), &map);
        assert_eq!(
            (r.min, r.max),
            (egui::pos2(120.0, 90.0), egui::pos2(160.0, 130.0))
        );
    }
```

- [ ] **Passo 6: Ajustar `doc_tab.rs` e `bench.rs`**

Em `doc_tab.rs`:

1. Imports: acrescente `use app_state::layout::ViewMode; use app_state::transform::PageMap; use app_state::zoom::Fit;`.
2. Acrescente o enum, acima da struct:

```rust
/// Ações sobre o documento, vindas de botões, menus ou atalhos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DocCommand {
    Save,
    Undo,
    Redo,
    /// Leva o foco ao campo de busca.
    Find,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fit(Fit),
    Mode(ViewMode),
    /// Duas páginas: liga/desliga a capa sozinha.
    ToggleCover,
    Rotate { clockwise: bool },
}
```

3. Campos novos em `DocTab`: `pub(crate) focus_search: bool` e
   `/// Última virada de página pela roda (tempo do egui), para não virar várias de uma vez. turned_at: f64`,
   iniciados com `false` e `f64::NEG_INFINITY`.
4. Método novo:

```rust
    pub fn run(&mut self, cmd: DocCommand) {
        match cmd {
            DocCommand::Save => return self.save(),
            DocCommand::Undo => return drop(self.edit.undo()),
            DocCommand::Redo => return drop(self.edit.redo()),
            DocCommand::Find => return self.focus_search = true,
            _ => {}
        }
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let page = self.last.current_page as i32;
        match cmd {
            DocCommand::ZoomIn | DocCommand::ZoomOut | DocCommand::ZoomReset => {
                let z = match cmd {
                    DocCommand::ZoomIn => zoom::zoom_in(view.zoom),
                    DocCommand::ZoomOut => zoom::zoom_out(view.zoom),
                    _ => zoom::DEFAULT_ZOOM,
                };
                let ratio = view.set_manual_zoom(z);
                self.jump_to = Some(self.last.scroll_y * ratio);
            }
            DocCommand::Fit(fit) => view.fit = fit,
            DocCommand::Mode(mode) => {
                view.set_mode(mode, page.max(0) as usize);
                self.jump_to = view.go_to(page, None);
            }
            DocCommand::ToggleCover => {
                view.cover = !view.cover;
                view.set_mode(view.mode, page.max(0) as usize);
                self.jump_to = view.go_to(page, None);
            }
            DocCommand::Rotate { clockwise } => {
                let r = if clockwise { view.rotation.cw() } else { view.rotation.ccw() };
                view.set_rotation(r);
                self.jump_to = view.go_to(page, None);
            }
            DocCommand::Save | DocCommand::Undo | DocCommand::Redo | DocCommand::Find => {}
        }
    }

    pub fn save(&mut self) {
        if let Some(view) = self.view.as_ref() {
            self.edit.save(&self.worker, view.path.clone());
        }
    }
```

   Se o clippy reclamar de `return drop(...)` ou `return self.focus_search = true`, escreva em
   blocos: `{ self.edit.undo(); return; }`.

5. `poll`:
   - O braço `Response::Rendered { page, scale, invert, rotation, image }` passa `rotation` para
     `view.on_rendered(ctx, page, scale, invert, rotation, image)`.
   - Troque `self.view.as_ref()?.scroll_target(p, r.y0)` por
     `self.view.as_mut()?.go_to(p, Some((r.x0, r.y0)))`.
6. `toolbar`:
   - Os botões de zoom chamam `self.run(DocCommand::ZoomOut)` e `self.run(DocCommand::ZoomIn)`.
     Saia do `let Some(view) = self.view.as_mut()` antes, guardando o que precisar, ou use a forma
     de comandos coletados abaixo.
   - "Largura" vira `DocCommand::Fit(Fit::Width)`.
   - Acrescente o botão "Página" (`DocCommand::Fit(Fit::Page)`), o seletor de modo e os botões de
     girar.
   - Troque `view.scroll_target(page as i32 - 1, 0.0)` por `view.go_to(page as i32 - 1, None)` e
     `view.scroll_target(page, rect.y0)` por `view.go_to(page, Some((rect.x0, rect.y0)))`.
   - O `consume_key(COMMAND, F)` passa a ser
     `if std::mem::take(&mut self.focus_search) || ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) { edit.request_focus(); }`.

   Para o borrow checker, junte os comandos num `Vec` e aplique ao fim:

```rust
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let mut commands = Vec::new();
        ui.horizontal(|ui| {
            // … (código atual: ferramentas, desfazer/refazer, salvar, tema) …
            if ui.button("−").clicked() {
                commands.push(DocCommand::ZoomOut);
            }
            ui.label(format!("{:.0}%", view.zoom * 100.0));
            if ui.button("+").clicked() {
                commands.push(DocCommand::ZoomIn);
            }
            if ui.button("Largura").clicked() {
                commands.push(DocCommand::Fit(Fit::Width));
            }
            if ui.button("Página").clicked() {
                commands.push(DocCommand::Fit(Fit::Page));
            }
            let mut mode = view.mode;
            egui::ComboBox::from_id_salt("modo")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m in ViewMode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
            if mode != view.mode {
                commands.push(DocCommand::Mode(mode));
            }
            if ui.button("⟲").on_hover_text("Girar anti-horário").clicked() {
                commands.push(DocCommand::Rotate { clockwise: false });
            }
            if ui.button("⟳").on_hover_text("Girar horário").clicked() {
                commands.push(DocCommand::Rotate { clockwise: true });
            }
            // … (página e busca, com `go_to`) …
        });
        for cmd in commands {
            self.run(cmd);
        }
    }
```

7. `sidebar`: `view.scroll_target(page as i32, 0.0)` vira `view.go_to(page as i32, None)`.
8. `canvas`: troque o bloco `if view.fit_pending { … }` por:

```rust
        if let Some(ratio) = view.apply_fit(ui.available_width() - 24.0, ui.available_height() - 24.0) {
            self.jump_to = Some(self.last.scroll_y * ratio);
        }
```

   E o callback e a virada de página por:

```rust
        let (worker, edit, search) = (&self.worker, &mut self.edit, &self.search);
        self.last = canvas::show_pages(
            ui,
            view,
            worker,
            self.jump_to.take(),
            &mut |ui, page, rect, map: &PageMap| {
                paint_search_hits(ui, search, page as i32, map);
                edit.on_page(ui, worker, page as i32, rect, map);
            },
        );
        let now = ui.input(|i| i.time);
        if self.last.turn != 0 && now - self.turned_at > 0.25 && view.turn(self.last.turn) {
            self.turned_at = now;
            self.jump_to = Some(0.0);
        }
```

   Remova a variável `zoom` que não é mais usada. `paint_search_hits` passa a receber
   `map: &PageMap` no lugar de `page_rect, zoom` e usa `screen_rect(r, map)`.
9. `handle_input`:
   - O bloco de navegação trata os modos página a página:

```rust
            if let (Some((key, _)), Some(view)) = (pressed, self.view.as_mut()) {
                if view.mode.is_paged() {
                    match key {
                        NavKey::PageUp => drop(view.turn(-1)),
                        NavKey::PageDown => drop(view.turn(1)),
                        NavKey::Home => view.turn_to(0),
                        NavKey::End => view.turn_to(usize::MAX),
                    }
                    self.jump_to = Some(0.0);
                } else {
                    let (scroll, height) = (self.last.scroll_y, self.last.viewport_h);
                    self.jump_to = Some(nav_target(key, scroll, height, view.layout.total_height()));
                }
            }
            // setas e espaço viram a página nos modos página a página
            if let Some(view) = self.view.as_mut().filter(|v| v.mode.is_paged()) {
                let step = ctx.input_mut(|i| {
                    [(egui::Key::ArrowRight, 1), (egui::Key::Space, 1), (egui::Key::ArrowLeft, -1)]
                        .into_iter()
                        .find(|&(k, _)| i.consume_key(egui::Modifiers::NONE, k))
                        .map(|(_, s)| s)
                });
                if let Some(step) = step.filter(|&s| view.turn(s)) {
                    let _ = step;
                    self.jump_to = Some(0.0);
                }
            }
```

   - Ctrl+roda: `view.set_zoom(view.zoom * zoom_delta)` vira `view.set_manual_zoom(…)`.
   - Desfazer, refazer e salvar viram `self.run(DocCommand::Redo)`, `self.run(DocCommand::Undo)`
     e `self.run(DocCommand::Save)`.

   (Se o clippy reclamar do `drop(bool)`, use `let _ = view.turn(-1);`.)

Em `bench.rs`, linha 88: `let target = view.go_to(*next as i32, None);`.

- [ ] **Passo 7: Rodar a skill `simplify` sobre `view.rs`, `canvas.rs`, `edit.rs` e `doc_tab.rs`**

- [ ] **Passo 8: Rodar os testes e o app**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: tudo passa, inclusive os 5 testes novos de `view.rs`.

Run: `cargo run --release -q -p gera-ui -- <cópia de um PDF>`
Esperado:
- Os 4 modos funcionam. Em "Página única" e "Duas páginas", a roda no fim da página passa para a
  seguinte e PageDown e as setas viram a página. **Se a roda virar no sentido contrário**,
  inverta o sinal de `wheel_y` em `canvas.rs` (e anote no commit).
- ⟳ gira, e destacar texto com a vista a 90° marca o texto certo. Confira salvando e reabrindo.
- "Página" ajusta a página inteira, e redimensionar a janela reajusta até mexer no zoom.

- [ ] **Passo 9: Commit**

```bash
git add crates/gera-ui
git commit -m "feat(gera-ui): modos de exibição, girar vista e ajustar à página" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 6: `app-state` — regras das abas

**Arquivos:**
- Criar: `crates/app-state/src/tabs.rs`
- Modificar: `crates/app-state/src/lib.rs` (`pub mod tabs;`)

**Interfaces:**
- Produz:
  - `pub const HOME: usize = 0`, `pub const TOOLS: usize = 1` e `pub const FIXED: usize = 2`.
  - `pub fn canonical(path: &Path) -> PathBuf`
  - `pub fn tab_title(path: &Path, max: usize) -> String`
  - `pub struct TabList` (`Default`, `Clone`, `Debug`), com os métodos:
    - `len()`, `active()`, `docs() -> &[PathBuf]`
    - `activate(tab)`
    - `doc_index(tab) -> Option<usize>`
    - `find(path) -> Option<usize>`
    - `open(path) -> (usize, bool)`
    - `close(tab) -> Option<usize>`
    - `rename(tab, path)`
    - `next()`, `prev()`

- [ ] **Passo 1: Escrever os testes que falham**

Crie `crates/app-state/src/tabs.rs` com:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!("/docs/{name}.pdf"))
    }

    fn three() -> TabList {
        let mut t = TabList::default();
        for n in ["a", "b", "c"] {
            t.open(p(n));
        }
        t
    }

    #[test]
    fn starts_on_home_with_the_fixed_tabs() {
        let t = TabList::default();
        assert_eq!((t.len(), t.active()), (FIXED, HOME));
        assert_eq!(t.doc_index(HOME), None);
        assert_eq!(t.doc_index(TOOLS), None);
    }

    #[test]
    fn opening_activates_and_opening_again_reuses_the_tab() {
        let mut t = three();
        assert_eq!(t.active(), FIXED + 2);
        assert_eq!(t.open(p("a")), (FIXED, false));
        assert_eq!(t.active(), FIXED);
        assert_eq!(t.len(), FIXED + 3);
    }

    #[test]
    fn closing_the_active_tab_picks_the_right_then_left_neighbour_then_home() {
        let mut t = three();
        t.activate(FIXED + 1); // b
        assert_eq!(t.close(FIXED + 1), Some(1));
        assert_eq!(t.docs(), [p("a"), p("c")]);
        assert_eq!(t.active(), FIXED + 1, "c ficou no lugar de b");
        assert_eq!(t.close(FIXED + 1), Some(1)); // c, a última
        assert_eq!(t.active(), FIXED, "volta para a vizinha da esquerda");
        assert_eq!(t.close(FIXED), Some(0));
        assert_eq!(t.active(), HOME, "sem documentos, volta ao Início");
    }

    #[test]
    fn closing_another_tab_keeps_the_same_document_active() {
        let mut t = three(); // ativa: c
        t.close(FIXED);
        assert_eq!(t.doc_index(t.active()).map(|i| &t.docs()[i]), Some(&p("c")));
    }

    #[test]
    fn fixed_tabs_never_close() {
        let mut t = three();
        assert_eq!(t.close(HOME), None);
        assert_eq!(t.close(TOOLS), None);
        assert_eq!(t.close(99), None);
        assert_eq!(t.len(), FIXED + 3);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut t = three();
        t.next();
        assert_eq!(t.active(), HOME);
        t.prev();
        assert_eq!(t.active(), FIXED + 2);
    }

    #[test]
    fn rename_follows_save_as() {
        let mut t = three();
        t.rename(FIXED, p("novo"));
        assert_eq!(t.find(&p("novo")), Some(FIXED));
        assert_eq!(t.find(&p("a")), None);
    }

    #[test]
    fn canonical_unifies_relative_and_symlinked_paths() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.pdf");
        std::fs::write(&real, b"%PDF").unwrap();
        let link = dir.path().join("atalho.pdf");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let dotted = dir.path().join(".").join("real.pdf");
        assert_eq!(canonical(&link), canonical(&real));
        assert_eq!(canonical(&dotted), canonical(&real));
        let missing = dir.path().join("sumiu.pdf");
        assert_eq!(canonical(&missing), missing, "inexistente: devolve o próprio caminho");
    }

    #[test]
    fn titles_are_the_file_name_truncated_on_char_boundaries() {
        assert_eq!(tab_title(Path::new("/x/relatório.pdf"), 40), "relatório.pdf");
        assert_eq!(tab_title(Path::new("/x/ação-çãoçãoção.pdf"), 6), "ação-…");
        assert_eq!(tab_title(Path::new("/"), 10), "/");
    }
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state tabs -q 2>&1 | tail -5`
Esperado: erro de compilação.

- [ ] **Passo 3: Implementar (acima dos testes)**

```rust
//! Regras das abas: Início e Ferramentas fixas, seguidas dos documentos abertos.
use std::path::{Path, PathBuf};

pub const HOME: usize = 0;
pub const TOOLS: usize = 1;
/// Quantidade de abas fixas: os documentos começam neste índice.
pub const FIXED: usize = 2;

/// Identidade de um documento: o caminho canônico, ou o próprio caminho se não der para resolver.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Nome do arquivo para a aba, com no máximo `max` caracteres (o excesso vira "…").
pub fn tab_title(path: &Path, max: usize) -> String {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned();
    if name.chars().count() <= max {
        return name;
    }
    let keep: String = name.chars().take(max.saturating_sub(1)).collect();
    format!("{keep}…")
}

/// Abas abertas (só a identidade de cada documento) e qual está ativa.
#[derive(Debug, Default, Clone)]
pub struct TabList {
    docs: Vec<PathBuf>,
    active: usize,
}

impl TabList {
    pub fn len(&self) -> usize {
        FIXED + self.docs.len()
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn docs(&self) -> &[PathBuf] {
        &self.docs
    }
    pub fn activate(&mut self, tab: usize) {
        if tab < self.len() {
            self.active = tab;
        }
    }
    /// Posição do documento da aba `tab` em `docs`, se for uma aba de documento.
    pub fn doc_index(&self, tab: usize) -> Option<usize> {
        tab.checked_sub(FIXED).filter(|&i| i < self.docs.len())
    }
    pub fn find(&self, path: &Path) -> Option<usize> {
        self.docs.iter().position(|p| p == path).map(|i| FIXED + i)
    }
    /// Ativa a aba de `path`, criando-a se preciso. Devolve `(aba, criada)`.
    pub fn open(&mut self, path: PathBuf) -> (usize, bool) {
        if let Some(tab) = self.find(&path) {
            self.active = tab;
            return (tab, false);
        }
        self.docs.push(path);
        self.active = self.len() - 1;
        (self.active, true)
    }
    /// Fecha a aba `tab` (as fixas não fecham). Devolve a posição do documento removido.
    pub fn close(&mut self, tab: usize) -> Option<usize> {
        let i = self.doc_index(tab)?;
        self.docs.remove(i);
        if self.active == tab {
            self.active = if tab < self.len() {
                tab
            } else if tab > FIXED {
                tab - 1
            } else {
                HOME
            };
        } else if self.active > tab {
            self.active -= 1;
        }
        Some(i)
    }
    pub fn rename(&mut self, tab: usize, path: PathBuf) {
        if let Some(i) = self.doc_index(tab) {
            self.docs[i] = path;
        }
    }
    pub fn next(&mut self) {
        self.active = (self.active + 1) % self.len();
    }
    pub fn prev(&mut self) {
        self.active = (self.active + self.len() - 1) % self.len();
    }
}
```

(O clippy pode pedir `is_empty`, por causa de `len_without_is_empty`. Nesse caso acrescente
`pub fn is_empty(&self) -> bool { false }` com o comentário "as abas fixas sempre existem".)

- [ ] **Passo 4: Rodar a skill `simplify` sobre `tabs.rs`**

- [ ] **Passo 5: Rodar os testes**

Run: `cargo test -p app-state tabs -q 2>&1 | grep -E "test result|FAILED"`
Esperado: 9 testes passam.

- [ ] **Passo 6: Commit**

```bash
git add crates/app-state
git commit -m "feat(app-state): regras das abas (fixas, duplicadas, fechar e vizinha)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 7: `app-state` — gravação segura, recentes e preferências

**Arquivos:**
- Criar: `crates/app-state/src/store.rs`, `crates/app-state/src/recents.rs`, `crates/app-state/src/prefs.rs`
- Modificar: `crates/app-state/src/lib.rs`, `crates/app-state/Cargo.toml`

**Interfaces:**
- Produz:
  - `store`:
    - `config_file(name: &str) -> Option<PathBuf>`
    - `load_json<T: DeserializeOwned + Default>(path: &Path) -> T`
    - `save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()>`
  - `recents`:
    - `MAX_RECENTS = 20`
    - `pub struct Recent { pub path: PathBuf, pub opened_at: u64 }`
    - `pub struct RecentFiles`, com `load(path)`, `save(&self, path)`, `items()`,
      `add(path, now)` e `remove(path)`
    - `now_secs() -> u64`
    - `ago(now: u64, then: u64) -> String`
  - `prefs`:
    - `pub enum LeftPanel { Thumbnails, Outline, Annotations }`, com `ALL`, `label()` e `icon()`
    - `pub struct Prefs { pub dark: bool, pub left_panel: Option<LeftPanel>, pub left_width: f32, pub tools_panel: bool, pub tools_width: f32 }`,
      com `load(path)`, `sanitized()`, `MIN_PANEL = 120.0` e `MAX_PANEL = 600.0`

- [ ] **Passo 1: Dependências**

Em `crates/app-state/Cargo.toml`, acrescente a `[dependencies]`:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Passo 2: Escrever os testes que falham**

`crates/app-state/src/store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Sample {
        n: u32,
        s: String,
    }

    #[test]
    fn saves_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/pasta/x.json");
        let v = Sample { n: 7, s: "olá".into() };
        save_json(&path, &v).unwrap();
        assert_eq!(load_json::<Sample>(&path), v);
        assert!(!path.with_extension("json.tmp").exists(), "o temporário some");
    }

    #[test]
    fn corrupt_or_missing_file_gives_default_and_keeps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        assert_eq!(load_json::<Sample>(&path), Sample::default());
        std::fs::write(&path, b"{ isto nao e json").unwrap();
        assert_eq!(load_json::<Sample>(&path), Sample::default());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ isto nao e json", "nada é apagado ao ler");
    }

    #[test]
    fn unwritable_location_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("arquivo");
        std::fs::write(&file, b"x").unwrap();
        // a "pasta" de destino é um arquivo comum
        assert!(save_json(&file.join("x.json"), &Sample::default()).is_err());
    }
}
```

`crates/app-state/src/recents.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: usize) -> PathBuf {
        PathBuf::from(format!("/d/{n}.pdf"))
    }

    #[test]
    fn newest_first_without_duplicates_and_limited() {
        let mut r = RecentFiles::default();
        for n in 0..25 {
            r.add(p(n), n as u64);
        }
        assert_eq!(r.items().len(), MAX_RECENTS);
        assert_eq!(r.items()[0].path, p(24));
        r.add(p(10), 100);
        assert_eq!(r.items()[0], Recent { path: p(10), opened_at: 100 });
        assert_eq!(r.items().iter().filter(|x| x.path == p(10)).count(), 1);
        r.remove(&p(10));
        assert!(r.items().iter().all(|x| x.path != p(10)));
    }

    #[test]
    fn load_tolerates_extra_fields_and_trims_long_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recentes.json");
        let items: Vec<String> = (0..30)
            .map(|n| format!(r#"{{"path":"/d/{n}.pdf","opened_at":{n},"cor":"azul"}}"#))
            .collect();
        std::fs::write(&path, format!(r#"{{"items":[{}],"versao":9}}"#, items.join(","))).unwrap();
        let r = RecentFiles::load(&path);
        assert_eq!(r.items().len(), MAX_RECENTS);
        assert_eq!(r.items()[0].path, p(0));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recentes.json");
        let mut r = RecentFiles::default();
        r.add(p(1), 5);
        r.save(&path).unwrap();
        assert_eq!(RecentFiles::load(&path), r);
    }

    #[test]
    fn ago_is_short_and_human() {
        assert_eq!(ago(1000, 1000), "agora");
        assert_eq!(ago(1000, 2000), "agora", "datas no futuro contam como agora");
        assert_eq!(ago(10_000, 10_000 - 300), "há 5 min");
        assert_eq!(ago(100_000, 100_000 - 7200), "há 2 h");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400), "há 1 dia");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "há 3 dias");
    }
}
```

`crates/app-state/src/prefs.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = Prefs::load(&dir.path().join("prefs.json"));
        assert_eq!(p, Prefs::default());
        assert_eq!(p.left_panel, Some(LeftPanel::Thumbnails));
        assert!(p.tools_panel && !p.dark);
    }

    #[test]
    fn widths_are_kept_inside_the_limits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        std::fs::write(&path, br#"{"left_width": 5000, "tools_width": -3, "dark": true}"#).unwrap();
        let p = Prefs::load(&path);
        assert_eq!((p.left_width, p.tools_width), (MAX_PANEL, MIN_PANEL));
        assert!(p.dark, "os outros campos valem");
        assert_eq!(p.left_panel, Some(LeftPanel::Thumbnails), "campo ausente: padrão");
    }

    #[test]
    fn panels_have_distinct_labels() {
        let labels: std::collections::HashSet<_> = LeftPanel::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(labels.len(), 3);
    }
}
```

Acrescente `pub mod prefs; pub mod recents; pub mod store;` a `lib.rs`, em ordem alfabética.

- [ ] **Passo 3: Rodar e ver falhar**

Run: `cargo test -p app-state -q 2>&1 | tail -5`
Esperado: erros de compilação.

- [ ] **Passo 4: Implementar**

`store.rs` (acima dos testes):

```rust
//! Arquivos JSON do app na pasta de configuração, gravados sem risco de deixar um arquivo pela metade.
use serde::{de::DeserializeOwned, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "gera-pdf-editor";

/// Arquivo `name` na pasta de configuração do app (`$XDG_CONFIG_HOME` ou `~/.config`).
pub fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(APP_DIR).join(name))
}

/// Lê `path`; arquivo ausente, ilegível ou inválido vira o valor padrão (e não é apagado).
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Grava em um temporário na mesma pasta, força ao disco e só então troca o arquivo.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("caminho sem pasta"))?;
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(&bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}
```

`recents.rs` (acima dos testes):

```rust
//! Arquivos abertos recentemente (aba Início e Arquivo → Abrir recente).
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_RECENTS: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    /// Segundos desde 1970 (UTC).
    pub opened_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentFiles {
    items: Vec<Recent>,
}

impl RecentFiles {
    pub fn load(path: &Path) -> Self {
        let mut r: Self = store::load_json(path);
        r.items.truncate(MAX_RECENTS);
        r
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        store::save_json(path, self)
    }

    /// O mais recente primeiro.
    pub fn items(&self) -> &[Recent] {
        &self.items
    }

    pub fn add(&mut self, path: PathBuf, now: u64) {
        self.items.retain(|r| r.path != path);
        self.items.insert(0, Recent { path, opened_at: now });
        self.items.truncate(MAX_RECENTS);
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|r| r.path != path);
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Há quanto tempo, em texto curto ("agora", "há 5 min", "há 3 h", "há 2 dias").
pub fn ago(now: u64, then: u64) -> String {
    match now.saturating_sub(then) {
        0..=59 => "agora".into(),
        s @ 60..=3599 => format!("há {} min", s / 60),
        s @ 3600..=86_399 => format!("há {} h", s / 3600),
        s => match s / 86_400 {
            1 => "há 1 dia".into(),
            d => format!("há {d} dias"),
        },
    }
}
```

`prefs.rs` (acima dos testes):

```rust
//! Preferências da interface, guardadas entre sessões.
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MIN_PANEL: f32 = 120.0;
pub const MAX_PANEL: f32 = 600.0;

/// Painel aberto pelo trilho à esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LeftPanel {
    #[default]
    Thumbnails,
    Outline,
    Annotations,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 3] = [LeftPanel::Thumbnails, LeftPanel::Outline, LeftPanel::Annotations];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "Miniaturas",
            LeftPanel::Outline => "Índice",
            LeftPanel::Annotations => "Anotações",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            LeftPanel::Thumbnails => "▤",
            LeftPanel::Outline => "☰",
            LeftPanel::Annotations => "💬",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub dark: bool,
    pub left_panel: Option<LeftPanel>,
    pub left_width: f32,
    /// Painel de ferramentas à direita aberto?
    pub tools_panel: bool,
    pub tools_width: f32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            dark: false,
            left_panel: Some(LeftPanel::Thumbnails),
            left_width: 180.0,
            tools_panel: true,
            tools_width: 200.0,
        }
    }
}

impl Prefs {
    pub fn load(path: &Path) -> Self {
        store::load_json::<Self>(path).sanitized()
    }

    /// Larguras dentro de `MIN_PANEL..=MAX_PANEL` (valores estranhos voltam ao padrão).
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        let fix = |w: f32, default: f32| if w.is_finite() { w.clamp(MIN_PANEL, MAX_PANEL) } else { default };
        self.left_width = fix(self.left_width, defaults.left_width);
        self.tools_width = fix(self.tools_width, defaults.tools_width);
        self
    }
}
```

- [ ] **Passo 5: Rodar a skill `simplify` sobre `store.rs`, `recents.rs` e `prefs.rs`**

- [ ] **Passo 6: Rodar os testes**

Run: `cargo test -p app-state -q 2>&1 | grep -E "test result|FAILED"`
Esperado: todos passam.

- [ ] **Passo 7: Commit**

```bash
git add crates/app-state Cargo.lock
git commit -m "feat(app-state): recentes e preferências com gravação segura" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 8: Casca com abas, abrir arquivos e fechar com segurança

**Arquivos:**
- Criar: `crates/gera-ui/src/open_dialog.rs`, `crates/gera-ui/src/shell/mod.rs`, `crates/gera-ui/src/shell/tabs.rs`, `crates/gera-ui/src/shell/close.rs`
- Reescrever: `crates/gera-ui/src/app.rs`
- Modificar: `crates/gera-ui/src/doc_tab.rs`, `crates/gera-ui/src/main.rs`, `crates/gera-ui/Cargo.toml`

**Interfaces:**
- Consome: `TabList`/`canonical`/`tab_title`/`HOME`/`TOOLS`/`FIXED` (tarefa 6), `RecentFiles`/`now_secs`/`store::config_file`/`Prefs` (tarefa 7).
- Produz:
  - `FileDialogs`, com `open(ctx)`, `save_as(ctx, doc: PathBuf)`, `busy()` e
    `poll() -> Option<Picked>`.
  - `pub enum Picked { Open(Vec<PathBuf>), SaveAs { doc: PathBuf, dest: PathBuf }, Cancelled, Failed }`
  - `CloseFlow`, com `new(dirty: Vec<PathBuf>, quit: bool)`, `current()`, `is_saving()`,
    `is_done()`, `quit()`, `answer(Answer) -> Step` e `saved(ok: bool) -> Step`.
  - `pub enum Answer { Save, Discard, Cancel }` e `pub enum Step { Save(PathBuf), Close(PathBuf), Abort }`
  - `shell::tabs::show(ui, active, labels: &[TabLabel]) -> Option<TabAction>`
  - Em `DocTab`:
    - `DocTab::new(ctx, path: PathBuf, dark: bool)` (agora recebe o caminho e o tema)
    - `pub(crate) path: PathBuf`
    - `suspend()`
    - `take_opened() -> bool`
    - `take_failure() -> Option<String>`
    - `take_renamed() -> Option<PathBuf>`
    - `save_as(dest)`
    - `title()`
  - `EditState::save_as(worker, dest)`

- [ ] **Passo 1: Escrever os testes que falham (`CloseFlow`)**

Crie `crates/gera-ui/src/shell/close.rs` só com os testes:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: &str) -> PathBuf {
        PathBuf::from(format!("/d/{n}.pdf"))
    }

    #[test]
    fn asks_one_document_at_a_time_and_discard_moves_on() {
        let mut f = CloseFlow::new(vec![p("a"), p("b")], true);
        assert_eq!(f.current(), Some(&p("a")));
        assert_eq!(f.answer(Answer::Discard), Step::Close(p("a")));
        assert_eq!(f.current(), Some(&p("b")));
        assert_eq!(f.answer(Answer::Discard), Step::Close(p("b")));
        assert!(f.is_done() && f.quit());
    }

    #[test]
    fn save_waits_for_the_result_before_closing() {
        let mut f = CloseFlow::new(vec![p("a")], false);
        assert_eq!(f.answer(Answer::Save), Step::Save(p("a")));
        assert!(f.is_saving() && !f.is_done());
        assert_eq!(f.saved(true), Step::Close(p("a")));
        assert!(f.is_done() && !f.quit());
    }

    #[test]
    fn a_failed_save_or_cancel_stops_everything_and_keeps_the_tabs() {
        let mut f = CloseFlow::new(vec![p("a"), p("b")], true);
        f.answer(Answer::Save);
        assert_eq!(f.saved(false), Step::Abort);
        assert!(f.is_done(), "nada mais é fechado");
        let mut g = CloseFlow::new(vec![p("a"), p("b")], true);
        assert_eq!(g.answer(Answer::Cancel), Step::Abort);
        assert!(g.is_done());
    }

    #[test]
    fn answers_after_the_end_do_nothing() {
        let mut f = CloseFlow::new(vec![], false);
        assert!(f.is_done());
        assert_eq!(f.answer(Answer::Discard), Step::Abort);
        assert_eq!(f.saved(true), Step::Abort);
    }
}
```

Crie `crates/gera-ui/src/shell/mod.rs`:

```rust
//! Casca no arranjo do Acrobat: abas, menus, barras e painéis.
pub mod close;
pub mod tabs;
```

Em `main.rs`, acrescente `mod open_dialog;` e `mod shell;` à lista de módulos.

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p gera-ui close -q 2>&1 | tail -5`
Esperado: erro de compilação.

- [ ] **Passo 3: Implementar `close.rs` (acima dos testes)**

```rust
//! Fechar abas com alterações pendentes: pergunta uma por vez e espera cada salvamento.
use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Cancel,
}

/// O que a casca deve fazer depois de uma resposta.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Salvar este documento e esperar o resultado.
    Save(PathBuf),
    /// Fechar este documento.
    Close(PathBuf),
    /// Parar tudo: cancelado ou falha ao salvar (as abas restantes ficam abertas).
    Abort,
}

#[derive(Debug, Default)]
pub struct CloseFlow {
    queue: VecDeque<PathBuf>,
    quit: bool,
    saving: bool,
}

impl CloseFlow {
    /// `dirty`: documentos com alterações pendentes; `quit`: sair do app no fim.
    pub fn new(dirty: Vec<PathBuf>, quit: bool) -> Self {
        Self {
            queue: dirty.into(),
            quit,
            saving: false,
        }
    }

    /// Documento sobre o qual se pergunta (ou que está sendo salvo).
    pub fn current(&self) -> Option<&PathBuf> {
        self.queue.front()
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn quit(&self) -> bool {
        self.quit
    }

    pub fn answer(&mut self, answer: Answer) -> Step {
        let Some(front) = self.queue.front().cloned() else {
            return Step::Abort;
        };
        match answer {
            Answer::Save => {
                self.saving = true;
                Step::Save(front)
            }
            Answer::Discard => {
                self.queue.pop_front();
                Step::Close(front)
            }
            Answer::Cancel => self.abort(),
        }
    }

    /// O salvamento do documento atual terminou (`ok`: sem pendências restantes).
    pub fn saved(&mut self, ok: bool) -> Step {
        if !self.saving {
            return Step::Abort;
        }
        self.saving = false;
        match (ok, self.queue.pop_front()) {
            (true, Some(done)) => Step::Close(done),
            _ => self.abort(),
        }
    }

    fn abort(&mut self) -> Step {
        self.queue.clear();
        self.saving = false;
        self.quit = false;
        Step::Abort
    }
}

/// Janela "Salvar alterações em X?". Devolve a resposta quando o usuário clica.
pub fn ask(ui: &mut egui::Ui, name: &str) -> Option<Answer> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("salvar-alteracoes")).show(ui.ctx(), |ui| {
        ui.heading("Alterações não salvas");
        ui.label(format!("Salvar as alterações em “{name}” antes de fechar?"));
        ui.horizontal(|ui| {
            if ui.button("Salvar").clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Não salvar").clicked() {
                answer = Some(Answer::Discard);
            }
            if ui.button("Cancelar").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer
}
```

(O `abort` zera `quit`, então um fluxo cancelado nunca fecha o app. Ajuste o teste
`answers_after_the_end_do_nothing` se precisar: ele já espera `Abort`.)

- [ ] **Passo 4: Seletor de arquivos (`open_dialog.rs`) e dependência**

Em `crates/gera-ui/Cargo.toml`, `[dependencies]`: `rfd = "0.17"` (features padrão: xdg-portal,
sem GTK). **Meça o binário agora**:

Run: `cargo build --release -q -p gera-ui && stat -c%s target/release/gera-pdf-editor`
Esperado: menor que 26214400 (25 MB). Se passar, **pare** e avise. O spec (risco 1) prevê
trocar por chamada direta ao portal ou por `zenity`/`kdialog`.

```rust
//! Seletor de arquivos do sistema (portal XDG) numa thread à parte, para não travar a janela.
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Debug)]
pub enum Picked {
    Open(Vec<PathBuf>),
    /// "Salvar como" do documento `doc` para `dest`.
    SaveAs { doc: PathBuf, dest: PathBuf },
    Cancelled,
    /// O seletor não pôde ser aberto.
    Failed,
}

#[derive(Default)]
pub struct FileDialogs {
    pending: Option<Receiver<Picked>>,
}

impl FileDialogs {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn open(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, || {
            match rfd::FileDialog::new()
                .set_title("Abrir PDF")
                .add_filter("PDF", &["pdf", "PDF"])
                .pick_files()
            {
                Some(files) if !files.is_empty() => Picked::Open(files),
                _ => Picked::Cancelled,
            }
        });
    }

    pub fn save_as(&mut self, ctx: &egui::Context, doc: PathBuf) {
        self.spawn(ctx, move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Salvar como")
                .add_filter("PDF", &["pdf"]);
            if let Some(dir) = doc.parent() {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = doc.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            match dialog.save_file() {
                Some(dest) => Picked::SaveAs { doc, dest },
                None => Picked::Cancelled,
            }
        });
    }

    fn spawn(&mut self, ctx: &egui::Context, pick: impl FnOnce() -> Picked + Send + 'static) {
        if self.busy() {
            return; // um seletor por vez
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("seletor-arquivos".into())
            .spawn(move || {
                let _ = tx.send(pick());
                ctx.request_repaint();
            });
        self.pending = Some(rx);
        if started.is_err() {
            self.pending = None; // o canal fecha sem resposta: vira `Failed` abaixo
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(Picked::Failed);
            self.pending = Some(rx);
        }
    }

    /// Resultado do seletor, quando houver. Uma thread que morre sem responder conta como falha.
    pub fn poll(&mut self) -> Option<Picked> {
        let rx = self.pending.as_ref()?;
        let picked = match rx.try_recv() {
            Ok(p) => p,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Picked::Failed,
        };
        self.pending = None;
        Some(picked)
    }
}
```

- [ ] **Passo 5: Barra de abas (`shell/tabs.rs`)**

```rust
//! Barra de abas: Início e Ferramentas fixas, documentos com ✕ e o ＋.
use app_state::tabs::{FIXED, HOME, TOOLS};

pub struct TabLabel {
    pub title: String,
    pub tooltip: String,
    pub dirty: bool,
}

pub enum TabAction {
    Activate(usize),
    Close(usize),
    New,
}

pub fn show(ui: &mut egui::Ui, active: usize, docs: &[TabLabel]) -> Option<TabAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (tab, name) in [(HOME, "🏠 Início"), (TOOLS, "🧰 Ferramentas")] {
            if ui.selectable_label(active == tab, name).clicked() {
                action = Some(TabAction::Activate(tab));
            }
        }
        ui.separator();
        for (n, doc) in docs.iter().enumerate() {
            let tab = FIXED + n;
            let text = if doc.dirty {
                format!("{} •", doc.title)
            } else {
                doc.title.clone()
            };
            if ui
                .selectable_label(active == tab, text)
                .on_hover_text(&doc.tooltip)
                .clicked()
            {
                action = Some(TabAction::Activate(tab));
            }
            if ui.small_button("✕").on_hover_text("Fechar (Ctrl+W)").clicked() {
                action = Some(TabAction::Close(tab));
            }
        }
        if ui.button("＋").on_hover_text("Abrir arquivo (Ctrl+O)").clicked() {
            action = Some(TabAction::New);
        }
    });
    action
}
```

- [ ] **Passo 6: `DocTab` ganha identidade, suspensão e eventos para a casca**

Em `edit.rs`, troque `save` por:

```rust
    /// Pede ao worker para gravar as pendentes; ignora se não há nada ou já está salvando.
    pub fn save(&mut self, worker: &Worker, dest: PathBuf) {
        if self.pending.is_dirty() {
            self.save_as(worker, dest);
        }
    }

    /// Grava o documento em `dest` com as pendentes (mesmo sem nenhuma: "Salvar como").
    pub fn save_as(&mut self, worker: &Worker, dest: PathBuf) {
        if self.saving {
            return;
        }
        self.saving = true;
        worker.send(Request::Save {
            pending: self.pending.items().to_vec(),
            dest,
        });
    }
```

Em `doc_tab.rs`:

1. Campos novos:
   - `pub(crate) path: PathBuf`, a identidade da aba (caminho canônico)
   - `/// Tema escuro herdado da casca. dark: bool`
   - `/// Abriu com sucesso desde a última consulta. opened: bool`
   - `/// Falha ao abrir (a aba deve ser fechada). failure: Option<String>`
   - `/// "Salvar como" em andamento: novo caminho. save_dest: Option<PathBuf>`
   - `/// "Salvar como" concluído: a casca atualiza a aba. renamed: Option<PathBuf>`
2. `new`:

```rust
    pub fn new(ctx: &egui::Context, path: PathBuf, dark: bool) -> Self {
        let ctx = ctx.clone();
        Self {
            worker: Worker::spawn(move || ctx.request_repaint()),
            path,
            dark,
            status: String::new(),
            opened: false,
            failure: None,
            save_dest: None,
            renamed: None,
            // … demais campos como antes …
        }
    }
```

3. `on_opened`: troque `let dark = self.view.as_ref().is_some_and(|v| v.dark);` por
   `let dark = self.dark;` e, no fim, `self.opened = true;`.
4. `poll`, falhas de abertura: o braço `Response::OpenFailed(e)` (sem senha) passa a ser:

```rust
                Response::OpenFailed(e) => {
                    self.status = format!("Erro: {e}");
                    if self.view.is_none() {
                        self.failure = Some(format!("Não foi possível abrir {}: {e}", self.path.display()));
                    }
                }
```

5. `poll`, `Response::Saved`: depois de `self.status = "Salvo.".into();`, acrescente:

```rust
                    if let Some(dest) = self.save_dest.take() {
                        if let Some(view) = self.view.as_mut() {
                            view.path = dest.clone();
                        }
                        self.renamed = Some(dest);
                    }
```

   Em `Response::SaveFailed`, acrescente `self.save_dest = None;`.
6. Métodos novos:

```rust
    /// A aba saiu da frente: libera texturas e renders pendentes (só tamanhos, zoom, posição e
    /// anotações ficam). Ao voltar, as páginas visíveis são pedidas de novo.
    pub fn suspend(&mut self) {
        if let Some(view) = self.view.as_mut() {
            view.clear_cache();
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        if let Some(view) = self.view.as_mut() {
            view.dark = dark;
            view.clear_cache();
        }
    }

    pub fn save_as(&mut self, dest: PathBuf) {
        if self.view.is_some() && !self.edit.saving {
            self.save_dest = Some(dest.clone());
            self.edit.save_as(&self.worker, dest);
        }
    }

    pub fn take_opened(&mut self) -> bool {
        std::mem::take(&mut self.opened)
    }

    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    pub fn take_renamed(&mut self) -> Option<PathBuf> {
        self.renamed.take()
    }

    pub fn title(&self) -> String {
        app_state::tabs::tab_title(&self.path, 28)
    }
```

7. No `toolbar`, o botão "Claro/Escuro" sai daqui. O tema passa a ser da casca (Editar →
   Preferências, tarefa 9), então remova esse bloco.
8. `show` não desenha mais a mensagem "Arraste um PDF…". Quando `self.view.is_none()`, o canvas
   mostra `self.status` (por exemplo "Abrindo…"), como já faz.

- [ ] **Passo 7: Reescrever `app.rs`**

```rust
use crate::bench::Bench;
use crate::doc_tab::DocTab;
use crate::open_dialog::{FileDialogs, Picked};
use crate::shell::close::{self, CloseFlow, Step};
use crate::shell::tabs::{self as tab_bar, TabAction, TabLabel};
use app_state::prefs::Prefs;
use app_state::recents::{now_secs, RecentFiles};
use app_state::store;
use app_state::tabs::{canonical, tab_title, TabList, HOME, TOOLS};
use std::path::PathBuf;

const RECENTS_FILE: &str = "recentes.json";
const PREFS_FILE: &str = "preferencias.json";

pub struct GeraApp {
    tabs: TabList,
    /// Um por aba de documento, na mesma ordem de `tabs.docs()`.
    docs: Vec<DocTab>,
    dialogs: FileDialogs,
    recents: RecentFiles,
    prefs: Prefs,
    /// Mensagem da casca (falhas do seletor ou de abertura), mostrada no Início.
    status: String,
    closing: Option<CloseFlow>,
    /// Saída já confirmada: não interceptar o fechamento da janela de novo.
    allow_close: bool,
    bench: Option<Bench>,
}

impl GeraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>, bench: Option<Bench>) -> Self {
        let prefs = store::config_file(PREFS_FILE).map_or_else(Prefs::default, |p| Prefs::load(&p));
        cc.egui_ctx.set_visuals(if prefs.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let mut app = Self {
            tabs: TabList::default(),
            docs: Vec::new(),
            dialogs: FileDialogs::default(),
            recents: store::config_file(RECENTS_FILE)
                .map(|p| RecentFiles::load(&p))
                .unwrap_or_default(),
            prefs,
            status: String::new(),
            closing: None,
            allow_close: false,
            bench,
        };
        for path in paths {
            app.open_path(&cc.egui_ctx, path);
        }
        app
    }

    pub(crate) fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = canonical(&path);
        let before = self.tabs.active();
        let (tab, created) = self.tabs.open(path.clone());
        if created {
            let mut doc = DocTab::new(ctx, path.clone(), self.prefs.dark);
            doc.open(path);
            self.docs.push(doc);
        }
        self.tabs.activate(before); // `activate` abaixo suspende a aba que estava na frente
        self.activate(tab);
    }

    fn activate(&mut self, tab: usize) {
        let old = self.tabs.active();
        self.tabs.activate(tab);
        if old != self.tabs.active() {
            if let Some(doc) = self.doc_at(old) {
                doc.suspend();
            }
        }
    }

    fn doc_at(&mut self, tab: usize) -> Option<&mut DocTab> {
        let i = self.tabs.doc_index(tab)?;
        self.docs.get_mut(i)
    }

    fn active_doc(&mut self) -> Option<&mut DocTab> {
        self.doc_at(self.tabs.active())
    }

    fn close_tab(&mut self, tab: usize) {
        if let Some(i) = self.tabs.close(tab) {
            self.docs.remove(i); // o Worker cai junto e a thread termina
        }
    }

    /// Fecha as abas de `paths`, perguntando antes pelas que têm alterações pendentes.
    fn request_close(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>, quit: bool) {
        let mut dirty = Vec::new();
        for path in paths {
            let Some(tab) = self.tabs.find(&path) else {
                continue;
            };
            if self.doc_at(tab).is_some_and(|d| d.is_dirty() || d.edit.saving) {
                dirty.push(path);
            } else {
                self.close_tab(tab);
            }
        }
        if !dirty.is_empty() {
            self.closing = Some(CloseFlow::new(dirty, quit));
        } else if quit {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn remember(&mut self, path: PathBuf) {
        self.recents.add(path, now_secs());
        if let Some(file) = store::config_file(RECENTS_FILE) {
            if let Err(e) = self.recents.save(&file) {
                self.status = format!("Não foi possível gravar os recentes: {e}");
            }
        }
    }

    /// Respostas dos workers e do seletor de arquivos.
    fn poll(&mut self, ctx: &egui::Context) {
        let active = self.tabs.active();
        for tab in (app_state::tabs::FIXED..self.tabs.len()).rev() {
            let bench = if tab == active { self.bench.as_mut() } else { None };
            let Some(i) = self.tabs.doc_index(tab) else {
                continue;
            };
            let doc = &mut self.docs[i];
            doc.poll(ctx, bench);
            let (opened, failure, renamed) = (doc.take_opened(), doc.take_failure(), doc.take_renamed());
            let path = doc.path.clone();
            if opened {
                self.remember(path.clone());
            }
            if let Some(dest) = renamed {
                let dest = canonical(&dest);
                self.tabs.rename(tab, dest.clone());
                self.docs[i].path = dest.clone();
                self.remember(dest);
            }
            if let Some(msg) = failure {
                self.status = msg;
                self.close_tab(tab);
            }
        }
        if let Some(picked) = self.dialogs.poll() {
            match picked {
                Picked::Open(files) => {
                    for f in files {
                        self.open_path(ctx, f);
                    }
                }
                Picked::SaveAs { doc, dest } => self.save_as(doc, dest),
                Picked::Cancelled => {}
                Picked::Failed => self.status = "Não foi possível abrir o seletor de arquivos.".into(),
            }
        }
    }

    fn save_as(&mut self, doc: PathBuf, dest: PathBuf) {
        let dest_id = canonical(&dest);
        if self.tabs.find(&dest_id).is_some_and(|t| self.tabs.find(&doc) != Some(t)) {
            self.status = "Esse arquivo já está aberto em outra aba.".into();
            return;
        }
        if let Some(tab) = self.tabs.find(&doc) {
            if let Some(d) = self.doc_at(tab) {
                d.save_as(dest);
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.iter().map(|f| f.path().to_path_buf()) {
            self.open_path(ctx, file);
        }
    }

    fn shell_keys(&mut self, ctx: &egui::Context) {
        let (open, close_tab, next, prev) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::O),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::W),
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Tab),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Tab),
            )
        });
        if open {
            self.dialogs.open(ctx);
        }
        if close_tab {
            if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
                let path = self.tabs.docs()[i].clone();
                self.request_close(ctx, vec![path], false);
            }
        }
        if prev || next {
            let old = self.tabs.active();
            if prev {
                self.tabs.prev();
            } else {
                self.tabs.next();
            }
            let new = self.tabs.active();
            self.tabs.activate(old);
            self.activate(new);
        }
    }

    /// Avança o fechamento em andamento: pergunta, espera o salvamento ou fecha.
    fn drive_close(&mut self, ui: &mut egui::Ui) {
        let Some(flow) = self.closing.as_ref() else {
            return;
        };
        let Some(path) = flow.current().cloned() else {
            let quit = flow.quit();
            self.closing = None;
            if quit {
                self.allow_close = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        };
        let tab = self.tabs.find(&path);
        let step = if flow.is_saving() {
            match tab.and_then(|t| self.doc_at(t)) {
                Some(doc) if doc.edit.saving => return, // esperando o worker
                Some(doc) => {
                    let ok = !doc.is_dirty();
                    self.closing.as_mut().map(|f| f.saved(ok))
                }
                None => self.closing.as_mut().map(|f| f.saved(true)),
            }
        } else {
            if let Some(t) = tab {
                if self.tabs.active() != t {
                    self.activate(t); // mostra o documento sobre o qual se pergunta
                }
            }
            match close::ask(ui, &tab_title(&path, 60)) {
                Some(answer) => self.closing.as_mut().map(|f| f.answer(answer)),
                None => return,
            }
        };
        match step {
            Some(Step::Save(p)) => {
                if let Some(doc) = self.tabs.find(&p).and_then(|t| self.doc_at(t)) {
                    doc.save();
                }
            }
            Some(Step::Close(p)) => {
                if let Some(t) = self.tabs.find(&p) {
                    self.close_tab(t);
                }
            }
            Some(Step::Abort) | None => self.closing = None,
        }
    }

    fn tab_labels(&self) -> Vec<TabLabel> {
        self.docs
            .iter()
            .map(|d| TabLabel {
                title: d.title(),
                tooltip: d.path.display().to_string(),
                dirty: d.is_dirty(),
            })
            .collect()
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("Gera PDF Editor");
            ui.add_space(16.0);
            if ui.add(egui::Button::new("📂  Abrir arquivo…").min_size(egui::vec2(220.0, 40.0))).clicked() {
                self.dialogs.open(ui.ctx());
            }
            if !self.status.is_empty() {
                ui.add_space(12.0);
                ui.label(&self.status);
            }
        });
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
        let active = self.tabs.active();
        if let Some(i) = self.tabs.doc_index(active) {
            if let (Some(bench), Some(view)) = (self.bench.as_mut(), self.docs[i].view.as_mut()) {
                if let Some(y) = bench.scroll_step(ctx, view) {
                    self.docs[i].jump_to = Some(y);
                }
            }
        }
        // Fechar a janela com alterações pendentes (em qualquer aba) pergunta antes.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.docs.iter().any(|d| d.is_dirty() || d.edit.saving) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.closing.is_none() {
                    let all = self.tabs.docs().to_vec();
                    self.request_close(ctx, all, true);
                }
            }
        }
        if self.closing.is_none() {
            self.shell_keys(ctx);
            if let Some(doc) = self.active_doc() {
                doc.handle_input(ctx);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let labels = self.tab_labels();
        let action = egui::Panel::top("abas")
            .show(ui, |ui| tab_bar::show(ui, self.tabs.active(), &labels))
            .inner;
        match action {
            Some(TabAction::Activate(t)) => self.activate(t),
            Some(TabAction::Close(t)) => {
                if let Some(i) = self.tabs.doc_index(t) {
                    let path = self.tabs.docs()[i].clone();
                    self.request_close(ui.ctx(), vec![path], false);
                }
            }
            Some(TabAction::New) => self.dialogs.open(ui.ctx()),
            None => {}
        }
        match self.tabs.active() {
            HOME => {
                egui::CentralPanel::default().show(ui, |ui| self.home(ui));
            }
            TOOLS => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.heading("Ferramentas");
                });
            }
            _ => {
                if let Some(doc) = self.active_doc() {
                    doc.show(ui);
                }
            }
        }
        self.drive_close(ui);
    }
}
```

Em `main.rs`, troque o laço de argumentos e a criação do app:

```rust
    let mut paths = Vec::new();
    let (mut bench, mut scroll) = (false, false);
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--bench-open" => bench = true,
            "--bench-scroll" => (bench, scroll) = (true, true),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
```

E `app::GeraApp::new(cc, paths, bench.then(|| bench::Bench::new(started, scroll)))`.

- [ ] **Passo 8: Rodar a skill `simplify` sobre os arquivos da tarefa**

- [ ] **Passo 9: Rodar os testes e o roteiro manual**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: tudo passa, inclusive os 4 testes do `CloseFlow`.

Roteiro manual (`cargo run --release -q -p gera-ui`, sem argumentos, e depois com 2 PDFs):
1. Sem argumentos, abre no Início. "Abrir arquivo…", `Ctrl+O` e "＋" abrem o seletor do sistema,
   com escolha múltipla. Cada arquivo vira uma aba.
2. Abrir de novo um PDF já aberto (inclusive por um link simbólico) só ativa a aba dele.
3. `Ctrl+Tab` e `Ctrl+Shift+Tab` passam pelas abas, Início e Ferramentas incluídos.
4. Anotar na aba A, ir para a B e fechar a janela. Aparece "Salvar alterações em A?":
   - "Cancelar" mantém tudo aberto.
   - "Salvar" grava e fecha.
   - "Não salvar" fecha.
5. Arquivo inválido (um `.txt` renomeado para `.pdf`) não deixa aba e mostra o erro no Início.
6. Arrastar PDFs (com `env -u WAYLAND_DISPLAY`, em X11) abre abas.
7. **Portal ausente:** rodar com `env -u DBUS_SESSION_BUS_ADDRESS`. Anote se aparece a mensagem
   de falha ou se o seletor só "cancela". É a limitação conhecida da seção Review Focus.

- [ ] **Passo 10: Commit**

```bash
git add crates/gera-ui Cargo.lock
git commit -m "feat(gera-ui): abas de documentos, seletor de arquivos e fechamento com confirmação" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 9: Barra de menus, barra de ferramentas, atalhos, Salvar como, tela cheia e ajuda

**Arquivos:**
- Criar: `crates/gera-ui/src/shell/commands.rs`, `shell/menus.rs`, `shell/toolbar.rs`, `shell/help.rs`
- Modificar: `crates/gera-ui/src/shell/mod.rs`, `crates/gera-ui/src/app.rs`, `crates/gera-ui/src/doc_tab.rs`

**Interfaces:**
- Consome: `DocCommand`/`DocTab::run` (tarefa 5), `FileDialogs::save_as` (tarefa 8).
- Produz:
  - `pub enum Command { Open, OpenRecent(PathBuf), SaveAs, CloseTab, CloseAll, Quit, Doc(DocCommand), ToggleDark, FullScreen, NextTab, PrevTab, ActivateTab(usize), Shortcuts, About }`
    (com `Debug`, `Clone` e `PartialEq`), mais o método `label(&self) -> String`.
  - `shortcuts() -> Vec<(egui::KeyboardShortcut, Command)>`
  - `shortcut_of(cmd: &Command) -> Option<egui::KeyboardShortcut>`
  - `from_keyboard(ctx) -> Vec<Command>`
  - `menus::menu_bar(ui, state: &MenuState) -> Option<Command>`
  - `toolbar::show(ui, doc: &mut DocTab) -> Option<Command>`
  - `toolbar::comment_row(ui, doc: &mut DocTab)`
  - `help::shortcuts_window(ui, open: &mut bool)` e `help::about_window(ui, open: &mut bool)`
  - Em `DocTab`: `pub(crate) comment_tools: bool`, mais `page_controls(ui)`, `search_box(ui)` e
    `zoom_percent() -> Option<f32>`.

- [ ] **Passo 1: Escrever os testes que falham (tabela de atalhos)**

Crie `crates/gera-ui/src/shell/commands.rs` só com os testes:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shortcut_is_listed_twice() {
        let all = shortcuts();
        for (n, (a, _)) in all.iter().enumerate() {
            assert!(all[n + 1..].iter().all(|(b, _)| b != a), "{a:?} repetido");
        }
    }

    #[test]
    fn shift_variants_come_before_their_plain_twins() {
        // o egui aceita Shift a mais num atalho sem Shift: o com Shift precisa ser testado antes
        let all = shortcuts();
        for (i, (plain, _)) in all.iter().enumerate().filter(|(_, (s, _))| !s.modifiers.shift) {
            let shifted = all
                .iter()
                .position(|(s, _)| s.logical_key == plain.logical_key && s.modifiers.shift);
            if let Some(j) = shifted {
                assert!(j < i, "{plain:?} vem antes da versão com Shift");
            }
        }
    }

    #[test]
    fn main_commands_have_shortcuts_and_labels() {
        for cmd in [
            Command::Open,
            Command::SaveAs,
            Command::Doc(DocCommand::Save),
            Command::Doc(DocCommand::Undo),
            Command::Doc(DocCommand::Rotate { clockwise: true }),
            Command::FullScreen,
        ] {
            assert!(shortcut_of(&cmd).is_some(), "{cmd:?}");
            assert!(!cmd.label().is_empty());
        }
    }
}
```

Acrescente `pub mod commands; pub mod help; pub mod menus; pub mod toolbar;` a `shell/mod.rs`.

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p gera-ui commands -q 2>&1 | tail -5`
Esperado: erro de compilação.

- [ ] **Passo 3: Implementar `commands.rs` (acima dos testes)**

```rust
//! Comandos da casca e a tabela única de atalhos (teclado, menus e janela de ajuda).
use crate::doc_tab::DocCommand;
use app_state::layout::ViewMode;
use app_state::zoom::Fit;
use egui::{Key, KeyboardShortcut, Modifiers};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open,
    OpenRecent(PathBuf),
    SaveAs,
    CloseTab,
    CloseAll,
    Quit,
    Doc(DocCommand),
    ToggleDark,
    FullScreen,
    NextTab,
    PrevTab,
    ActivateTab(usize),
    Shortcuts,
    About,
}

impl Command {
    pub fn label(&self) -> String {
        let s = match self {
            Command::Open => "Abrir…",
            Command::OpenRecent(p) => return p.display().to_string(),
            Command::SaveAs => "Salvar como…",
            Command::CloseTab => "Fechar aba",
            Command::CloseAll => "Fechar todas",
            Command::Quit => "Sair",
            Command::ToggleDark => "Modo escuro",
            Command::FullScreen => "Tela cheia",
            Command::NextTab => "Próxima aba",
            Command::PrevTab => "Aba anterior",
            Command::ActivateTab(_) => "Ir para a aba",
            Command::Shortcuts => "Atalhos de teclado",
            Command::About => "Sobre o Gera PDF Editor",
            Command::Doc(d) => match d {
                DocCommand::Save => "Salvar",
                DocCommand::Undo => "Desfazer",
                DocCommand::Redo => "Refazer",
                DocCommand::Find => "Localizar",
                DocCommand::ZoomIn => "Ampliar",
                DocCommand::ZoomOut => "Reduzir",
                DocCommand::ZoomReset => "Tamanho real",
                DocCommand::Fit(Fit::Width) => "Ajustar à largura",
                DocCommand::Fit(Fit::Page) => "Ajustar à página",
                DocCommand::Fit(Fit::None) => "Zoom livre",
                DocCommand::Mode(m) => return m.label().into(),
                DocCommand::ToggleCover => "Mostrar capa sozinha",
                DocCommand::Rotate { clockwise: true } => "Girar no sentido horário",
                DocCommand::Rotate { clockwise: false } => "Girar no sentido anti-horário",
            },
        };
        s.into()
    }
}

/// Atalhos de teclado. Os com Shift vêm antes dos sem: o egui aceita Shift a mais num atalho
/// que não o pede, e testar `Ctrl+Z` antes consumiria `Ctrl+Shift+Z`.
pub fn shortcuts() -> Vec<(KeyboardShortcut, Command)> {
    let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
    let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
    let doc = Command::Doc;
    vec![
        (cmd_shift(Key::S), Command::SaveAs),
        (cmd_shift(Key::Z), doc(DocCommand::Redo)),
        (cmd_shift(Key::R), doc(DocCommand::Rotate { clockwise: false })),
        (cmd_shift(Key::Tab), Command::PrevTab),
        (cmd(Key::O), Command::Open),
        (cmd(Key::S), doc(DocCommand::Save)),
        (cmd(Key::W), Command::CloseTab),
        (cmd(Key::Q), Command::Quit),
        (cmd(Key::Z), doc(DocCommand::Undo)),
        (cmd(Key::F), doc(DocCommand::Find)),
        (cmd(Key::R), doc(DocCommand::Rotate { clockwise: true })),
        (cmd(Key::Tab), Command::NextTab),
        (cmd(Key::Plus), doc(DocCommand::ZoomIn)),
        (cmd(Key::Equals), doc(DocCommand::ZoomIn)),
        (cmd(Key::Minus), doc(DocCommand::ZoomOut)),
        (cmd(Key::Num0), doc(DocCommand::Fit(Fit::Page))),
        (cmd(Key::Num1), doc(DocCommand::ZoomReset)),
        (cmd(Key::Num2), doc(DocCommand::Fit(Fit::Width))),
        (KeyboardShortcut::new(Modifiers::NONE, Key::F11), Command::FullScreen),
    ]
}

/// Primeiro atalho do comando (para mostrar no menu).
pub fn shortcut_of(cmd: &Command) -> Option<KeyboardShortcut> {
    shortcuts().into_iter().find(|(_, c)| c == cmd).map(|(s, _)| s)
}

/// Enquanto o usuário digita, estes atalhos são do campo de texto.
fn belongs_to_text_fields(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Doc(DocCommand::Undo | DocCommand::Redo | DocCommand::ZoomIn | DocCommand::ZoomOut)
    )
}

/// Comandos pedidos pelo teclado neste quadro (as teclas são consumidas).
pub fn from_keyboard(ctx: &egui::Context) -> Vec<Command> {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|i| {
        shortcuts()
            .into_iter()
            .filter(|(_, c)| !(typing && belongs_to_text_fields(c)))
            .filter(|(s, _)| i.consume_shortcut(s))
            .map(|(_, c)| c)
            .collect()
    })
}

/// Modos de exibição na ordem do menu.
pub fn view_modes() -> [ViewMode; 4] {
    ViewMode::ALL
}
```

- [ ] **Passo 4: `DocTab` — controles reaproveitados pela barra da casca**

Em `doc_tab.rs`, remova o método `toolbar` e a linha `egui::Panel::top("toolbar")…` do `show`.
Divida o conteúdo do antigo `toolbar` em métodos públicos, que a barra da casca chama:

```rust
    /// ◀ campo da página / total ▶
    pub fn page_controls(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let count = view.layout.page_count().max(1);
        let mut page = self.last.current_page + 1;
        let prev = ui.button("◀").clicked();
        let go = ui.add(egui::DragValue::new(&mut page).range(1..=count).speed(0.2));
        ui.label(format!("/ {count}"));
        let next = ui.button("▶").clicked();
        let target = match (prev, next) {
            (true, _) => Some(page.saturating_sub(1).max(1)),
            (_, true) => Some((page + 1).min(count)),
            _ => go.changed().then_some(page),
        };
        if let Some(y) = target.and_then(|p| view.go_to(p as i32 - 1, None)) {
            self.jump_to = Some(y);
        }
    }

    /// Campo de busca, resumo e ▲▼ (o foco vem de `DocCommand::Find`).
    pub fn search_box(&mut self, ui: &mut egui::Ui) {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.search.text)
                .hint_text("Buscar…")
                .desired_width(160.0),
        );
        if std::mem::take(&mut self.focus_search) {
            edit.request_focus();
        }
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.search.start(&self.worker);
            edit.request_focus();
        }
        ui.label(self.search.summary());
        let prev = ui.button("▲").clicked();
        let next = ui.button("▼").clicked();
        if prev || next {
            let target = self.search.step(next);
            if let Some(y) = target.and_then(|(page, rect)| {
                self.view.as_mut()?.go_to(page, Some((rect.x0, rect.y0)))
            }) {
                self.jump_to = Some(y);
            }
        }
    }

    pub fn zoom_percent(&self) -> Option<f32> {
        self.view.as_ref().map(|v| v.zoom * 100.0)
    }
```

Acrescente o campo `pub(crate) comment_tools: bool` (inicia `false`). Em `handle_input`, remova
os blocos de desfazer, refazer e salvar: agora eles vêm da tabela de atalhos da casca. Ficam só a
navegação (PageUp, setas…) e o Ctrl+roda.

- [ ] **Passo 5: `toolbar.rs`**

```rust
//! Barra de ferramentas do documento e a linha de ferramentas de comentário.
use crate::doc_tab::{DocCommand, DocTab};
use crate::shell::commands::Command;
use app_state::tools::Tool;
use app_state::zoom::Fit;

pub fn show(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.button("📂").on_hover_text("Abrir (Ctrl+O)").clicked() {
            out = Some(Command::Open);
        }
        let save = if doc.is_dirty() { "💾•" } else { "💾" };
        if ui
            .add_enabled(!doc.edit.saving, egui::Button::new(save))
            .on_hover_text("Salvar (Ctrl+S)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Save));
        }
        ui.add_enabled(false, egui::Button::new("🖨"))
            .on_disabled_hover_text("Imprimir: em breve (v1.1-B)");
        if ui.button("🔍").on_hover_text("Localizar (Ctrl+F)").clicked() {
            out = Some(Command::Doc(DocCommand::Find));
        }
        ui.separator();
        doc.page_controls(ui);
        ui.separator();
        if ui.button("−").on_hover_text("Reduzir (Ctrl+-)").clicked() {
            out = Some(Command::Doc(DocCommand::ZoomOut));
        }
        if let Some(pct) = doc.zoom_percent() {
            ui.label(format!("{pct:.0}%"));
        }
        if ui.button("＋").on_hover_text("Ampliar (Ctrl++)").clicked() {
            out = Some(Command::Doc(DocCommand::ZoomIn));
        }
        ui.menu_button("▾", |ui| {
            for cmd in [
                DocCommand::Fit(Fit::Width),
                DocCommand::Fit(Fit::Page),
                DocCommand::ZoomReset,
            ] {
                let cmd = Command::Doc(cmd);
                if ui.button(cmd.label()).clicked() {
                    out = Some(cmd);
                }
            }
        });
        ui.separator();
        doc.search_box(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(false, egui::Button::new("✍ Assinar"))
                .on_disabled_hover_text("Em breve (v3)");
            if ui
                .add(egui::Button::selectable(doc.comment_tools, "✏ Comentar"))
                .clicked()
            {
                doc.comment_tools = !doc.comment_tools;
                if !doc.comment_tools {
                    doc.edit.tool = Tool::Select; // sem a linha, a ferramenta ativa ficaria escondida
                }
            }
        });
    });
    out
}

/// Ferramentas de anotação, desfazer e refazer (aparece com "Comentar" ligado).
pub fn comment_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        for tool in Tool::ALL {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        ui.separator();
        let can_edit = !doc.edit.saving;
        if ui
            .add_enabled(can_edit && doc.is_dirty(), egui::Button::new("↶"))
            .on_hover_text("Desfazer (Ctrl+Z)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Undo));
        }
        if ui
            .add_enabled(can_edit, egui::Button::new("↷"))
            .on_hover_text("Refazer (Ctrl+Shift+Z)")
            .clicked()
        {
            out = Some(Command::Doc(DocCommand::Redo));
        }
    });
    out
}
```

- [ ] **Passo 6: `menus.rs`**

```rust
//! Barra de menus no arranjo do Acrobat. Itens futuros aparecem desativados com a fase.
use crate::doc_tab::DocCommand;
use crate::shell::commands::{shortcut_of, view_modes, Command};
use app_state::layout::ViewMode;
use app_state::recents::RecentFiles;
use app_state::tabs::tab_title;
use app_state::zoom::Fit;

pub struct MenuState<'a> {
    /// Modo de exibição do documento ativo (`None` no Início e em Ferramentas).
    pub doc_mode: Option<ViewMode>,
    /// "Mostrar capa sozinha" ligado no documento ativo.
    pub doc_cover: bool,
    pub dark: bool,
    pub recents: &'a RecentFiles,
    /// (aba, título) de todas as abas, na ordem da barra.
    pub tabs: Vec<(usize, String)>,
    pub active: usize,
}

fn item(ui: &mut egui::Ui, out: &mut Option<Command>, cmd: Command, enabled: bool) {
    let mut button = egui::Button::new(cmd.label());
    if let Some(sc) = shortcut_of(&cmd) {
        button = button.shortcut_text(ui.ctx().format_shortcut(&sc));
    }
    if ui.add_enabled(enabled, button).clicked() {
        *out = Some(cmd);
    }
}

fn check(ui: &mut egui::Ui, out: &mut Option<Command>, cmd: Command, checked: bool, enabled: bool) {
    if ui
        .add_enabled(enabled, egui::Button::selectable(checked, cmd.label()))
        .clicked()
    {
        *out = Some(cmd);
    }
}

fn soon(ui: &mut egui::Ui, label: &str, phase: &str) {
    ui.add_enabled(false, egui::Button::new(format!("{label}  · em breve ({phase})")));
}

pub fn menu_bar(ui: &mut egui::Ui, s: &MenuState) -> Option<Command> {
    let mut out = None;
    let has_doc = s.doc_mode.is_some();
    let doc = |c| Command::Doc(c);
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Arquivo", |ui| {
            item(ui, &mut out, Command::Open, true);
            ui.menu_button("Abrir recente", |ui| {
                if s.recents.items().is_empty() {
                    ui.label("Nenhum arquivo recente");
                }
                for r in s.recents.items() {
                    if ui.button(tab_title(&r.path, 60)).on_hover_text(r.path.display().to_string()).clicked() {
                        out = Some(Command::OpenRecent(r.path.clone()));
                    }
                }
            });
            ui.separator();
            item(ui, &mut out, doc(DocCommand::Save), has_doc);
            item(ui, &mut out, Command::SaveAs, has_doc);
            soon(ui, "Exportar", "v5");
            ui.separator();
            soon(ui, "Propriedades", "v1.1-B");
            soon(ui, "Imprimir", "v1.1-B");
            ui.separator();
            item(ui, &mut out, Command::CloseTab, has_doc);
            item(ui, &mut out, Command::Quit, true);
        });
        ui.menu_button("Editar", |ui| {
            item(ui, &mut out, doc(DocCommand::Undo), has_doc);
            item(ui, &mut out, doc(DocCommand::Redo), has_doc);
            ui.separator();
            soon(ui, "Copiar", "v1.1-B");
            soon(ui, "Selecionar tudo", "v1.1-B");
            ui.separator();
            item(ui, &mut out, doc(DocCommand::Find), has_doc);
            ui.separator();
            ui.menu_button("Preferências", |ui| {
                check(ui, &mut out, Command::ToggleDark, s.dark, true);
            });
        });
        ui.menu_button("Exibir", |ui| {
            ui.menu_button("Zoom", |ui| {
                for c in [
                    DocCommand::ZoomIn,
                    DocCommand::ZoomOut,
                    DocCommand::ZoomReset,
                    DocCommand::Fit(Fit::Width),
                    DocCommand::Fit(Fit::Page),
                ] {
                    item(ui, &mut out, doc(c), has_doc);
                }
            });
            ui.menu_button("Exibição de página", |ui| {
                for m in view_modes() {
                    check(ui, &mut out, doc(DocCommand::Mode(m)), s.doc_mode == Some(m), has_doc);
                }
                ui.separator();
                let two_up = s.doc_mode.is_some_and(|m| m.columns() == 2);
                check(ui, &mut out, doc(DocCommand::ToggleCover), s.doc_cover, two_up);
            });
            ui.menu_button("Girar vista", |ui| {
                item(ui, &mut out, doc(DocCommand::Rotate { clockwise: true }), has_doc);
                item(ui, &mut out, doc(DocCommand::Rotate { clockwise: false }), has_doc);
            });
            item(ui, &mut out, Command::FullScreen, true);
            ui.separator();
            check(ui, &mut out, Command::ToggleDark, s.dark, true);
        });
        ui.menu_button("Assinar", |ui| {
            soon(ui, "Preencher e assinar", "v3");
            soon(ui, "Assinatura digital (certificado)", "v3");
            soon(ui, "Validar assinaturas", "v3");
        });
        ui.menu_button("Janela", |ui| {
            for (tab, title) in &s.tabs {
                if ui.add(egui::Button::selectable(*tab == s.active, title)).clicked() {
                    out = Some(Command::ActivateTab(*tab));
                }
            }
            ui.separator();
            item(ui, &mut out, Command::NextTab, true);
            item(ui, &mut out, Command::PrevTab, true);
            item(ui, &mut out, Command::CloseAll, s.tabs.len() > 2);
        });
        ui.menu_button("Ajuda", |ui| {
            item(ui, &mut out, Command::Shortcuts, true);
            item(ui, &mut out, Command::About, true);
        });
    });
    out
}
```

- [ ] **Passo 7: `help.rs`**

```rust
//! Janelas "Atalhos de teclado" e "Sobre".
use crate::shell::commands::shortcuts;

pub fn shortcuts_window(ui: &mut egui::Ui, open: &mut bool) {
    let ctx = ui.ctx().clone();
    egui::Window::new("Atalhos de teclado")
        .open(open)
        .collapsible(false)
        .show(ui, |ui| {
            egui::Grid::new("atalhos").striped(true).show(ui, |ui| {
                for (sc, cmd) in shortcuts() {
                    ui.label(cmd.label());
                    ui.monospace(ctx.format_shortcut(&sc));
                    ui.end_row();
                }
                ui.label("Página anterior / próxima");
                ui.monospace("PageUp / PageDown");
                ui.end_row();
                ui.label("Ampliar ou reduzir com a roda");
                ui.monospace("Ctrl + roda");
                ui.end_row();
            });
        });
}

pub fn about_window(ui: &mut egui::Ui, open: &mut bool) {
    egui::Window::new("Sobre o Gera PDF Editor")
        .open(open)
        .collapsible(false)
        .show(ui, |ui| {
            ui.heading(format!("Gera PDF Editor {}", env!("CARGO_PKG_VERSION")));
            ui.label("Editor de PDF leve e de código aberto.");
            ui.label("Licença: GNU AGPL-3.0.");
            ui.label("Renderização: MuPDF (Artifex Software, AGPL-3.0).");
            ui.label("Avisos de terceiros: THIRD_PARTY_NOTICES.md.");
        });
}
```

- [ ] **Passo 8: Ligar tudo no `app.rs`**

1. Campos novos em `GeraApp`: `fullscreen: bool`, `show_shortcuts: bool` e `show_about: bool`.
2. Método que executa comandos:

```rust
    fn run(&mut self, ctx: &egui::Context, cmd: Command) {
        match cmd {
            Command::Open => self.dialogs.open(ctx),
            Command::OpenRecent(path) => {
                if path.exists() {
                    self.open_path(ctx, path);
                } else {
                    self.status = format!("{} não existe mais.", path.display());
                }
            }
            Command::SaveAs => {
                if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
                    let path = self.tabs.docs()[i].clone();
                    self.dialogs.save_as(ctx, path);
                }
            }
            Command::CloseTab => {
                if let Some(i) = self.tabs.doc_index(self.tabs.active()) {
                    let path = self.tabs.docs()[i].clone();
                    self.request_close(ctx, vec![path], false);
                }
            }
            Command::CloseAll => {
                let all = self.tabs.docs().to_vec();
                self.request_close(ctx, all, false);
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Doc(c) => {
                if let Some(doc) = self.active_doc() {
                    doc.run(c);
                }
            }
            Command::ToggleDark => self.set_dark(ctx, !self.prefs.dark),
            Command::FullScreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Command::NextTab | Command::PrevTab => {
                let old = self.tabs.active();
                if cmd == Command::NextTab {
                    self.tabs.next();
                } else {
                    self.tabs.prev();
                }
                let new = self.tabs.active();
                self.tabs.activate(old);
                self.activate(new);
            }
            Command::ActivateTab(t) => self.activate(t),
            Command::Shortcuts => self.show_shortcuts = true,
            Command::About => self.show_about = true,
        }
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.prefs.dark = dark;
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        for doc in &mut self.docs {
            doc.set_dark(dark);
        }
        if let Some(file) = store::config_file(PREFS_FILE) {
            if let Err(e) = store::save_json(&file, &self.prefs) {
                self.status = format!("Não foi possível gravar as preferências: {e}");
            }
        }
    }
```

3. Remova `shell_keys`. No `logic`, troque a chamada dele por:

```rust
            for cmd in commands::from_keyboard(ctx) {
                self.run(ctx, cmd);
            }
            if self.fullscreen
                && !ctx.egui_wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.run(ctx, Command::FullScreen);
            }
```

4. No `ui`, antes da barra de abas, e só fora da tela cheia:

```rust
        if !self.fullscreen {
            let state = MenuState {
                doc_mode: self
                    .tabs
                    .doc_index(self.tabs.active())
                    .and_then(|i| self.docs[i].view.as_ref())
                    .map(|v| v.mode),
                doc_cover: self
                    .tabs
                    .doc_index(self.tabs.active())
                    .and_then(|i| self.docs[i].view.as_ref())
                    .is_some_and(|v| v.cover),
                dark: self.prefs.dark,
                recents: &self.recents,
                tabs: [(HOME, "Início".to_string()), (TOOLS, "Ferramentas".to_string())]
                    .into_iter()
                    .chain(self.docs.iter().enumerate().map(|(n, d)| (FIXED + n, d.title())))
                    .collect(),
                active: self.tabs.active(),
            };
            let picked = egui::Panel::top("menus")
                .show(ui, |ui| menus::menu_bar(ui, &state))
                .inner;
            if let Some(cmd) = picked {
                self.run(ui.ctx(), cmd);
            }
        }
```

   A barra de abas também só aparece fora da tela cheia. Com um documento ativo e fora da tela
   cheia, desenhe a barra de ferramentas e a linha Comentar antes de `doc.show(ui)`:

```rust
            _ if !self.fullscreen => {
                let mut cmd = None;
                if let Some(doc) = self.active_doc() {
                    cmd = egui::Panel::top("ferramentas-doc")
                        .show(ui, |ui| toolbar::show(ui, doc))
                        .inner;
                    if doc.comment_tools {
                        cmd = egui::Panel::top("comentar")
                            .show(ui, |ui| toolbar::comment_row(ui, doc))
                            .inner
                            .or(cmd);
                    }
                    doc.show(ui, true);
                }
                if let Some(cmd) = cmd {
                    self.run(ui.ctx(), cmd);
                }
            }
            _ => {
                if let Some(doc) = self.active_doc() {
                    doc.show(ui, false); // tela cheia: só as páginas
                }
            }
```

   Na tela cheia, `DocTab::show` também esconde a barra de status e o painel lateral. Acrescente
   o parâmetro `show(&mut self, ui: &mut egui::Ui, chrome: bool)` e envolva o `Panel::bottom` e o
   `self.sidebar(ui)` em `if chrome { … }`.
5. No fim do `ui`:
   `help::shortcuts_window(ui, &mut self.show_shortcuts); help::about_window(ui, &mut self.show_about);`.

- [ ] **Passo 9: Rodar a skill `simplify` sobre os arquivos da tarefa**

- [ ] **Passo 10: Rodar os testes e o roteiro manual**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: tudo passa.

Roteiro manual:
1. Cada menu abre. Os itens futuros aparecem acinzentados com a fase, e os atalhos aparecem à
   direita dos itens.
2. `Ctrl+Shift+S` abre "Salvar como". O arquivo novo tem as anotações, e a aba passa a mostrar o
   novo nome.
3. `Ctrl+R` e `Ctrl+Shift+R` giram, e `Ctrl+0`, `Ctrl+1` e `Ctrl+2` ajustam. `Ctrl+Z` dentro do
   campo de busca desfaz o texto, não a anotação.
4. `F11` entra em tela cheia, e `Esc` ou `F11` sai.
5. "✏ Comentar" mostra e esconde a linha de ferramentas.
6. Editar → Preferências → Modo escuro troca o tema de todas as abas e vale na próxima abertura.
7. Ajuda → Atalhos lista a tabela, e Sobre mostra versão e licenças.

- [ ] **Passo 11: Commit**

```bash
git add crates/gera-ui
git commit -m "feat(gera-ui): menus, barra de ferramentas, atalhos, salvar como, tela cheia e ajuda" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 10: Catálogo de ferramentas, aba Início com recentes e aba Ferramentas

**Arquivos:**
- Criar: `crates/app-state/src/tools_catalog.rs`, `crates/gera-ui/src/shell/home.rs`, `crates/gera-ui/src/shell/tools_hub.rs`
- Modificar: `crates/app-state/src/lib.rs`, `crates/gera-ui/src/shell/mod.rs`, `crates/gera-ui/src/app.rs`

**Interfaces:**
- Produz:
  - `pub enum Category { CreateEdit, Review, Protect, Organize }`, com `ALL` e `label()`.
  - `pub enum Status { Ready, Planned(&'static str) }`
  - `pub struct ToolEntry { pub id: &'static str, pub name: &'static str, pub icon: &'static str, pub category: Category, pub status: Status, pub description: &'static str }`
  - `pub const CATALOG: &[ToolEntry]`, `pub const COMMENT: &str = "comentar"` e
    `pub fn in_category(c: Category) -> impl Iterator<Item = &'static ToolEntry>`.
  - `home::show(ui, recents: &RecentFiles, status: &str) -> Option<HomeAction>`, com
    `pub enum HomeAction { OpenDialog, Open(PathBuf), Forget(PathBuf) }`.
  - `tools_hub::hub(ui) -> Option<&'static str>` e `tools_hub::side_list(ui) -> Option<&'static str>`.

- [ ] **Passo 1: Escrever os testes que falham**

Crie `crates/app-state/src/tools_catalog.rs` só com:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let ids: std::collections::HashSet<_> = CATALOG.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn every_category_has_tools_and_every_tool_is_described() {
        for c in Category::ALL {
            assert!(in_category(c).count() > 0, "{c:?}");
        }
        for t in CATALOG {
            assert!(!t.name.is_empty() && !t.description.is_empty() && !t.icon.is_empty(), "{}", t.id);
        }
    }

    #[test]
    fn only_comment_is_ready_in_this_phase() {
        let ready: Vec<_> = CATALOG.iter().filter(|t| t.status == Status::Ready).map(|t| t.id).collect();
        assert_eq!(ready, [COMMENT]);
        assert!(CATALOG
            .iter()
            .all(|t| t.status == Status::Ready || matches!(t.status, Status::Planned(p) if p.starts_with('v'))));
    }
}
```

Acrescente `pub mod tools_catalog;` a `lib.rs`.

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state tools_catalog -q 2>&1 | tail -5`
Esperado: erro de compilação.

- [ ] **Passo 3: Implementar o catálogo (acima dos testes)**

```rust
//! Catálogo de ferramentas: fonte única do hub Ferramentas, do painel direito e dos menus.
//! Cada fase seguinte só troca o `status` de um item para `Ready`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    CreateEdit,
    Review,
    Protect,
    Organize,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::CreateEdit, Category::Review, Category::Protect, Category::Organize];

    pub fn label(self) -> &'static str {
        match self {
            Category::CreateEdit => "Criar e editar",
            Category::Review => "Revisar e aprovar",
            Category::Protect => "Proteger e padronizar",
            Category::Organize => "Organizar",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ready,
    /// Chega na fase indicada (ex.: "v2").
    Planned(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub category: Category,
    pub status: Status,
    pub description: &'static str,
}

pub const COMMENT: &str = "comentar";

const fn tool(
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    category: Category,
    status: Status,
    description: &'static str,
) -> ToolEntry {
    ToolEntry { id, name, icon, category, status, description }
}

use Category::*;
use Status::*;

pub const CATALOG: &[ToolEntry] = &[
    tool("criar", "Criar PDF", "📄", CreateEdit, Planned("v5"), "PDF a partir de imagens e documentos"),
    tool("editar", "Editar PDF", "✏", CreateEdit, Planned("v4"), "Alterar texto e imagens"),
    tool("exportar", "Exportar PDF", "📤", CreateEdit, Planned("v5"), "Converter para Word, Excel ou imagens"),
    tool("ocr", "Digitalizar e OCR", "🔎", CreateEdit, Planned("v5"), "Reconhecer texto em páginas escaneadas"),
    tool("combinar", "Combinar arquivos", "🧩", CreateEdit, Planned("v2"), "Juntar vários PDFs em um"),
    tool(COMMENT, "Comentar", "💬", Review, Ready, "Destacar, sublinhar, notas e desenho"),
    tool("assinar", "Preencher e assinar", "✍", Review, Planned("v3"), "Preencher campos e assinar"),
    tool("certificados", "Certificados", "🔏", Review, Planned("v3"), "Assinatura digital com certificado"),
    tool("comparar", "Comparar arquivos", "⚖", Review, Planned("v5"), "Diferenças entre duas versões"),
    tool("proteger", "Proteger PDF", "🔒", Protect, Planned("v5"), "Senha e permissões"),
    tool("redigir", "Redigir", "⬛", Protect, Planned("v5"), "Apagar informações sensíveis de vez"),
    tool("comprimir", "Comprimir PDF", "🗜", Protect, Planned("v5"), "Reduzir o tamanho do arquivo"),
    tool("formulario", "Preparar formulário", "🗒", Protect, Planned("v3"), "Criar campos de formulário"),
    tool("organizar", "Organizar páginas", "🗂", Organize, Planned("v2"), "Reordenar, girar e excluir páginas"),
    tool("dividir", "Dividir PDF", "✂", Organize, Planned("v2"), "Separar em vários arquivos"),
    tool("inserir", "Inserir páginas", "➕", Organize, Planned("v2"), "Trazer páginas de outro arquivo"),
    tool("marca-dagua", "Marca d'água", "💧", Organize, Planned("v2"), "Texto ou imagem em todas as páginas"),
    tool("cabecalho", "Cabeçalho e rodapé", "📑", Organize, Planned("v2"), "Numeração e textos nas margens"),
    tool("recortar", "Recortar páginas", "⛶", Organize, Planned("v2"), "Ajustar as margens das páginas"),
];

pub fn in_category(c: Category) -> impl Iterator<Item = &'static ToolEntry> {
    CATALOG.iter().filter(move |t| t.category == c)
}
```

(Se o clippy reclamar do `use` depois dos itens, mova os dois `use` para o topo do arquivo.)

- [ ] **Passo 4: `home.rs` e `tools_hub.rs`**

`crates/gera-ui/src/shell/home.rs`:

```rust
//! Aba Início: abrir arquivo e lista de recentes.
use app_state::recents::{ago, now_secs, RecentFiles};
use app_state::tabs::tab_title;
use std::path::PathBuf;

pub enum HomeAction {
    OpenDialog,
    Open(PathBuf),
    Forget(PathBuf),
}

pub fn show(ui: &mut egui::Ui, recents: &RecentFiles, status: &str) -> Option<HomeAction> {
    let mut action = None;
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.heading("Gera PDF Editor");
        ui.add_space(16.0);
        let open = egui::Button::new("📂  Abrir arquivo…").min_size(egui::vec2(220.0, 40.0));
        if ui.add(open).clicked() {
            action = Some(HomeAction::OpenDialog);
        }
        if !status.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().warn_fg_color, status);
        }
    });
    ui.add_space(24.0);
    ui.heading("Recentes");
    ui.separator();
    if recents.items().is_empty() {
        ui.label("Os arquivos que você abrir aparecem aqui.");
        return action;
    }
    let now = now_secs();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("recentes").striped(true).num_columns(4).show(ui, |ui| {
            for r in recents.items() {
                let exists = r.path.exists();
                let mut name = egui::RichText::new(format!("📄 {}", tab_title(&r.path, 60)));
                if !exists {
                    name = name.strikethrough().weak();
                }
                let folder = r.path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                if ui
                    .add_enabled(exists, egui::Button::new(name).frame(false))
                    .on_hover_text(r.path.display().to_string())
                    .on_disabled_hover_text("O arquivo não existe mais.")
                    .clicked()
                {
                    action = Some(HomeAction::Open(r.path.clone()));
                }
                ui.weak(folder);
                ui.weak(ago(now, r.opened_at));
                if !exists && ui.small_button("Remover da lista").clicked() {
                    action = Some(HomeAction::Forget(r.path.clone()));
                }
                ui.end_row();
            }
        });
    });
    action
}
```

`crates/gera-ui/src/shell/tools_hub.rs`:

```rust
//! Aba Ferramentas (grade de cartões) e a lista resumida do painel direito.
use app_state::tools_catalog::{in_category, Category, Status, ToolEntry};

fn badge(t: &ToolEntry) -> String {
    match t.status {
        Status::Ready => String::new(),
        Status::Planned(phase) => format!("Em breve · {phase}"),
    }
}

/// Cartão de uma ferramenta; devolve `true` se clicado (só as prontas são clicáveis).
fn card(ui: &mut egui::Ui, t: &ToolEntry) -> bool {
    let ready = t.status == Status::Ready;
    let frame = egui::Frame::group(ui.style()).inner_margin(10.0);
    let response = frame
        .show(ui, |ui| {
            ui.set_width(180.0);
            ui.set_height(86.0);
            ui.add_enabled_ui(ready, |ui| {
                ui.label(egui::RichText::new(format!("{} {}", t.icon, t.name)).strong());
                ui.small(t.description);
                if !ready {
                    ui.small(egui::RichText::new(badge(t)).italics());
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    ready && response.clicked()
}

pub fn hub(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for c in Category::ALL {
            ui.add_space(12.0);
            ui.heading(c.label());
            ui.horizontal_wrapped(|ui| {
                for t in in_category(c) {
                    if card(ui, t) {
                        clicked = Some(t.id);
                    }
                }
            });
        }
    });
    clicked
}

/// Lista compacta para o painel direito.
pub fn side_list(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut clicked = None;
    ui.heading("Ferramentas");
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for c in Category::ALL {
            ui.add_space(6.0);
            ui.small(c.label());
            for t in in_category(c) {
                let ready = t.status == Status::Ready;
                let button = egui::Button::new(format!("{} {}", t.icon, t.name)).frame(false);
                if ui
                    .add_enabled(ready, button)
                    .on_disabled_hover_text(badge(t))
                    .clicked()
                {
                    clicked = Some(t.id);
                }
            }
        }
    });
    clicked
}
```

Acrescente `pub mod home; pub mod tools_hub;` a `shell/mod.rs`.

- [ ] **Passo 5: Ligar no `app.rs`**

1. Remova o método `home` da tarefa 8.
2. Campos novos: `/// Ligar a linha Comentar no próximo documento aberto pelo seletor. comment_after_open: bool`
   e `/// Último documento que esteve na frente (para o hub voltar a ele). last_doc: Option<PathBuf>`.
3. No `ui`, guarde `last_doc` quando a aba ativa for de documento
   (`self.last_doc = self.tabs.doc_index(active).map(|i| self.tabs.docs()[i].clone());`) e troque
   os braços `HOME` e `TOOLS`:

```rust
            HOME => {
                let action = egui::CentralPanel::default()
                    .show(ui, |ui| home::show(ui, &self.recents, &self.status))
                    .inner;
                match action {
                    Some(HomeAction::OpenDialog) => self.dialogs.open(ui.ctx()),
                    Some(HomeAction::Open(p)) => self.open_path(ui.ctx(), p),
                    Some(HomeAction::Forget(p)) => {
                        self.recents.remove(&p);
                        if let Some(file) = store::config_file(RECENTS_FILE) {
                            let _ = self.recents.save(&file);
                        }
                    }
                    None => {}
                }
            }
            TOOLS => {
                let picked = egui::CentralPanel::default().show(ui, tools_hub::hub).inner;
                if let Some(id) = picked {
                    self.run_tool(ui.ctx(), id);
                }
            }
```

4. Método:

```rust
    /// Cartão ou item de ferramenta clicado. Só "Comentar" existe nesta fase.
    fn run_tool(&mut self, ctx: &egui::Context, id: &str) {
        if id != tools_catalog::COMMENT {
            return;
        }
        match self.last_doc.clone().and_then(|p| self.tabs.find(&p)) {
            Some(tab) => {
                self.activate(tab);
                if let Some(doc) = self.active_doc() {
                    doc.comment_tools = true;
                }
            }
            None => {
                self.comment_after_open = true;
                self.dialogs.open(ctx);
            }
        }
    }
```

5. Em `poll`, no braço `Picked::Open(files)`, depois de abrir os arquivos:
   `if std::mem::take(&mut self.comment_after_open) { if let Some(doc) = self.active_doc() { doc.comment_tools = true; } }`.
   No braço `Picked::Cancelled | Picked::Failed`, faça `self.comment_after_open = false;`.

- [ ] **Passo 6: Rodar a skill `simplify` sobre os arquivos da tarefa**

- [ ] **Passo 7: Rodar os testes e o roteiro manual**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: tudo passa.

Roteiro manual:
1. O Início lista os recentes com pasta e "há N min". Clicar abre o arquivo, ou ativa a aba dele
   se já estiver aberto. Renomeie um PDF recente no gerenciador de arquivos: o item aparece
   riscado, e "Remover da lista" o tira.
2. Ferramentas mostra 4 categorias, com os cartões futuros acinzentados e "Em breve · vN".
3. Clicar em "Comentar" com um documento aberto vai para a aba dele com a linha de comentário
   ligada. Sem documento, abre o seletor e liga a linha no PDF aberto.

- [ ] **Passo 8: Commit**

```bash
git add crates/app-state crates/gera-ui
git commit -m "feat: catálogo de ferramentas, aba Início com recentes e aba Ferramentas" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 11: Trilho esquerdo, painéis redimensionáveis e preferências persistidas

**Arquivos:**
- Criar: `crates/gera-ui/src/shell/panels.rs`
- Modificar: `crates/gera-ui/src/sidebar.rs`, `crates/gera-ui/src/doc_tab.rs`, `crates/gera-ui/src/app.rs`, `crates/gera-ui/src/shell/menus.rs`, `crates/gera-ui/src/shell/commands.rs`, `crates/gera-ui/src/shell/mod.rs`

**Interfaces:**
- Consome: `Prefs`/`LeftPanel`/`MIN_PANEL`/`MAX_PANEL` (tarefa 7) e `tools_hub::side_list` (tarefa 10).
- Produz:
  - `panels::rail(ui, open: Option<LeftPanel>) -> Option<LeftPanel>`, que devolve o ícone
    clicado.
  - `DocTab::side_panel(ui, panel: LeftPanel)`
  - `sidebar::show(ui, panel: LeftPanel, view, worker, outline, saved, pending) -> SidebarAction`,
    que não tem mais a fileira de abas interna.
  - `Command::TogglePanel(LeftPanel)` e `Command::ToggleTools`
  - `MenuState`: campos novos `left: Option<LeftPanel>` e `tools_panel: bool`.

- [ ] **Passo 1: Escrever o teste que falha**

Acrescente aos testes de `commands.rs`:

```rust
    #[test]
    fn panel_commands_have_labels() {
        for p in app_state::prefs::LeftPanel::ALL {
            assert!(!Command::TogglePanel(p).label().is_empty());
        }
        assert!(!Command::ToggleTools.label().is_empty());
    }
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p gera-ui commands -q 2>&1 | tail -5`
Esperado: erro de compilação (variantes inexistentes).

- [ ] **Passo 3: Implementar**

`commands.rs`:
- Acrescente `TogglePanel(app_state::prefs::LeftPanel)` e `ToggleTools` ao enum `Command`.
- Em `label`, acrescente os braços
  `Command::TogglePanel(p) => return p.label().into(),` e `Command::ToggleTools => "Ferramentas",`.

`sidebar.rs`:
- Remova o enum `SidebarTab` e use `app_state::prefs::LeftPanel`.
- `show` perde o parâmetro `tab: &mut SidebarTab`, ganha `panel: LeftPanel` e não desenha mais a
  fileira de botões:

```rust
pub fn show(
    ui: &mut egui::Ui,
    panel: LeftPanel,
    view: &mut DocView,
    worker: &Worker,
    outline: &[OutlineItem],
    saved: &[AnnotInfo],
    pending: &[NewAnnot],
) -> SidebarAction {
    ui.strong(panel.label());
    ui.separator();
    match panel {
        LeftPanel::Thumbnails => thumbnails(ui, view, worker),
        LeftPanel::Outline => outline_list(ui, outline),
        LeftPanel::Annotations => annotation_list(ui, saved, pending),
    }
}
```

`doc_tab.rs`:
- Remova o campo `tab: SidebarTab` e o método `sidebar`. No `show`, remova `self.sidebar(ui)`.
- Acrescente:

```rust
    /// Conteúdo do painel esquerdo; clicar leva à página.
    pub fn side_panel(&mut self, ui: &mut egui::Ui, panel: LeftPanel) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let action = sidebar::show(
            ui,
            panel,
            view,
            &self.worker,
            &self.outline,
            &self.saved_annots,
            self.edit.pending.items(),
        );
        if let SidebarAction::JumpToPage(page) = action {
            if let Some(y) = view.go_to(page as i32, None) {
                self.jump_to = Some(y);
            }
        }
    }
```

   **Ordem dos painéis no egui:** os laterais precisam ser adicionados **antes** do
   `CentralPanel`. Separe `show` em dois: `show_status(ui, chrome)`, com a barra de status de
   baixo, e `show_canvas(ui)`, com o `begin_frame`, o `CentralPanel`, as janelas de nota e senha
   e o `end_frame`. A casca chama na ordem: barras de cima, `show_status`, painéis laterais e
   `show_canvas`. Como o `begin_frame` precisa vir antes das miniaturas (que pedem renders),
   mova-o para um método `begin(&mut self)`, chamado pela casca antes dos painéis.

`shell/panels.rs`:

```rust
//! Trilho de ícones à esquerda, que abre e fecha o painel lateral.
use app_state::prefs::LeftPanel;

pub fn rail(ui: &mut egui::Ui, open: Option<LeftPanel>) -> Option<LeftPanel> {
    let mut clicked = None;
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        for p in LeftPanel::ALL {
            let button = egui::Button::selectable(open == Some(p), egui::RichText::new(p.icon()).size(18.0));
            if ui.add(button).on_hover_text(p.label()).clicked() {
                clicked = Some(p);
            }
            ui.add_space(4.0);
        }
    });
    clicked
}
```

Acrescente `pub mod panels;` a `shell/mod.rs`.

`menus.rs`:
- Acrescente a `MenuState` os campos `pub left: Option<LeftPanel>` e `pub tools_panel: bool`.
- Em "Exibir", antes do separador do modo escuro, acrescente:

```rust
            ui.menu_button("Painéis", |ui| {
                for p in LeftPanel::ALL {
                    check(ui, &mut out, Command::TogglePanel(p), s.left == Some(p), true);
                }
                check(ui, &mut out, Command::ToggleTools, s.tools_panel, true);
            });
```

`app.rs`:
- Campo novo: `/// Preferências mudaram e ainda não foram gravadas. prefs_dirty: bool`.
- `run`, braços novos:

```rust
            Command::TogglePanel(p) => {
                self.prefs.left_panel = if self.prefs.left_panel == Some(p) { None } else { Some(p) };
                self.prefs_dirty = true;
            }
            Command::ToggleTools => {
                self.prefs.tools_panel = !self.prefs.tools_panel;
                self.prefs_dirty = true;
            }
```

- `set_dark` passa a só marcar `self.prefs_dirty = true;`, no lugar de gravar direto.
- No `logic`, ao fim:

```rust
        // grava as preferências quando o usuário solta o mouse (não a cada pixel de arrasto)
        if self.prefs_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.prefs_dirty = false;
            if let Some(file) = store::config_file(PREFS_FILE) {
                if let Err(e) = store::save_json(&file, &self.prefs) {
                    self.status = format!("Não foi possível gravar as preferências: {e}");
                }
            }
        }
```

- No braço da aba de documento fora da tela cheia, a ordem fica:

```rust
            _ if !self.fullscreen => {
                let mut cmd = None;
                let (mut left, mut tools) = (self.prefs.left_panel, self.prefs.tools_panel);
                let (mut left_w, mut tools_w) = (self.prefs.left_width, self.prefs.tools_width);
                let mut tool_id = None;
                if let Some(doc) = self.active_doc() {
                    doc.begin();
                    cmd = egui::Panel::top("ferramentas-doc").show(ui, |ui| toolbar::show(ui, doc)).inner;
                    if doc.comment_tools {
                        cmd = egui::Panel::top("comentar").show(ui, |ui| toolbar::comment_row(ui, doc)).inner.or(cmd);
                    }
                    doc.show_status(ui, true);
                    let clicked = egui::Panel::left("trilho")
                        .exact_size(40.0)
                        .resizable(false)
                        .show(ui, |ui| panels::rail(ui, left))
                        .inner;
                    if let Some(p) = clicked {
                        left = if left == Some(p) { None } else { Some(p) };
                    }
                    if let Some(p) = left {
                        let r = egui::Panel::left("painel-esquerdo")
                            .resizable(true)
                            .default_size(left_w)
                            .size_range(MIN_PANEL..=MAX_PANEL)
                            .show(ui, |ui| doc.side_panel(ui, p));
                        left_w = r.response.rect.width();
                    }
                    if tools {
                        let r = egui::Panel::right("painel-ferramentas")
                            .resizable(true)
                            .default_size(tools_w)
                            .size_range(MIN_PANEL..=MAX_PANEL)
                            .show(ui, tools_hub::side_list);
                        tools_w = r.response.rect.width();
                        tool_id = r.inner;
                    }
                    doc.show_canvas(ui);
                }
                if (left, tools) != (self.prefs.left_panel, self.prefs.tools_panel)
                    || (left_w - self.prefs.left_width).abs() > 0.5
                    || (tools_w - self.prefs.tools_width).abs() > 0.5
                {
                    self.prefs.left_panel = left;
                    self.prefs.tools_panel = tools;
                    self.prefs.left_width = left_w;
                    self.prefs.tools_width = tools_w;
                    self.prefs_dirty = true;
                }
                if let Some(id) = tool_id {
                    self.run_tool(ui.ctx(), id);
                }
                if let Some(cmd) = cmd {
                    self.run(ui.ctx(), cmd);
                }
                let _ = &mut tools; // só leitura aqui; o menu alterna via comando
            }
```

  (Remova a última linha se o compilador não reclamar de `mut` sem uso. Ela só existe para deixar
  claro que `tools` não muda neste bloco. Se reclamar de `unused_mut`, declare `tools` sem `mut`.)

- Na tela cheia: `doc.begin(); doc.show_status(ui, false); doc.show_canvas(ui);`.
- `MenuState` recebe `left: self.prefs.left_panel` e `tools_panel: self.prefs.tools_panel`.

- [ ] **Passo 4: Rodar a skill `simplify` sobre os arquivos da tarefa**

- [ ] **Passo 5: Rodar os testes e o roteiro manual**

Run: `cargo clippy --workspace --all-targets -q -- -D warnings && cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"`
Esperado: tudo passa.

Roteiro manual:
1. O trilho mostra ▤ ☰ 💬. Clicar abre o painel correspondente, e clicar de novo fecha.
2. Arrastar a borda muda a largura, dentro de 120 a 600.
3. O painel direito lista as ferramentas, e só "Comentar" é clicável.
4. Exibir → Painéis alterna os mesmos painéis.
5. Feche e reabra o app: painéis, larguras e tema voltam como estavam. Confira
   `~/.config/gera-pdf-editor/preferencias.json`.
6. Corrompa esse arquivo (escreva `{{{` nele) e abra o app: abre com o padrão, sem erro.

- [ ] **Passo 6: Commit**

```bash
git add crates/gera-ui
git commit -m "feat(gera-ui): trilho esquerdo, painéis redimensionáveis e preferências persistidas" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 12: Medição das metas com 3 abas e verificação final

**Arquivos:**
- Modificar: `scripts/measure.sh`

- [ ] **Passo 1: Medir memória com 3 abas**

Em `scripts/measure.sh`:

1. Troque o bloco da amostra e do `trap` por:

```bash
TABS_DIR=$(mktemp -d)
cleanup() { rm -rf "$TABS_DIR"; [ -n "${TMP_SAMPLE:-}" ] && rm -f "$TMP_SAMPLE"; }
trap cleanup EXIT

if [ -n "${1:-}" ]; then
  SAMPLE=$1; LABEL="PDF real"
else
  TMP_SAMPLE=$(mktemp --suffix=.pdf); SAMPLE=$TMP_SAMPLE; LABEL="PDF sintético (texto simples: melhor caso)"
  cargo run --release -q -p pdf-core --example make_sample -- "$SAMPLE" 100
fi
```

2. Depois do bloco `SCROLL=…`:

```bash
# três abas abertas ao mesmo tempo (cópias: caminhos diferentes viram abas diferentes)
for n in 1 2 3; do cp "$SAMPLE" "$TABS_DIR/aba$n.pdf"; done
TABS=$(timeout 60 "$BIN" --bench-open "$TABS_DIR/aba1.pdf" "$TABS_DIR/aba2.pdf" "$TABS_DIR/aba3.pdf" | grep '^BENCH') \
  || { echo "FALHA: o app não respondeu em 60 s com 3 abas" >&2; exit 1; }
RSS_TABS=$(field rss_kb "$TABS")
```

3. Junto aos `echo` e `check` finais:

```bash
echo "rss_kb_3_abas=$RSS_TABS (limite $MAX_RSS_KB)"
check "memória com 3 abas (kB)" "$RSS_TABS" "$MAX_RSS_KB"
```

- [ ] **Passo 2: Rodar a medição**

Run: `scripts/measure.sh`
Esperado: termina com código 0, e todas as métricas (binário, abertura, memória, memória após
rolar, memória com 3 abas) ficam dentro dos limites. Se alguma falhar, **pare** e investigue com
a skill `superpowers:systematic-debugging` antes de mexer em limites.

- [ ] **Passo 3: Verificação final**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -q -- -D warnings
cargo test --workspace -q 2>&1 | grep -E "test result|FAILED"
cargo build --release -q -p gera-ui
cargo audit 2>/dev/null || echo "cargo-audit não instalado: N/A"
git status --short
```

Esperado:
- fmt e clippy limpos, todos os testes passando e build release OK.
- Nenhuma vulnerabilidade crítica ou alta no `cargo audit` (se houver, avalie se afeta o projeto
  antes de atualizar).
- `git status` só com o que vai no commit, e nenhum segredo.

Use a skill `superpowers:verification-before-completion` antes de declarar a v1.1-A concluída.

- [ ] **Passo 4: Commit**

```bash
git add scripts/measure.sh
git commit -m "build: mede memória com 3 abas abertas" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
