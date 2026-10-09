# Gera PDF Editor v1 (visualizador leve com anotações) — Plano de Implementação

> **Para agentes:** SUB-SKILL OBRIGATÓRIA: use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para executar este plano tarefa a tarefa. Os passos usam checkbox (`- [ ]`).

**Objetivo:** app desktop Linux em Rust que abre PDFs com rapidez e pouco uso de memória, e permite buscar texto, destacar, sublinhar, anotar e desenhar, salvando as anotações dentro do PDF.

**Arquitetura:** workspace Cargo com 3 crates. `pdf-core` é a única que conhece o MuPDF e roda todo o trabalho do documento em **uma thread dedicada** (o `Document` do MuPDF não é `Send`), falando por canais. `app-state` tem a lógica pura e testável (zoom, layout, cache LRU, anotações pendentes). `gera-ui` é o egui/eframe (backend glow) que só desenha e traduz entrada em pedidos.

**Stack:** Rust stable, `mupdf` 0.8 (estático, features enxutas), `eframe`/`egui` 0.36 com `glow`, `thiserror` 2, `tempfile` (testes).

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1-viewer-design.md`

## Restrições globais

(Valores copiados do spec; valem para todas as tarefas.)

- Plataforma: somente Linux; código portável. Licença do projeto: **AGPL-3.0**.
- Metas: abertura (cold start até a 1ª página visível) **< 300 ms**; memória com PDF típico de 100 páginas **< 150 MB**, sem crescer sem limite ao rolar; binário release **< 25 MB**, sem webview/runtime externo; rolagem sem travar a interface.
- Motor: crate `mupdf` 0.8 com features desligadas (JavaScript, EPUB/HTML, OCR/Tesseract etc.). UI: `eframe` com backend `glow`, nunca `wgpu`.
- Nenhum tipo do MuPDF vaza de `pdf-core`. A UI nunca chama o MuPDF.
- Salvar com segurança: gravar em arquivo temporário na mesma pasta e renomear; o original só é substituído se tudo der certo.
- PDF é entrada não confiável: JavaScript desligado na compilação, nada embutido é executado, limite de tamanho do que é renderizado.
- Perfil release: `lto`, `codegen-units = 1`, `strip`, `panic = "abort"`.
- Nenhum código ou binário da Adobe no repositório. Não rodar `git push`.

**Regras do usuário (CLAUDE.md global), válidas em toda tarefa de código:**
1. Antes de rodar os testes de uma tarefa, invoque a skill `simplify` sobre os arquivos criados/alterados e aplique o que ela apontar (registre o que pular).
2. Rode apenas o mínimo que valide a alteração (testes do crate tocado + `cargo clippy` do crate); a suíte completa e a medição só na Tarefa 15.
3. Todo commit termina com o trailer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` (use um segundo `-m`).

**Aviso sobre o código deste plano:** foi escrito lendo o código-fonte do `mupdf` 0.8.0 e a documentação do eframe 0.36, mas **não foi compilado** (não há Rust instalado ainda). Se uma assinatura divergir, corrija no passo de compilação mantendo as interfaces "Produz" de cada tarefa. A API de `egui` 0.36 mudou em relação às versões antigas (`App::ui(&mut Ui, ..)`, `Panel::top(..).show(ui, ..)`); confira com `npx ctx7@latest docs /emilk/egui "<pergunta>"` antes de improvisar.

## Foco de revisão

Entradas/condições que o spec não cita, mas que mais provavelmente quebram o app. Cada uma tem teste na tarefa dona:

1. **Arquivo vazio, truncado ou lixo** deve virar erro claro, nunca *panic* (Tarefa 6).
2. **PDF com senha:** pedir senha, rejeitar senha errada, e **continuar protegido depois de salvar** (Tarefa 6).
3. **Caminho com acento, espaço e parênteses** (`Relatório final (1).pdf`) abre e salva (Tarefa 6).
4. **Falha ao salvar** (pasta inexistente/sem permissão) deixa o original intacto e o documento utilizável (Tarefa 5).
5. **Entradas degeneradas:** anotação sem área, traço vazio, zoom `NaN`/0/negativo, documento de 0 páginas, página girada (Tarefas 3, 5, 8).

## Estrutura de arquivos

```
gera-pdf-editor/
├── Cargo.toml                      # workspace + perfil release
├── LICENSE                         # AGPL-3.0
├── THIRD_PARTY_NOTICES.md
├── .gitignore
├── .github/workflows/ci.yml
├── scripts/measure.sh              # mede binário, abertura e memória
├── packaging/{build-appimage.sh, gera-pdf-editor.desktop, gera-pdf-editor.svg}
└── crates/
    ├── pdf-core/
    │   ├── Cargo.toml
    │   ├── examples/make_sample.rs
    │   ├── src/{lib.rs, error.rs, types.rs, sample.rs, doc.rs, worker.rs}
    │   └── tests/{common/mod.rs, open.rs, render.rs, search.rs, annotations.rs, robustness.rs, worker.rs}
    ├── app-state/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, zoom.rs, layout.rs, cache.rs, annots.rs, tools.rs}
    └── gera-ui/
        ├── Cargo.toml
        └── src/{main.rs, app.rs, view.rs, canvas.rs, edit.rs, search.rs, sidebar.rs}
```

Responsabilidades: `doc.rs` = operações síncronas sobre o PDF; `worker.rs` = thread + protocolo de mensagens; `view.rs` = textura/cache/pedidos de render de um documento; `canvas.rs` = rolagem virtualizada e desenho das páginas; `edit.rs` = ferramentas e anotações pendentes na UI; `search.rs` = estado da busca; `sidebar.rs` = miniaturas, índice e lista de anotações.

---

### Tarefa 1: Toolchain e esqueleto do workspace

**Arquivos:**
- Criar: `Cargo.toml`, `.gitignore`, `LICENSE`, `.github/workflows/ci.yml`
- Criar: `crates/pdf-core/{Cargo.toml,src/lib.rs}`, `crates/app-state/{Cargo.toml,src/lib.rs}`, `crates/gera-ui/{Cargo.toml,src/main.rs}`

**Interfaces:**
- Produz: workspace que compila; `cargo test --workspace` roda 0 testes sem erro; crates `pdf-core`, `app-state`, `gera-ui` (binário `gera-pdf-editor`).

- [ ] **Passo 1: Instalar o toolchain Rust (sem root) e as dependências de build do MuPDF**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
rustup component add clippy rustfmt
dnf list --available clang cmake gcc-c++ make pkgconf-pkg-config fontconfig-devel freetype-devel 2>&1 | tail -n 10
pkexec dnf install -y clang cmake gcc-c++ make pkgconf-pkg-config fontconfig-devel freetype-devel
cargo --version && clang --version | head -n 1
```
Esperado: versões do `cargo` e do `clang` impressas. (Pacotes já instalados são ignorados pelo dnf.)

- [ ] **Passo 2: Criar `Cargo.toml` do workspace**

```toml
[workspace]
resolver = "2"
members = ["crates/pdf-core", "crates/app-state", "crates/gera-ui"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "AGPL-3.0-only"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

- [ ] **Passo 3: Criar os três crates mínimos**

`crates/pdf-core/Cargo.toml`:
```toml
[package]
name = "pdf-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
mupdf = { version = "0.8", default-features = false, features = ["base14-fonts", "system-fonts"] }
thiserror = "2"

[dev-dependencies]
tempfile = "3"
```
`crates/pdf-core/src/lib.rs`:
```rust
//! Única camada que conhece o MuPDF.
```
`crates/app-state/Cargo.toml`:
```toml
[package]
name = "app-state"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
pdf-core = { path = "../pdf-core" }
```
`crates/app-state/src/lib.rs`:
```rust
//! Estado e lógica pura do app (sem UI e sem MuPDF direto).
```
`crates/gera-ui/Cargo.toml`:
```toml
[package]
name = "gera-ui"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "gera-pdf-editor"
path = "src/main.rs"

[dependencies]
pdf-core = { path = "../pdf-core" }
app-state = { path = "../app-state" }
egui = "0.36"
eframe = { version = "0.36", default-features = false, features = ["glow", "default_fonts", "x11", "wayland"] }
```
`crates/gera-ui/src/main.rs`:
```rust
fn main() {}
```

- [ ] **Passo 4: `.gitignore`, licença e CI**

`.gitignore`:
```
/target
*.pdf
!crates/pdf-core/tests/fixtures/*.pdf
.env
```
```bash
curl -sSfL https://www.gnu.org/licenses/agpl-3.0.txt -o LICENSE && head -n 3 LICENSE
```
`.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - run: sudo apt-get update && sudo apt-get install -y clang libclang-dev cmake pkg-config libfontconfig1-dev libfreetype6-dev
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - run: cargo build --release -p gera-ui
```

- [ ] **Passo 5: Compilar (esta é a primeira compilação do MuPDF; demora alguns minutos)**

Run: `cargo build --workspace 2>&1 | tail -n 15`
Esperado: `Finished`. Se o `mupdf-sys` falhar por falta de `libclang`, instale `clang-devel` com `pkexec dnf install -y clang-devel` e repita. Se a feature `system-fonts` falhar por `fontconfig`, confirme o passo 1.

- [ ] **Passo 6: Commit**

```bash
git add -A && git commit -m "chore: esqueleto do workspace, licença AGPL e CI" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 2: `pdf-core` — tipos, erros, PDF de exemplo e abrir documento

**Arquivos:**
- Criar: `crates/pdf-core/src/{error.rs,types.rs,sample.rs,doc.rs}`
- Modificar: `crates/pdf-core/src/lib.rs`
- Criar: `crates/pdf-core/tests/common/mod.rs`, `crates/pdf-core/tests/open.rs`

**Interfaces:**
- Produz (usado por todas as tarefas seguintes):
```rust
// error.rs
pub enum PdfError { Open(String), PasswordRequired, WrongPassword, PageOutOfRange(i32),
                    InvalidScale(f32), InvalidAnnotation(String), Engine(String), Io(std::io::Error) }
pub type Result<T> = std::result::Result<T, PdfError>;
// types.rs
pub struct PdfRect { pub x0: f32, pub y0: f32, pub x1: f32, pub y1: f32 }   // coordenadas da página em pontos, origem no canto superior esquerdo, já considerando rotação
pub struct PageSize { pub width: f32, pub height: f32 }
pub struct RenderedPage { pub width: u32, pub height: u32, pub rgb: Vec<u8> }
pub struct Word { pub text: String, pub rect: PdfRect }
pub enum AnnotKind { Highlight, Underline, Note, Ink }
pub struct AnnotInfo { pub kind: AnnotKind, pub page: i32, pub rect: PdfRect, pub contents: String }
pub enum NewAnnot { Highlight { page: i32, rects: Vec<PdfRect> }, Underline { page: i32, rects: Vec<PdfRect> },
                    Note { page: i32, at: (f32, f32), text: String }, Ink { page: i32, strokes: Vec<Vec<(f32, f32)>> } }
pub struct OutlineItem { pub title: String, pub page: Option<i32>, pub depth: usize }
pub struct DocSummary { pub path: PathBuf, pub sizes: Vec<PageSize>, pub outline: Vec<OutlineItem>, pub annots: Vec<AnnotInfo> }
// sample.rs
pub fn sample_pdf(pages: usize, rotate: i32) -> Vec<u8>   // PDF mínimo: "Page N Hello Gera" + índice "Capitulo N"
// doc.rs
pub struct PdfDoc { /* privado */ }
impl PdfDoc {
    pub fn open(path: &Path, password: Option<&str>) -> Result<PdfDoc>;
    pub fn page_count(&self) -> Result<i32>;
    pub fn page_size(&self, page: i32) -> Result<PageSize>;
}
```

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/common/mod.rs`:
```rust
use pdf_core::sample::sample_pdf;
use std::path::PathBuf;
use tempfile::TempDir;

pub fn write_sample(dir: &TempDir, name: &str, pages: usize, rotate: i32) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, sample_pdf(pages, rotate)).unwrap();
    path
}
```
`crates/pdf-core/tests/open.rs`:
```rust
mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfError};

#[test]
fn opens_and_reports_pages_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 3, 0);
    let doc = PdfDoc::open(&path, None).unwrap();
    assert_eq!(doc.page_count().unwrap(), 3);
    let size = doc.page_size(0).unwrap();
    assert_eq!((size.width, size.height), (612.0, 792.0));
}

#[test]
fn rotated_page_swaps_width_and_height() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 90);
    let size = PdfDoc::open(&path, None).unwrap().page_size(0).unwrap();
    assert_eq!((size.width, size.height), (792.0, 612.0));
}

#[test]
fn out_of_range_page_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    assert!(matches!(doc.page_size(2), Err(PdfError::PageOutOfRange(2))));
    assert!(matches!(doc.page_size(-1), Err(PdfError::PageOutOfRange(-1))));
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test open 2>&1 | tail -n 8`
Esperado: FALHA de compilação (`pdf_core::sample`/`PdfDoc` não existem).

- [ ] **Passo 3: Implementar**

`crates/pdf-core/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("não foi possível abrir o PDF: {0}")]
    Open(String),
    #[error("o PDF está protegido por senha")]
    PasswordRequired,
    #[error("senha incorreta")]
    WrongPassword,
    #[error("a página {0} não existe")]
    PageOutOfRange(i32),
    #[error("escala de renderização inválida: {0}")]
    InvalidScale(f32),
    #[error("anotação inválida: {0}")]
    InvalidAnnotation(String),
    #[error("erro do motor PDF: {0}")]
    Engine(String),
    #[error("erro de arquivo: {0}")]
    Io(#[from] std::io::Error),
}

impl From<mupdf::Error> for PdfError {
    fn from(e: mupdf::Error) -> Self {
        PdfError::Engine(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PdfError>;
```
`crates/pdf-core/src/types.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl PdfRect {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// Retângulo com os cantos em qualquer ordem, reordenados para x0<=x1 e y0<=y1.
    pub fn from_corners(a: (f32, f32), b: (f32, f32)) -> Self {
        Self::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }

    pub fn intersects(&self, o: &PdfRect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

impl From<mupdf::Rect> for PdfRect {
    fn from(r: mupdf::Rect) -> Self {
        Self::new(r.x0, r.y0, r.x1, r.y1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// 3 bytes por pixel (RGB), linhas contíguas.
    pub rgb: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub rect: PdfRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotKind {
    Highlight,
    Underline,
    Note,
    Ink,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotInfo {
    pub kind: AnnotKind,
    pub page: i32,
    pub rect: PdfRect,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewAnnot {
    Highlight { page: i32, rects: Vec<PdfRect> },
    Underline { page: i32, rects: Vec<PdfRect> },
    Note { page: i32, at: (f32, f32), text: String },
    Ink { page: i32, strokes: Vec<Vec<(f32, f32)>> },
}

impl NewAnnot {
    pub fn page(&self) -> i32 {
        match self {
            Self::Highlight { page, .. }
            | Self::Underline { page, .. }
            | Self::Note { page, .. }
            | Self::Ink { page, .. } => *page,
        }
    }

    /// Anotação sem conteúdo desenhável (não deve ser guardada).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Highlight { rects, .. } | Self::Underline { rects, .. } => rects.is_empty(),
            Self::Note { text, .. } => text.trim().is_empty(),
            Self::Ink { strokes, .. } => strokes.iter().all(|s| s.len() < 2),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<i32>,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct DocSummary {
    pub path: PathBuf,
    pub sizes: Vec<PageSize>,
    pub outline: Vec<OutlineItem>,
    pub annots: Vec<AnnotInfo>,
}
```
`crates/pdf-core/src/sample.rs`:
```rust
//! Gera PDFs mínimos para testes, exemplos e medições.

/// `rotate` é o valor de /Rotate (0, 90, 180, 270). Cada página tem o texto
/// "Page N Hello Gera" e há um índice com um item "Capitulo N" por página.
pub fn sample_pdf(pages: usize, rotate: i32) -> Vec<u8> {
    let outlines_obj = 4 + 2 * pages;
    let mut objs: Vec<String> = Vec::new();
    let outlines_ref = if pages > 0 { format!(" /Outlines {outlines_obj} 0 R") } else { String::new() };
    objs.push(format!("<< /Type /Catalog /Pages 2 0 R{outlines_ref} >>"));
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    for i in 0..pages {
        let content = 5 + 2 * i;
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {rotate} /Contents {content} 0 R \
             /Resources << /Font << /F1 3 0 R >> >> >>"
        ));
        let stream = format!("BT /F1 24 Tf 72 700 Td (Page {} Hello Gera) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
    }
    if pages > 0 {
        let first = outlines_obj + 1;
        let last = outlines_obj + pages;
        objs.push(format!("<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {pages} >>"));
        for i in 0..pages {
            let me = first + i;
            let prev = if i > 0 { format!(" /Prev {} 0 R", me - 1) } else { String::new() };
            let next = if i + 1 < pages { format!(" /Next {} 0 R", me + 1) } else { String::new() };
            objs.push(format!(
                "<< /Title (Capitulo {}) /Parent {outlines_obj} 0 R /Dest [{} 0 R /Fit]{prev}{next} >>",
                i + 1,
                4 + 2 * i
            ));
        }
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in &offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes(),
    );
    out
}
```
`crates/pdf-core/src/doc.rs`:
```rust
use crate::error::{PdfError, Result};
use crate::types::PageSize;
use mupdf::pdf::PdfDocument;
use std::path::{Path, PathBuf};

pub struct PdfDoc {
    pub(crate) inner: PdfDocument,
    pub(crate) path: PathBuf,
    /// Guardada só em memória, para reabrir o arquivo (ex.: descartar mudanças após falha ao salvar).
    pub(crate) password: Option<String>,
}

/// Abre o arquivo e, se for protegido, autentica com `password`.
pub(crate) fn open_authenticated(path: &Path, password: Option<&str>) -> Result<PdfDocument> {
    let mut inner = PdfDocument::open(path).map_err(|e| PdfError::Open(e.to_string()))?;
    if inner.needs_password()? {
        match password {
            None => return Err(PdfError::PasswordRequired),
            Some(p) if !inner.authenticate(p)? => return Err(PdfError::WrongPassword),
            Some(_) => {}
        }
    }
    Ok(inner)
}

impl PdfDoc {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let inner = open_authenticated(path, password)?;
        Ok(Self { inner, path: path.to_path_buf(), password: password.map(str::to_owned) })
    }

    pub fn page_count(&self) -> Result<i32> {
        Ok(self.inner.page_count()?)
    }

    pub(crate) fn load(&self, page: i32) -> Result<mupdf::Page> {
        if page < 0 || page >= self.page_count()? {
            return Err(PdfError::PageOutOfRange(page));
        }
        Ok(self.inner.load_page(page)?)
    }

    pub fn page_size(&self, page: i32) -> Result<PageSize> {
        let b = self.load(page)?.bounds()?;
        Ok(PageSize { width: b.x1 - b.x0, height: b.y1 - b.y0 })
    }
}
```
`crates/pdf-core/src/lib.rs`:
```rust
//! Única camada que conhece o MuPDF.
pub mod sample;

mod doc;
mod error;
mod types;

pub use doc::PdfDoc;
pub use error::{PdfError, Result};
pub use types::*;
```

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core --test open 2>&1 | tail -n 8`
Esperado: `3 passed`. Se `rotated_page_swaps...` falhar com 612x792, o `bounds()` não aplica `/Rotate`: nesse caso troque width/height em `page_size` quando a página tiver rotação 90/270 (leia `/Rotate` via `self.inner.load_pdf_page(page)?.object()`), mantendo o teste como está.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): tipos, erros, PDF de exemplo e abertura de documento" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 3: `pdf-core` — renderização

**Arquivos:**
- Modificar: `crates/pdf-core/src/doc.rs`
- Criar: `crates/pdf-core/tests/render.rs`

**Interfaces:**
- Consome: `PdfDoc::load`, `page_size`, `PdfError::InvalidScale`.
- Produz: `PdfDoc::render(&self, page: i32, scale: f32, invert: bool) -> Result<RenderedPage>` (RGB, branco opaco como fundo; `invert` inverte as cores para o modo escuro; escala inválida ou imagem acima de 40 milhões de pixels devolve `InvalidScale`).

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/render.rs`:
```rust
mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfError};

fn doc(rotate: i32) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, rotate), None).unwrap();
    (dir, doc)
}

#[test]
fn renders_at_scale_with_white_background_and_ink() {
    let (_d, doc) = doc(0);
    let img = doc.render(0, 1.0, false).unwrap();
    assert_eq!((img.width, img.height), (612, 792));
    assert_eq!(img.rgb.len(), 612 * 792 * 3);
    assert_eq!(&img.rgb[0..3], &[255, 255, 255]);
    assert!(img.rgb.iter().any(|&b| b != 255), "o texto deveria aparecer");
    let big = doc.render(0, 2.0, false).unwrap();
    assert_eq!((big.width, big.height), (1224, 1584));
}

#[test]
fn invert_turns_background_black() {
    let (_d, doc) = doc(0);
    let img = doc.render(0, 0.5, true).unwrap();
    assert_eq!(&img.rgb[0..3], &[0, 0, 0]);
}

#[test]
fn rotated_page_renders_rotated() {
    let (_d, doc) = doc(90);
    let img = doc.render(0, 1.0, false).unwrap();
    assert_eq!((img.width, img.height), (792, 612));
}

#[test]
fn invalid_or_huge_scale_is_rejected() {
    let (_d, doc) = doc(0);
    for s in [0.0, -1.0, f32::NAN, f32::INFINITY, 1000.0] {
        assert!(matches!(doc.render(0, s, false), Err(PdfError::InvalidScale(_))), "escala {s}");
    }
    assert!(matches!(doc.render(9, 1.0, false), Err(PdfError::PageOutOfRange(9))));
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test render 2>&1 | tail -n 6`
Esperado: FALHA de compilação (`render` não existe).

- [ ] **Passo 3: Implementar em `doc.rs`**

Acrescente os imports `use crate::types::RenderedPage;` e `use mupdf::{Colorspace, Matrix};`, a constante e o método:
```rust
/// Limite de pixels por imagem renderizada (evita estourar a memória).
const MAX_PIXELS: f64 = 40_000_000.0;

impl PdfDoc {
    pub fn render(&self, page: i32, scale: f32, invert: bool) -> Result<RenderedPage> {
        let size = self.page_size(page)?;
        let pixels = f64::from(size.width * scale) * f64::from(size.height * scale);
        if !scale.is_finite() || scale <= 0.0 || pixels > MAX_PIXELS {
            return Err(PdfError::InvalidScale(scale));
        }
        let ctm = Matrix::new_scale(scale, scale);
        let mut pix = self.load(page)?.to_pixmap(&ctm, &Colorspace::device_rgb(), false, true)?;
        if invert {
            pix.invert()?;
        }
        let (w, h) = (pix.width() as usize, pix.height() as usize);
        let (n, stride) = (pix.n() as usize, pix.stride() as usize);
        let samples = pix.samples();
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in 0..h {
            let line = &samples[row * stride..row * stride + w * n];
            for px in line.chunks_exact(n) {
                rgb.extend_from_slice(&px[..3]);
            }
        }
        Ok(RenderedPage { width: w as u32, height: h as u32, rgb })
    }
}
```

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core --test render 2>&1 | tail -n 6`
Esperado: `4 passed`. Se `invert_turns_background_black` falhar, confirme que `Pixmap::invert` existe em `src/pixmap.rs` (existe na 0.8.0); se o fundo vier com canal alfa (`n == 4`), o código já ignora o 4º byte.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): renderização de páginas em RGB com limite de tamanho" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 4: `pdf-core` — busca e palavras em uma área

**Arquivos:**
- Modificar: `crates/pdf-core/src/doc.rs`
- Criar: `crates/pdf-core/tests/search.rs`

**Interfaces:**
- Produz:
```rust
PdfDoc::search(&self, page: i32, needle: &str) -> Result<Vec<PdfRect>>      // needle vazio ou com NUL => Ok(vec![])
PdfDoc::words_in_rect(&self, page: i32, area: PdfRect) -> Result<Vec<Word>> // palavras cujo retângulo intersecta a área
```

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/search.rs`:
```rust
mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    (dir, doc)
}

#[test]
fn finds_text_with_a_sensible_rectangle() {
    let (_d, doc) = doc();
    let hits = doc.search(0, "Hello").unwrap();
    assert_eq!(hits.len(), 1);
    let r = hits[0];
    assert!((r.x0 - 72.0 - 70.0).abs() < 40.0, "x0 = {}", r.x0); // depois de "Page 1 "
    assert!(r.y0 > 50.0 && r.y1 < 110.0, "y = {}..{}", r.y0, r.y1); // topo da página (origem no canto superior)
    assert!(r.x1 > r.x0 && r.y1 > r.y0);
}

#[test]
fn degenerate_needles_return_nothing_without_panicking() {
    let (_d, doc) = doc();
    assert!(doc.search(0, "").unwrap().is_empty());
    assert!(doc.search(0, "a\0b").unwrap().is_empty());
    assert!(doc.search(0, "inexistente").unwrap().is_empty());
    assert!(doc.search(0, &"x".repeat(100_000)).unwrap().is_empty());
}

#[test]
fn words_in_rect_selects_by_intersection() {
    let (_d, doc) = doc();
    let all = doc.words_in_rect(0, PdfRect::new(0.0, 0.0, 612.0, 792.0)).unwrap();
    let texts: Vec<_> = all.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["Page", "1", "Hello", "Gera"]);
    let none = doc.words_in_rect(0, PdfRect::new(400.0, 600.0, 600.0, 780.0)).unwrap();
    assert!(none.is_empty());
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test search 2>&1 | tail -n 6`
Esperado: FALHA de compilação.

- [ ] **Passo 3: Implementar em `doc.rs`**

Imports: `use crate::types::{PdfRect, Word};` e `use mupdf::{Quad, TextPageFlags};`.
```rust
fn quad_rect(q: &Quad) -> PdfRect {
    let xs = [q.ul.x, q.ur.x, q.ll.x, q.lr.x];
    let ys = [q.ul.y, q.ur.y, q.ll.y, q.lr.y];
    let min = |v: [f32; 4]| v.into_iter().fold(f32::INFINITY, f32::min);
    let max = |v: [f32; 4]| v.into_iter().fold(f32::NEG_INFINITY, f32::max);
    PdfRect::new(min(xs), min(ys), max(xs), max(ys))
}

impl PdfDoc {
    pub fn search(&self, page: i32, needle: &str) -> Result<Vec<PdfRect>> {
        let page = self.load(page)?;
        if needle.is_empty() || needle.contains('\0') || needle.len() > 4096 {
            return Ok(Vec::new());
        }
        Ok(page.search(needle, 512)?.iter().map(quad_rect).collect())
    }

    pub fn words_in_rect(&self, page: i32, area: PdfRect) -> Result<Vec<Word>> {
        let text = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        Ok(text
            .words()
            .into_iter()
            .map(|w| Word { text: w.text, rect: w.bounds.into() })
            .filter(|w| w.rect.intersects(&area))
            .collect())
    }
}
```
(`needle.len() > 4096` garante o teste da agulha gigante sem custo; o limite é uma proteção, não uma regra de negócio.)

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core --test search 2>&1 | tail -n 6`
Esperado: `3 passed`. Se a ordem das palavras for diferente, ajuste o `assert_eq!` à ordem real do MuPDF sem relaxar a verificação (o conjunto precisa ser exatamente esse).

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): busca de texto e palavras em uma área" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 5: `pdf-core` — anotações e salvamento seguro (Spike 1)

**Arquivos:**
- Modificar: `crates/pdf-core/src/doc.rs`
- Criar: `crates/pdf-core/tests/annotations.rs`

**Interfaces:**
- Consome: `NewAnnot`, `AnnotInfo`, `AnnotKind`, `PdfRect`, `PdfError::InvalidAnnotation`.
- Produz:
```rust
PdfDoc::annotations(&self, page: i32) -> Result<Vec<AnnotInfo>>          // só destaque, sublinhado, nota e tinta
PdfDoc::all_annotations(&self) -> Result<Vec<AnnotInfo>>
PdfDoc::save_with(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()>
// save_with: valida tudo, aplica, grava em temporário na pasta de `dest` e renomeia.
// Em qualquer erro, descarta as mudanças em memória (recarrega do disco) e devolve o erro; `dest` nunca fica pela metade.
```

**Esta tarefa é o Spike 1 do spec.** Se um tipo de anotação não funcionar no `mupdf` 0.8, registre no commit qual foi e remova esse tipo da v1 (variante de `NewAnnot`, ferramenta da UI) em vez de gambiarra.

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/annotations.rs`:
```rust
mod common;
use common::write_sample;
use pdf_core::{AnnotKind, NewAnnot, PdfDoc, PdfError, PdfRect};

fn setup() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let doc = PdfDoc::open(&path, None).unwrap();
    (dir, path, doc)
}

fn all_kinds(hit: PdfRect) -> Vec<NewAnnot> {
    vec![
        NewAnnot::Highlight { page: 0, rects: vec![hit] },
        NewAnnot::Underline { page: 0, rects: vec![hit] },
        NewAnnot::Note { page: 0, at: (300.0, 300.0), text: "olá nota".into() },
        NewAnnot::Ink { page: 0, strokes: vec![vec![(100.0, 400.0), (150.0, 420.0), (200.0, 400.0)]] },
    ]
}

#[test]
fn all_four_kinds_survive_save_and_reopen() {
    let (_d, path, mut doc) = setup();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&all_kinds(hit), &path).unwrap();
    let reopened = PdfDoc::open(&path, None).unwrap();
    let kinds: Vec<_> = reopened.annotations(0).unwrap().iter().map(|a| a.kind).collect();
    for k in [AnnotKind::Highlight, AnnotKind::Underline, AnnotKind::Note, AnnotKind::Ink] {
        assert!(kinds.contains(&k), "faltou {k:?}; veio {kinds:?}");
    }
    let note = reopened.annotations(0).unwrap().into_iter().find(|a| a.kind == AnnotKind::Note).unwrap();
    assert_eq!(note.contents, "olá nota");
    assert_eq!(reopened.all_annotations().unwrap().len(), 4);
}

#[test]
fn highlight_is_visible_when_rendered_after_reopen() {
    let (_d, path, mut doc) = setup();
    let before = doc.render(0, 1.0, false).unwrap();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&[NewAnnot::Highlight { page: 0, rects: vec![hit] }], &path).unwrap();
    let after = PdfDoc::open(&path, None).unwrap().render(0, 1.0, false).unwrap();
    assert_ne!(before.rgb, after.rgb);
}

#[test]
fn degenerate_annotations_are_errors_not_panics() {
    let (_d, path, mut doc) = setup();
    let bad = [
        NewAnnot::Highlight { page: 0, rects: vec![] },
        NewAnnot::Highlight { page: 0, rects: vec![PdfRect::new(10.0, 10.0, 10.0, 10.0)] },
        NewAnnot::Ink { page: 0, strokes: vec![] },
        NewAnnot::Ink { page: 0, strokes: vec![vec![]] },
        NewAnnot::Note { page: 7, at: (0.0, 0.0), text: "x".into() },
    ];
    for a in bad {
        assert!(doc.save_with(std::slice::from_ref(&a), &path).is_err(), "{a:?}");
    }
    assert!(matches!(
        doc.save_with(&[NewAnnot::Note { page: 7, at: (0.0, 0.0), text: "x".into() }], &path),
        Err(PdfError::PageOutOfRange(7))
    ));
    assert!(doc.annotations(0).unwrap().is_empty(), "nada deve ter ficado aplicado");
}

#[test]
fn failed_save_keeps_original_and_document_usable() {
    let (dir, path, mut doc) = setup();
    let original = std::fs::read(&path).unwrap();
    let dest = dir.path().join("nao_existe").join("x.pdf");
    let hit = doc.search(0, "Hello").unwrap()[0];
    let res = doc.save_with(&[NewAnnot::Highlight { page: 0, rects: vec![hit] }], &dest);
    assert!(res.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original, "original intacto");
    assert!(!dest.exists());
    assert!(doc.annotations(0).unwrap().is_empty(), "mudanças em memória descartadas");
    assert_eq!(doc.render(0, 1.0, false).unwrap().width, 612);
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test annotations 2>&1 | tail -n 6`
Esperado: FALHA de compilação.

- [ ] **Passo 3: Implementar em `doc.rs`**

Imports: `use crate::types::{AnnotInfo, AnnotKind, NewAnnot};`, `use mupdf::pdf::{Encryption, PdfAnnotationType, PdfPage, PdfWriteOptions};`, `use mupdf::{Point, Quad, Rect};`.
```rust
fn quads(rects: &[PdfRect]) -> Result<Vec<Quad>> {
    if rects.is_empty() {
        return Err(PdfError::InvalidAnnotation("sem área para marcar".into()));
    }
    Ok(rects
        .iter()
        .map(|r| {
            Quad::new(
                Point::new(r.x0, r.y0),
                Point::new(r.x1, r.y0),
                Point::new(r.x0, r.y1),
                Point::new(r.x1, r.y1),
            )
        })
        .collect())
}

fn add_one(page: &mut PdfPage, a: &NewAnnot) -> Result<()> {
    match a {
        NewAnnot::Highlight { rects, .. } => drop(page.add_highlight_annotation(quads(rects)?)?),
        NewAnnot::Underline { rects, .. } => drop(page.add_underline_annotation(quads(rects)?)?),
        NewAnnot::Note { at, text, .. } => {
            drop(page.add_text_annotation(Rect::new(at.0, at.1, at.0 + 24.0, at.1 + 24.0), text)?)
        }
        NewAnnot::Ink { strokes, .. } => drop(
            page.add_ink_annotation(strokes.iter().map(|s| s.iter().map(|&(x, y)| Point::new(x, y))))?,
        ),
    }
    Ok(())
}

fn kind_of(t: PdfAnnotationType) -> Option<AnnotKind> {
    match t {
        PdfAnnotationType::Highlight => Some(AnnotKind::Highlight),
        PdfAnnotationType::Underline => Some(AnnotKind::Underline),
        PdfAnnotationType::Text => Some(AnnotKind::Note),
        PdfAnnotationType::Ink => Some(AnnotKind::Ink),
        _ => None,
    }
}

fn temp_path(dest: &Path) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .ok_or_else(|| PdfError::Engine("caminho de destino sem nome de arquivo".into()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".gera-tmp");
    Ok(dest.with_file_name(tmp_name))
}

impl PdfDoc {
    pub fn annotations(&self, page: i32) -> Result<Vec<AnnotInfo>> {
        self.load(page)?;
        let page_ref = self.inner.load_pdf_page(page)?;
        let mut out = Vec::new();
        for annot in page_ref.annotations() {
            if let Some(kind) = kind_of(annot.r#type()?) {
                out.push(AnnotInfo {
                    kind,
                    page,
                    rect: annot.bounds()?.into(),
                    contents: annot.contents()?.unwrap_or_default().to_owned(),
                });
            }
        }
        Ok(out)
    }

    pub fn all_annotations(&self) -> Result<Vec<AnnotInfo>> {
        let mut all = Vec::new();
        for p in 0..self.page_count()? {
            all.extend(self.annotations(p)?);
        }
        Ok(all)
    }

    pub fn save_with(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()> {
        let result = self.apply_and_write(pending, dest);
        if result.is_err() {
            self.reload()?;
        }
        result
    }

    /// Descarta as mudanças em memória reabrindo o arquivo (com a mesma senha, se houver).
    fn reload(&mut self) -> Result<()> {
        self.inner = open_authenticated(&self.path, self.password.as_deref())?;
        Ok(())
    }

    fn apply_and_write(&mut self, pending: &[NewAnnot], dest: &Path) -> Result<()> {
        let count = self.page_count()?;
        if let Some(bad) = pending.iter().map(NewAnnot::page).find(|p| *p < 0 || *p >= count) {
            return Err(PdfError::PageOutOfRange(bad));
        }
        let mut pages: Vec<i32> = pending.iter().map(NewAnnot::page).collect();
        pages.sort_unstable();
        pages.dedup();
        for p in pages {
            let mut page = self.inner.load_pdf_page(p)?;
            for a in pending.iter().filter(|a| a.page() == p) {
                add_one(&mut page, a)?;
            }
            page.update()?;
        }
        let tmp = temp_path(dest)?;
        let tmp_str = tmp.to_str().ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))?;
        let mut opts = PdfWriteOptions::default();
        opts.set_encryption(Encryption::Keep);
        if let Err(e) = self.inner.save_with_options(tmp_str, opts) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        if let Err(e) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        self.path = dest.to_path_buf();
        Ok(())
    }
}
```
Nota: `reload()` reabre `self.path` (o caminho **de antes** da tentativa, pois `self.path` só muda no sucesso), usando a senha guardada em `PdfDoc`; por isso funciona também para PDFs protegidos. Em `doc.rs` ajuste o import para `use mupdf::pdf::{Encryption, PdfAnnotationType, PdfDocument, PdfPage, PdfWriteOptions};` (o `PdfDocument` já está importado da Tarefa 2).

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core --test annotations 2>&1 | tail -n 12`
Esperado: `4 passed`. Se uma variante falhar (por exemplo, a nota perde o texto ou a tinta não é gravada), aplique a regra do Spike 1 descrita no topo da tarefa.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): anotações (destaque, sublinhado, nota, tinta) e salvamento seguro" -m "Spike 1 validado: <liste aqui os tipos que funcionaram>" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```
(Preencha a linha do Spike 1 com o resultado real antes de commitar.)

---

### Tarefa 6: `pdf-core` — índice, senha, arquivos ruins e caminhos com acento

**Arquivos:**
- Modificar: `crates/pdf-core/src/doc.rs`
- Criar: `crates/pdf-core/tests/robustness.rs`

**Interfaces:**
- Produz:
```rust
PdfDoc::outline(&self) -> Result<Vec<OutlineItem>>          // achatado em ordem, com depth
PdfDoc::summary(&self) -> Result<DocSummary>                 // sizes de todas as páginas + outline + annots
```

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/robustness.rs`:
```rust
mod common;
use common::write_sample;
use mupdf::pdf::{Encryption, PdfDocument, PdfWriteOptions};
use pdf_core::sample::sample_pdf;
use pdf_core::{NewAnnot, PdfDoc, PdfError};

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn bad_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let good = sample_pdf(2, 0);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("vazio.pdf", vec![]),
        ("lixo.pdf", (0..4096u32).map(|i| (i * 31 % 251) as u8).collect()),
        ("so_cabecalho.pdf", b"%PDF-1.4\n".to_vec()),
        ("truncado.pdf", good[..good.len() / 2].to_vec()),
    ];
    for (name, bytes) in cases {
        let path = write(&dir, name, &bytes);
        match PdfDoc::open(&path, None) {
            Err(_) => {}
            Ok(doc) => {
                // se o MuPDF conseguir reparar, nada pode dar panic
                let n = doc.page_count().unwrap_or(0);
                for p in 0..n.min(3) {
                    let _ = doc.page_size(p);
                    let _ = doc.render(p, 0.5, false);
                }
            }
        }
    }
    assert!(PdfDoc::open(&dir.path().join("nao_existe.pdf"), None).is_err());
}

#[test]
fn zero_page_document_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "zero.pdf", &sample_pdf(0, 0));
    if let Ok(doc) = PdfDoc::open(&path, None) {
        assert_eq!(doc.page_count().unwrap_or(0), 0);
        assert!(doc.summary().map(|s| s.sizes.is_empty()).unwrap_or(true));
    }
}

fn encrypted(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("senha.pdf");
    let doc = PdfDocument::from_bytes(&sample_pdf(1, 0)).unwrap();
    let mut opts = PdfWriteOptions::default();
    opts.set_encryption(Encryption::Aes256).set_owner_password("dono").set_user_password("segredo");
    doc.save_with_options(path.to_str().unwrap(), opts).unwrap();
    path
}

#[test]
fn password_flow() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted(&dir);
    assert!(matches!(PdfDoc::open(&path, None), Err(PdfError::PasswordRequired)));
    assert!(matches!(PdfDoc::open(&path, Some("errada")), Err(PdfError::WrongPassword)));
    assert_eq!(PdfDoc::open(&path, Some("segredo")).unwrap().page_count().unwrap(), 1);
}

#[test]
fn saving_keeps_the_file_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let path = encrypted(&dir);
    let mut doc = PdfDoc::open(&path, Some("segredo")).unwrap();
    let hit = doc.search(0, "Hello").unwrap()[0];
    doc.save_with(&[NewAnnot::Highlight { page: 0, rects: vec![hit] }], &path).unwrap();
    assert!(matches!(PdfDoc::open(&path, None), Err(PdfError::PasswordRequired)));
    let again = PdfDoc::open(&path, Some("segredo")).unwrap();
    assert_eq!(again.annotations(0).unwrap().len(), 1);
}

#[test]
fn accented_path_with_spaces_opens_and_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "Relatório final (1) — ação.pdf", 2, 0);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    let hit = doc.search(1, "Hello").unwrap()[0];
    doc.save_with(&[NewAnnot::Highlight { page: 1, rects: vec![hit] }], &path).unwrap();
    assert_eq!(PdfDoc::open(&path, None).unwrap().annotations(1).unwrap().len(), 1);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers.len(), 1, "sem arquivo temporário sobrando: {leftovers:?}");
}

#[test]
fn outline_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 3, 0), None).unwrap();
    let outline = doc.outline().unwrap();
    assert_eq!(outline.len(), 3);
    assert_eq!(outline[1].title, "Capitulo 2");
    assert_eq!(outline[1].page, Some(1));
    assert_eq!(outline[1].depth, 0);
    let s = doc.summary().unwrap();
    assert_eq!(s.sizes.len(), 3);
    assert_eq!(s.outline.len(), 3);
    assert!(s.annots.is_empty());
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test robustness 2>&1 | tail -n 8`
Esperado: FALHA de compilação (`outline`/`summary` não existem).

- [ ] **Passo 3: Implementar em `doc.rs`**

Imports: `use crate::types::{DocSummary, OutlineItem};`. (A senha já é guardada em `PdfDoc` e reaproveitada por `reload` desde as Tarefas 2 e 5.) Nova API:
```rust
fn flatten(items: &[mupdf::Outline], depth: usize, out: &mut Vec<OutlineItem>) {
    for it in items {
        out.push(OutlineItem {
            title: it.title.clone(),
            page: it.dest.as_ref().map(|d| d.loc.page_number as i32),
            depth,
        });
        flatten(&it.down, depth + 1, out);
    }
}

impl PdfDoc {
    pub fn outline(&self) -> Result<Vec<OutlineItem>> {
        let mut out = Vec::new();
        flatten(&self.inner.outlines()?, 0, &mut out);
        Ok(out)
    }

    pub fn summary(&self) -> Result<DocSummary> {
        let sizes = (0..self.page_count()?).map(|p| self.page_size(p)).collect::<Result<Vec<_>>>()?;
        Ok(DocSummary {
            path: self.path.clone(),
            sizes,
            outline: self.outline().unwrap_or_default(),
            annots: self.all_annotations().unwrap_or_default(),
        })
    }
}
```
(Índice ou anotações corrompidos não impedem abrir o documento; por isso `unwrap_or_default` nesses dois.)

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core 2>&1 | tail -n 15`
Esperado: todos os testes do crate passam. Se `saving_keeps_the_file_encrypted` falhar (arquivo sai sem senha), isso é um **bug de segurança**: investigue as opções de escrita (`Encryption::Keep`, senhas) antes de seguir; não relaxe o teste.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): índice, senha, resumo do documento e testes de robustez" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 7: `pdf-core` — thread dedicada (worker) e protocolo de mensagens

**Arquivos:**
- Criar: `crates/pdf-core/src/worker.rs`, `crates/pdf-core/tests/worker.rs`, `crates/pdf-core/examples/make_sample.rs`
- Modificar: `crates/pdf-core/src/lib.rs`

**Interfaces:**
- Consome: `PdfDoc` (todas as operações), `DocSummary`.
- Produz:
```rust
#[derive(Clone, Default)] pub struct CancelToken(/* Arc<AtomicBool> */);
impl CancelToken { pub fn new() -> Self; pub fn cancel(&self); pub fn is_cancelled(&self) -> bool; }
pub enum Request {
    Open { path: PathBuf, password: Option<String> },
    Render { page: i32, scale: f32, invert: bool, cancel: CancelToken },
    Search { id: u64, needle: String, cancel: CancelToken },
    Words { tag: u64, page: i32, area: PdfRect },
    Save { pending: Vec<NewAnnot>, dest: PathBuf },
}
pub enum Response {
    Opened(DocSummary), OpenFailed(PdfError),
    Rendered { page: i32, scale: f32, invert: bool, image: RenderedPage },
    RenderFailed { page: i32, error: String },
    SearchHits { id: u64, page: i32, rects: Vec<PdfRect> }, SearchDone { id: u64 },
    Words { tag: u64, page: i32, words: Vec<Word> },
    Saved { annots: Vec<AnnotInfo> }, SaveFailed(String),
}
pub struct Worker { pub rx: std::sync::mpsc::Receiver<Response> /* + tx privado */ }
impl Worker { pub fn spawn(wake: impl Fn() + Send + 'static) -> Worker; pub fn send(&self, r: Request); }
```
Regras: pedidos `Render`/`Search` com token cancelado são ignorados sem resposta; `wake()` é chamado após cada resposta (a UI usa para pedir repaint); soltar o `Worker` encerra a thread.

- [ ] **Passo 1: Escrever os testes que falham**

`crates/pdf-core/tests/worker.rs`:
```rust
mod common;
use common::write_sample;
use pdf_core::{CancelToken, NewAnnot, PdfError, PdfRect, Request, Response, Worker};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn recv(w: &Worker) -> Response {
    w.rx.recv_timeout(Duration::from_secs(20)).expect("o worker deveria responder")
}

fn opened(pages: usize) -> (tempfile::TempDir, std::path::PathBuf, Worker, Arc<AtomicUsize>) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", pages, 0);
    let wakes = Arc::new(AtomicUsize::new(0));
    let w2 = wakes.clone();
    let worker = Worker::spawn(move || {
        w2.fetch_add(1, Ordering::SeqCst);
    });
    worker.send(Request::Open { path: path.clone(), password: None });
    assert!(matches!(recv(&worker), Response::Opened(s) if s.sizes.len() == pages));
    (dir, path, worker, wakes)
}

#[test]
fn cancelled_render_is_skipped_and_order_is_kept() {
    let (_d, _p, w, wakes) = opened(3);
    let cancelled = CancelToken::new();
    cancelled.cancel();
    w.send(Request::Render { page: 0, scale: 1.0, invert: false, cancel: cancelled });
    w.send(Request::Render { page: 1, scale: 1.0, invert: false, cancel: CancelToken::new() });
    match recv(&w) {
        Response::Rendered { page, .. } => assert_eq!(page, 1),
        other => panic!("esperava Rendered da página 1, veio {other:?}"),
    }
    assert!(wakes.load(Ordering::SeqCst) >= 2);
}

#[test]
fn search_streams_hits_then_done() {
    let (_d, _p, w, _) = opened(3);
    w.send(Request::Search { id: 7, needle: "Hello".into(), cancel: CancelToken::new() });
    let mut pages = vec![];
    loop {
        match recv(&w) {
            Response::SearchHits { id, page, rects } => {
                assert_eq!(id, 7);
                assert_eq!(rects.len(), 1);
                pages.push(page);
            }
            Response::SearchDone { id } => {
                assert_eq!(id, 7);
                break;
            }
            other => panic!("inesperado: {other:?}"),
        }
    }
    assert_eq!(pages, [0, 1, 2]);
}

#[test]
fn words_save_and_failed_open() {
    let (dir, path, w, _) = opened(1);
    w.send(Request::Words { tag: 3, page: 0, area: PdfRect::new(0.0, 0.0, 612.0, 792.0) });
    let rects = match recv(&w) {
        Response::Words { tag, words, .. } => {
            assert_eq!(tag, 3);
            assert_eq!(words.len(), 4);
            words.iter().filter(|x| x.text == "Hello").map(|x| x.rect).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    w.send(Request::Save { pending: vec![NewAnnot::Highlight { page: 0, rects }], dest: path });
    match recv(&w) {
        Response::Saved { annots } => assert_eq!(annots.len(), 1),
        other => panic!("{other:?}"),
    }
    w.send(Request::Save {
        pending: vec![NewAnnot::Note { page: 0, at: (1.0, 1.0), text: "x".into() }],
        dest: dir.path().join("nada").join("x.pdf"),
    });
    assert!(matches!(recv(&w), Response::SaveFailed(_)));
    w.send(Request::Open { path: dir.path().join("nao_existe.pdf"), password: None });
    assert!(matches!(recv(&w), Response::OpenFailed(PdfError::Open(_))));
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p pdf-core --test worker 2>&1 | tail -n 6`
Esperado: FALHA de compilação.

- [ ] **Passo 3: Implementar `worker.rs`**

```rust
use crate::error::PdfError;
use crate::types::{AnnotInfo, DocSummary, NewAnnot, PdfRect, RenderedPage, Word};
use crate::PdfDoc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub enum Request {
    Open { path: PathBuf, password: Option<String> },
    Render { page: i32, scale: f32, invert: bool, cancel: CancelToken },
    Search { id: u64, needle: String, cancel: CancelToken },
    Words { tag: u64, page: i32, area: PdfRect },
    Save { pending: Vec<NewAnnot>, dest: PathBuf },
}

#[derive(Debug)]
pub enum Response {
    Opened(DocSummary),
    OpenFailed(PdfError),
    Rendered { page: i32, scale: f32, invert: bool, image: RenderedPage },
    RenderFailed { page: i32, error: String },
    SearchHits { id: u64, page: i32, rects: Vec<PdfRect> },
    SearchDone { id: u64 },
    Words { tag: u64, page: i32, words: Vec<Word> },
    Saved { annots: Vec<AnnotInfo> },
    SaveFailed(String),
}

pub struct Worker {
    tx: Sender<Request>,
    pub rx: Receiver<Response>,
}

impl Worker {
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, req_rx) = mpsc::channel();
        let (resp_tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("pdf-worker".into())
            .stack_size(16 << 20)
            .spawn(move || run(req_rx, resp_tx, wake))
            .expect("não foi possível criar a thread do PDF");
        Self { tx, rx }
    }

    pub fn send(&self, request: Request) {
        let _ = self.tx.send(request);
    }
}

fn run(requests: Receiver<Request>, out: Sender<Response>, wake: impl Fn()) {
    let send = |r: Response| {
        let _ = out.send(r);
        wake();
    };
    let mut doc: Option<PdfDoc> = None;
    while let Ok(request) = requests.recv() {
        match request {
            Request::Open { path, password } => match PdfDoc::open(&path, password.as_deref()) {
                Ok(d) => match d.summary() {
                    Ok(summary) => {
                        doc = Some(d);
                        send(Response::Opened(summary));
                    }
                    Err(e) => send(Response::OpenFailed(e)),
                },
                Err(e) => send(Response::OpenFailed(e)),
            },
            Request::Render { page, scale, invert, cancel } => {
                let Some(d) = doc.as_ref().filter(|_| !cancel.is_cancelled()) else { continue };
                match d.render(page, scale, invert) {
                    Ok(image) => send(Response::Rendered { page, scale, invert, image }),
                    Err(e) => send(Response::RenderFailed { page, error: e.to_string() }),
                }
            }
            Request::Search { id, needle, cancel } => {
                let Some(d) = doc.as_ref() else { continue };
                let pages = d.page_count().unwrap_or(0);
                let mut completed = true;
                for page in 0..pages {
                    if cancel.is_cancelled() {
                        completed = false;
                        break;
                    }
                    if let Ok(rects) = d.search(page, &needle) {
                        if !rects.is_empty() {
                            send(Response::SearchHits { id, page, rects });
                        }
                    }
                }
                if completed {
                    send(Response::SearchDone { id });
                }
            }
            Request::Words { tag, page, area } => {
                let words = doc.as_ref().and_then(|d| d.words_in_rect(page, area).ok()).unwrap_or_default();
                send(Response::Words { tag, page, words });
            }
            Request::Save { pending, dest } => {
                let Some(d) = doc.as_mut() else {
                    send(Response::SaveFailed("nenhum documento aberto".into()));
                    continue;
                };
                match d.save_with(&pending, &dest) {
                    Ok(()) => send(Response::Saved { annots: d.all_annotations().unwrap_or_default() }),
                    Err(e) => send(Response::SaveFailed(e.to_string())),
                }
            }
        }
    }
}
```
`lib.rs`: acrescente `mod worker;` e `pub use worker::{CancelToken, Request, Response, Worker};`.

`crates/pdf-core/examples/make_sample.rs` (usado pelo script de medição):
```rust
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("uso: make_sample <saida.pdf> [paginas]");
    let pages = args.next().map_or(100, |p| p.parse().expect("número de páginas inválido"));
    std::fs::write(path, pdf_core::sample::sample_pdf(pages, 0)).expect("falha ao gravar");
}
```

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p pdf-core 2>&1 | tail -n 10`
Esperado: todos os testes do crate passam (inclui os anteriores).

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p pdf-core --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(pdf-core): thread dedicada com protocolo de mensagens e cancelamento" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 8: `app-state` — zoom e layout virtualizado

**Arquivos:**
- Criar: `crates/app-state/src/{zoom.rs,layout.rs}`
- Modificar: `crates/app-state/src/lib.rs`

**Interfaces:**
- Produz:
```rust
// zoom.rs
pub const MIN_ZOOM: f32 = 0.1; pub const MAX_ZOOM: f32 = 8.0;
pub fn clamp_zoom(z: f32) -> f32;            // NaN/inf => 1.0
pub fn zoom_in(z: f32) -> f32; pub fn zoom_out(z: f32) -> f32;   // x1.25 / ÷1.25, sempre limitado
pub fn fit_width(page_w: f32, avail_w: f32) -> f32;              // entradas inválidas => 1.0
pub fn quantize(z: f32) -> u32;              // chave de cache: round(z*1000)
// layout.rs
pub const PAGE_GAP: f32 = 12.0;
#[derive(Debug, Default, Clone)] pub struct Layout { /* privado */ }
impl Layout {
    pub fn new(sizes: &[(f32, f32)], zoom: f32) -> Layout;   // tamanhos em pontos; zoom inválido => 1.0; tamanho inválido => 0
    pub fn page_count(&self) -> usize; pub fn total_height(&self) -> f32; pub fn max_width(&self) -> f32;
    pub fn page_top(&self, i: usize) -> f32; pub fn page_width(&self, i: usize) -> f32; pub fn page_height(&self, i: usize) -> f32;
    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> std::ops::Range<usize>;
    pub fn page_at(&self, y: f32) -> usize;                  // 0 se não houver páginas
    pub fn expand(&self, r: std::ops::Range<usize>, by: usize) -> std::ops::Range<usize>;
}
```

- [ ] **Passo 1: Escrever os testes que falham** (dentro dos próprios arquivos, módulo `tests`)

`zoom.rs` (final do arquivo):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_steps() {
        assert_eq!(clamp_zoom(100.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(0.0), MIN_ZOOM);
        assert_eq!(clamp_zoom(-3.0), MIN_ZOOM);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        assert_eq!(clamp_zoom(f32::INFINITY), 1.0);
        assert!((zoom_in(1.0) - 1.25).abs() < 1e-6);
        assert!((zoom_out(1.25) - 1.0).abs() < 1e-6);
        assert_eq!(zoom_in(MAX_ZOOM), MAX_ZOOM);
        assert_eq!(zoom_out(MIN_ZOOM), MIN_ZOOM);
    }

    #[test]
    fn fit_width_handles_bad_input() {
        assert!((fit_width(500.0, 1000.0) - 2.0).abs() < 1e-6);
        assert_eq!(fit_width(0.0, 1000.0), 1.0);
        assert_eq!(fit_width(500.0, -5.0), 1.0);
        assert_eq!(fit_width(f32::NAN, 100.0), 1.0);
    }

    #[test]
    fn quantize_is_stable() {
        assert_eq!(quantize(1.5), 1500);
        assert_eq!(quantize(1.5004), 1500);
    }
}
```
`layout.rs` (final do arquivo):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn ten() -> Layout {
        Layout::new(&[(100.0, 200.0); 10], 1.0)
    }

    #[test]
    fn positions_and_totals() {
        let l = ten();
        assert_eq!(l.page_count(), 10);
        assert_eq!(l.page_top(0), PAGE_GAP);
        assert_eq!(l.page_top(1), PAGE_GAP + 200.0 + PAGE_GAP);
        assert_eq!(l.total_height(), PAGE_GAP + 10.0 * (200.0 + PAGE_GAP));
        assert_eq!(l.max_width(), 100.0);
        let z = Layout::new(&[(100.0, 200.0)], 2.0);
        assert_eq!((z.page_width(0), z.page_height(0)), (200.0, 400.0));
    }

    #[test]
    fn visible_range_covers_only_intersecting_pages() {
        let l = ten(); // páginas em 12..212, 224..424, 436..636, ...
        assert_eq!(l.visible_range(0.0, 300.0), 0..2);
        assert_eq!(l.visible_range(230.0, 10.0), 1..2);
        assert!(l.visible_range(215.0, 5.0).is_empty(), "a janela 215..220 cai na lacuna entre as páginas 0 e 1");
    }

    #[test]
    fn visible_range_edges_do_not_panic() {
        let l = ten();
        assert_eq!(l.visible_range(1.0e9, 100.0), 10..10);
        assert_eq!(l.visible_range(-500.0, 100.0), 0..0);
        assert!(l.visible_range(f32::NAN, 100.0).len() <= 10);
        assert_eq!(l.visible_range(0.0, 0.0), 0..0);
        let empty = Layout::new(&[], 1.0);
        assert_eq!(empty.visible_range(0.0, 500.0), 0..0);
        assert_eq!(empty.page_at(100.0), 0);
        assert_eq!(empty.expand(0..0, 3), 0..0);
    }

    #[test]
    fn bad_zoom_and_sizes_are_sanitized() {
        let l = Layout::new(&[(f32::NAN, -5.0), (100.0, 100.0)], f32::NAN);
        assert_eq!((l.page_width(0), l.page_height(0)), (0.0, 0.0));
        assert_eq!(l.page_width(1), 100.0);
        let z = Layout::new(&[(100.0, 100.0)], -2.0);
        assert_eq!(z.page_width(0), 100.0);
    }

    #[test]
    fn page_at_and_expand() {
        let l = ten();
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(230.0), 1);
        assert_eq!(l.page_at(1.0e9), 9);
        assert_eq!(l.expand(2..4, 1), 1..5);
        assert_eq!(l.expand(0..2, 3), 0..5);
        assert_eq!(l.expand(8..10, 3), 5..10);
    }
}
```
- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state 2>&1 | tail -n 6`
Esperado: FALHA de compilação (funções ainda não existem).

- [ ] **Passo 3: Implementar**

`zoom.rs` (acima do módulo de testes):
```rust
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 8.0;

pub fn clamp_zoom(z: f32) -> f32 {
    if z.is_finite() {
        z.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

pub fn zoom_in(z: f32) -> f32 {
    clamp_zoom(z * 1.25)
}

pub fn zoom_out(z: f32) -> f32 {
    clamp_zoom(z / 1.25)
}

pub fn fit_width(page_w: f32, avail_w: f32) -> f32 {
    if page_w.is_finite() && page_w > 0.0 && avail_w.is_finite() && avail_w > 0.0 {
        clamp_zoom(avail_w / page_w)
    } else {
        1.0
    }
}

pub fn quantize(z: f32) -> u32 {
    (z * 1000.0).round() as u32
}
```
`layout.rs` (acima do módulo de testes):
```rust
use std::ops::Range;

pub const PAGE_GAP: f32 = 12.0;

#[derive(Debug, Default, Clone)]
pub struct Layout {
    tops: Vec<f32>,
    widths: Vec<f32>,
    heights: Vec<f32>,
    max_width: f32,
    total: f32,
}

fn sane(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

impl Layout {
    pub fn new(sizes: &[(f32, f32)], zoom: f32) -> Self {
        let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
        let mut l = Layout::default();
        let mut y = PAGE_GAP;
        for &(w, h) in sizes {
            let (w, h) = (sane(w) * zoom, sane(h) * zoom);
            l.tops.push(y);
            l.widths.push(w);
            l.heights.push(h);
            l.max_width = l.max_width.max(w);
            y += h + PAGE_GAP;
        }
        l.total = y;
        l
    }

    pub fn page_count(&self) -> usize {
        self.tops.len()
    }
    pub fn total_height(&self) -> f32 {
        self.total
    }
    pub fn max_width(&self) -> f32 {
        self.max_width
    }
    pub fn page_top(&self, i: usize) -> f32 {
        self.tops[i]
    }
    pub fn page_width(&self, i: usize) -> f32 {
        self.widths[i]
    }
    pub fn page_height(&self, i: usize) -> f32 {
        self.heights[i]
    }

    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> Range<usize> {
        let n = self.tops.len();
        if n == 0 || !(viewport_h > 0.0) || !scroll_y.is_finite() {
            return 0..0;
        }
        let bottom = scroll_y + viewport_h;
        let (mut lo, mut hi) = (0, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.tops[mid] + self.heights[mid] <= scroll_y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let last = self.tops.partition_point(|&t| t < bottom);
        if lo >= last {
            return lo.min(n)..lo.min(n);
        }
        lo..last
    }

    pub fn page_at(&self, y: f32) -> usize {
        let n = self.tops.len();
        if n == 0 {
            return 0;
        }
        self.tops.partition_point(|&t| t <= y).saturating_sub(1).min(n - 1)
    }

    pub fn expand(&self, r: Range<usize>, by: usize) -> Range<usize> {
        if self.tops.is_empty() {
            return 0..0;
        }
        r.start.saturating_sub(by)..(r.end + by).min(self.tops.len())
    }
}
```
Nota: o teste `visible_range(-500.0, 100.0) == 0..0` vale porque a janela -500..-400 não toca nenhuma página (primeira página começa em 12). Confira: `lo` = 0 (nenhuma página termina antes de -500), `last` = `partition_point(t < -400)` = 0, então `lo >= last` e devolve `0..0`.

`lib.rs`:
```rust
//! Estado e lógica pura do app (sem UI e sem MuPDF direto).
pub mod layout;
pub mod zoom;
```

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p app-state 2>&1 | tail -n 10`
Esperado: todos passam.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p app-state --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(app-state): zoom e layout virtualizado das páginas" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 9: `app-state` — cache LRU limitado por bytes

**Arquivos:**
- Criar: `crates/app-state/src/cache.rs`
- Modificar: `crates/app-state/src/lib.rs`

**Interfaces:**
- Produz:
```rust
pub trait Weighed { fn weight(&self) -> usize; }
pub struct LruCache<K, V> { /* privado */ }   // K: Hash + Eq + Clone, V: Weighed
impl<K, V> LruCache<K, V> {
    pub fn new(budget_bytes: usize) -> Self;
    pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool; pub fn bytes(&self) -> usize;
    pub fn contains(&self, k: &K) -> bool;
    pub fn peek(&self, k: &K) -> Option<&V>;          // não altera a recência
    pub fn keys(&self) -> impl Iterator<Item = &K>;
    pub fn get(&mut self, k: &K) -> Option<&V>;       // marca como usado recentemente
    pub fn insert(&mut self, k: K, v: V);             // evita estourar o orçamento removendo os menos recentes; o item novo nunca é removido
    pub fn clear(&mut self);
}
```

- [ ] **Passo 1: Escrever os testes que falham** (final de `cache.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct Blob(usize);
    impl Weighed for Blob {
        fn weight(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn evicts_least_recently_used_over_budget() {
        let mut c = LruCache::new(100);
        c.insert("a", Blob(40));
        c.insert("b", Blob(40));
        assert!(c.get(&"a").is_some()); // "a" fica mais recente que "b"
        c.insert("c", Blob(40)); // 120 > 100 => remove "b"
        assert!(c.contains(&"a") && c.contains(&"c") && !c.contains(&"b"));
        assert_eq!(c.bytes(), 80);
    }

    #[test]
    fn newest_item_is_kept_even_if_larger_than_budget() {
        let mut c = LruCache::new(10);
        c.insert("a", Blob(5));
        c.insert("big", Blob(50));
        assert!(c.contains(&"big") && !c.contains(&"a"));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn replacing_a_key_updates_byte_accounting() {
        let mut c = LruCache::new(100);
        c.insert("a", Blob(30));
        c.insert("a", Blob(70));
        assert_eq!((c.len(), c.bytes()), (1, 70));
    }

    #[test]
    fn peek_does_not_refresh_and_clear_empties() {
        let mut c = LruCache::new(80);
        c.insert("a", Blob(40));
        c.insert("b", Blob(40));
        assert!(c.peek(&"a").is_some()); // não protege "a"
        c.insert("c", Blob(40)); // remove "a" (o mais antigo)
        assert!(!c.contains(&"a"));
        assert_eq!(c.keys().count(), 2);
        c.clear();
        assert!(c.is_empty() && c.bytes() == 0);
    }
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state cache 2>&1 | tail -n 6`
Esperado: FALHA de compilação.

- [ ] **Passo 3: Implementar** (acima dos testes)

```rust
use std::collections::HashMap;
use std::hash::Hash;

pub trait Weighed {
    fn weight(&self) -> usize;
}

pub struct LruCache<K, V> {
    map: HashMap<K, (V, u64)>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl<K: Hash + Eq + Clone, V: Weighed> LruCache<K, V> {
    pub fn new(budget_bytes: usize) -> Self {
        Self { map: HashMap::new(), tick: 0, bytes: 0, budget: budget_bytes }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }
    pub fn peek(&self, k: &K) -> Option<&V> {
        self.map.get(k).map(|e| &e.0)
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.map.keys()
    }

    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|e| {
            e.1 = tick;
            &e.0
        })
    }

    pub fn insert(&mut self, k: K, v: V) {
        self.tick += 1;
        self.bytes += v.weight();
        if let Some((old, _)) = self.map.insert(k, (v, self.tick)) {
            self.bytes -= old.weight();
        }
        while self.bytes > self.budget && self.map.len() > 1 {
            let Some(oldest) = self.map.iter().min_by_key(|(_, e)| e.1).map(|(k, _)| k.clone()) else { break };
            if let Some((v, _)) = self.map.remove(&oldest) {
                self.bytes -= v.weight();
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }
}
```
`lib.rs`: acrescente `pub mod cache;`.

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p app-state 2>&1 | tail -n 8`
Esperado: todos passam (zoom, layout, cache).

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p app-state --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(app-state): cache LRU limitado por bytes" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 10: `app-state` — ferramentas e anotações pendentes com desfazer/refazer

**Arquivos:**
- Criar: `crates/app-state/src/{tools.rs,annots.rs}`
- Modificar: `crates/app-state/src/lib.rs`

**Interfaces:**
- Consome: `pdf_core::NewAnnot`.
- Produz:
```rust
// tools.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Tool { #[default] Select, Highlight, Underline, Note, Ink }
impl Tool { pub const ALL: [Tool; 5]; pub fn label(self) -> &'static str; }
// annots.rs
#[derive(Debug, Default)] pub struct PendingAnnots { /* privado */ }
impl PendingAnnots {
    pub fn add(&mut self, a: NewAnnot) -> bool;   // false (e ignora) se a.is_empty(); limpa a pilha de refazer
    pub fn undo(&mut self) -> bool; pub fn redo(&mut self) -> bool;
    pub fn items(&self) -> &[NewAnnot]; pub fn is_dirty(&self) -> bool; pub fn clear(&mut self);
}
```

- [ ] **Passo 1: Escrever os testes que falham** (final de `annots.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    fn hl(page: i32) -> NewAnnot {
        NewAnnot::Highlight { page, rects: vec![PdfRect::new(0.0, 0.0, 10.0, 10.0)] }
    }

    #[test]
    fn add_undo_redo() {
        let mut p = PendingAnnots::default();
        assert!(!p.is_dirty());
        assert!(p.add(hl(0)) && p.add(hl(1)));
        assert_eq!(p.items().len(), 2);
        assert!(p.undo());
        assert_eq!(p.items(), &[hl(0)]);
        assert!(p.redo());
        assert_eq!(p.items(), &[hl(0), hl(1)]);
        assert!(!p.redo());
    }

    #[test]
    fn a_new_edit_discards_the_redo_stack() {
        let mut p = PendingAnnots::default();
        p.add(hl(0));
        p.undo();
        p.add(hl(2));
        assert!(!p.redo());
        assert_eq!(p.items(), &[hl(2)]);
    }

    #[test]
    fn empty_annotations_are_ignored() {
        let mut p = PendingAnnots::default();
        assert!(!p.add(NewAnnot::Highlight { page: 0, rects: vec![] }));
        assert!(!p.add(NewAnnot::Ink { page: 0, strokes: vec![vec![(1.0, 1.0)]] }));
        assert!(!p.add(NewAnnot::Note { page: 0, at: (0.0, 0.0), text: "  ".into() }));
        assert!(!p.is_dirty());
        assert!(!p.undo());
    }

    #[test]
    fn clear_resets_everything() {
        let mut p = PendingAnnots::default();
        p.add(hl(0));
        p.undo();
        p.clear();
        assert!(!p.is_dirty() && !p.redo());
    }
}
```

- [ ] **Passo 2: Rodar e ver falhar**

Run: `cargo test -p app-state annots 2>&1 | tail -n 6`
Esperado: FALHA de compilação.

- [ ] **Passo 3: Implementar**

`annots.rs` (acima dos testes):
```rust
use pdf_core::NewAnnot;

#[derive(Debug, Default)]
pub struct PendingAnnots {
    items: Vec<NewAnnot>,
    undone: Vec<NewAnnot>,
}

impl PendingAnnots {
    pub fn add(&mut self, a: NewAnnot) -> bool {
        if a.is_empty() {
            return false;
        }
        self.items.push(a);
        self.undone.clear();
        true
    }

    pub fn undo(&mut self) -> bool {
        match self.items.pop() {
            Some(a) => {
                self.undone.push(a);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.undone.pop() {
            Some(a) => {
                self.items.push(a);
                true
            }
            None => false,
        }
    }

    pub fn items(&self) -> &[NewAnnot] {
        &self.items
    }

    pub fn is_dirty(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.undone.clear();
    }
}
```
`tools.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Highlight,
    Underline,
    Note,
    Ink,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Select, Tool::Highlight, Tool::Underline, Tool::Note, Tool::Ink];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selecionar",
            Tool::Highlight => "Destacar",
            Tool::Underline => "Sublinhar",
            Tool::Note => "Nota",
            Tool::Ink => "Desenhar",
        }
    }
}
```
`lib.rs`: acrescente `pub mod annots;` e `pub mod tools;`.

- [ ] **Passo 4: Rodar e ver passar**

Run: `cargo test -p app-state 2>&1 | tail -n 8`
Esperado: todos passam.

- [ ] **Passo 5: `simplify`, clippy e commit**

```bash
cargo clippy -p app-state --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(app-state): ferramentas e anotações pendentes com desfazer/refazer" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 11: `gera-ui` — janela, abrir arquivo e rolagem com páginas renderizadas (Spike 3)

**Arquivos:**
- Criar: `crates/gera-ui/src/{app.rs,view.rs,canvas.rs}`
- Modificar: `crates/gera-ui/src/main.rs`

**Interfaces:**
- Consome: `pdf_core::{Worker, Request, Response, CancelToken, DocSummary, PageSize, RenderedPage}`, `app_state::{layout::Layout, zoom, cache::{LruCache, Weighed}}`.
- Produz:
```rust
// view.rs
pub type RenderKey = (i32, u32, bool);                       // (página, escala quantizada, invertido)
pub struct DocView { pub path: PathBuf, pub sizes: Vec<PageSize>, pub zoom: f32, pub fit_pending: bool, pub layout: Layout, pub dark: bool, /* privado: ppp, cache, requested, wanted */ }
impl DocView {
    pub fn new(path: PathBuf, sizes: Vec<PageSize>) -> Self;
    pub fn set_zoom(&mut self, z: f32) -> f32;               // devolve a razão novo/antigo; refaz o layout
    pub fn fit_width(&mut self, avail: f32) -> f32;
    pub fn set_ppp(&mut self, ppp: f32);
    pub fn begin_frame(&mut self); pub fn end_frame(&mut self);   // end_frame cancela pedidos que ninguém pediu neste quadro
    pub fn texture_at(&mut self, page: i32, scale: f32) -> Option<egui::TextureId>;
    pub fn any_texture(&self, page: i32) -> Option<egui::TextureId>;   // qualquer escala (placeholder de baixa resolução)
    pub fn request_at(&mut self, worker: &Worker, page: i32, scale: f32);
    pub fn page_scale(&self) -> f32;                          // zoom * ppp
    pub fn on_rendered(&mut self, ctx: &egui::Context, page: i32, scale: f32, invert: bool, img: RenderedPage);
    pub fn clear_cache(&mut self);
}
// canvas.rs
pub struct CanvasFrame { pub scroll_y: f32, pub current_page: usize }
pub fn show_pages(ui: &mut egui::Ui, view: &mut DocView, worker: &Worker, jump_to: Option<f32>,
                  on_page: &mut dyn FnMut(&mut egui::Ui, usize, egui::Rect)) -> CanvasFrame;
```
`on_page` é chamado depois de pintar cada página visível (as tarefas 13 usam para sobrepor anotações e tratar o mouse).

Esta tarefa é o **Spike 3**: confirmar que a rolagem fica fluida e que só páginas visíveis são renderizadas.

- [ ] **Passo 1: Escrever `view.rs`**

```rust
use app_state::cache::{LruCache, Weighed};
use app_state::layout::Layout;
use app_state::zoom;
use pdf_core::{CancelToken, PageSize, RenderedPage, Request, Worker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub type RenderKey = (i32, u32, bool);

/// Orçamento de texturas de página em memória (RGBA na GPU/CPU).
const CACHE_BUDGET_BYTES: usize = 96 * 1024 * 1024;

struct CachedTexture {
    handle: egui::TextureHandle,
    bytes: usize,
}

impl Weighed for CachedTexture {
    fn weight(&self) -> usize {
        self.bytes
    }
}

pub struct DocView {
    pub path: PathBuf,
    pub sizes: Vec<PageSize>,
    pub zoom: f32,
    pub fit_pending: bool,
    pub layout: Layout,
    pub dark: bool,
    ppp: f32,
    cache: LruCache<RenderKey, CachedTexture>,
    requested: HashMap<RenderKey, CancelToken>,
    wanted: HashSet<RenderKey>,
}

impl DocView {
    pub fn new(path: PathBuf, sizes: Vec<PageSize>) -> Self {
        let mut view = Self {
            path,
            sizes,
            zoom: 1.0,
            fit_pending: true,
            layout: Layout::default(),
            dark: false,
            ppp: 1.0,
            cache: LruCache::new(CACHE_BUDGET_BYTES),
            requested: HashMap::new(),
            wanted: HashSet::new(),
        };
        view.relayout();
        view
    }

    fn relayout(&mut self) {
        let dims: Vec<(f32, f32)> = self.sizes.iter().map(|s| (s.width, s.height)).collect();
        self.layout = Layout::new(&dims, self.zoom);
    }

    pub fn set_zoom(&mut self, z: f32) -> f32 {
        let new = zoom::clamp_zoom(z);
        let ratio = new / self.zoom;
        self.zoom = new;
        self.relayout();
        ratio
    }

    pub fn fit_width(&mut self, avail: f32) -> f32 {
        let page_w = self.sizes.first().map_or(1.0, |s| s.width);
        self.set_zoom(zoom::fit_width(page_w, avail))
    }

    pub fn set_ppp(&mut self, ppp: f32) {
        if ppp.is_finite() && ppp > 0.0 {
            self.ppp = ppp;
        }
    }

    pub fn page_scale(&self) -> f32 {
        self.zoom * self.ppp
    }

    fn key(&self, page: i32, scale: f32) -> RenderKey {
        (page, zoom::quantize(scale), self.dark)
    }

    pub fn begin_frame(&mut self) {
        self.wanted.clear();
    }

    pub fn end_frame(&mut self) {
        let wanted = &self.wanted;
        self.requested.retain(|k, token| {
            let keep = wanted.contains(k);
            if !keep {
                token.cancel();
            }
            keep
        });
    }

    pub fn texture_at(&mut self, page: i32, scale: f32) -> Option<egui::TextureId> {
        let key = self.key(page, scale);
        self.cache.get(&key).map(|t| t.handle.id())
    }

    pub fn any_texture(&self, page: i32) -> Option<egui::TextureId> {
        let dark = self.dark;
        let key = self.cache.keys().find(|k| k.0 == page && k.2 == dark)?;
        self.cache.peek(key).map(|t| t.handle.id())
    }

    pub fn request_at(&mut self, worker: &Worker, page: i32, scale: f32) {
        let key = self.key(page, scale);
        self.wanted.insert(key);
        if self.cache.contains(&key) || self.requested.contains_key(&key) {
            return;
        }
        let cancel = CancelToken::new();
        self.requested.insert(key, cancel.clone());
        worker.send(Request::Render { page, scale, invert: self.dark, cancel });
    }

    pub fn on_rendered(&mut self, ctx: &egui::Context, page: i32, scale: f32, invert: bool, img: RenderedPage) {
        let key = (page, zoom::quantize(scale), invert);
        self.requested.remove(&key);
        if invert != self.dark {
            return; // resposta atrasada de antes de trocar o tema
        }
        let image = egui::ColorImage::from_rgb([img.width as usize, img.height as usize], &img.rgb);
        let handle = ctx.load_texture(format!("p{}-{}", key.0, key.1), image, egui::TextureOptions::LINEAR);
        let bytes = img.width as usize * img.height as usize * 4;
        self.cache.insert(key, CachedTexture { handle, bytes });
    }

    pub fn clear_cache(&mut self) {
        for token in self.requested.values() {
            token.cancel();
        }
        self.requested.clear();
        self.cache.clear();
    }
}
```

- [ ] **Passo 2: Escrever `canvas.rs`**

```rust
use crate::view::DocView;
use pdf_core::Worker;

pub struct CanvasFrame {
    pub scroll_y: f32,
    pub current_page: usize,
}

pub fn show_pages(
    ui: &mut egui::Ui,
    view: &mut DocView,
    worker: &Worker,
    jump_to: Option<f32>,
    on_page: &mut dyn FnMut(&mut egui::Ui, usize, egui::Rect),
) -> CanvasFrame {
    view.set_ppp(ui.ctx().pixels_per_point());
    let mut area = egui::ScrollArea::both().auto_shrink(false);
    if let Some(y) = jump_to {
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    let mut current_page = 0;
    let out = area.show_viewport(ui, |ui, viewport| {
        let total = egui::vec2(view.layout.max_width(), view.layout.total_height());
        ui.set_min_size(total);
        let origin = ui.max_rect().min;
        let content_w = ui.max_rect().width().max(total.x);
        let visible = view.layout.visible_range(viewport.min.y, viewport.height());
        current_page = view.layout.page_at(viewport.min.y + viewport.height() * 0.3);
        let scale = view.page_scale();
        for i in view.layout.expand(visible.clone(), 1) {
            view.request_at(worker, i as i32, scale);
        }
        for i in visible {
            let w = view.layout.page_width(i);
            let x = origin.x + (content_w - w).max(0.0) / 2.0;
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, origin.y + view.layout.page_top(i)),
                egui::vec2(w, view.layout.page_height(i)),
            );
            paint_page(ui, view, i as i32, rect, scale);
            on_page(ui, i, rect);
        }
    });
    CanvasFrame { scroll_y: out.state.offset.y, current_page }
}

fn paint_page(ui: &egui::Ui, view: &mut DocView, page: i32, rect: egui::Rect, scale: f32) {
    let texture = view.texture_at(page, scale).or_else(|| view.any_texture(page));
    let painter = ui.painter();
    match texture {
        Some(id) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(id, rect, uv, egui::Color32::WHITE);
        }
        None => {
            let gray = if view.dark { 30 } else { 245 };
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(gray));
        }
    }
}
```

- [ ] **Passo 3: Escrever `app.rs` (versão inicial: abrir, renderizar, zoom, arrastar-e-soltar, bench)**

```rust
use crate::canvas;
use crate::view::DocView;
use app_state::zoom;
use pdf_core::{DocSummary, Request, Response, Worker};
use std::path::PathBuf;
use std::time::Instant;

pub struct GeraApp {
    pub(crate) worker: Worker,
    pub(crate) view: Option<DocView>,
    pub(crate) status: String,
    pub(crate) scroll_y: f32,
    pub(crate) current_page: usize,
    pub(crate) jump_to: Option<f32>,
    started: Instant,
    bench: bool,
}

impl GeraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>, bench: bool, started: Instant) -> Self {
        let ctx = cc.egui_ctx.clone();
        let worker = Worker::spawn(move || ctx.request_repaint());
        let mut app = Self {
            worker,
            view: None,
            status: "Arraste um PDF para cá ou execute: gera-pdf-editor arquivo.pdf".into(),
            scroll_y: 0.0,
            current_page: 0,
            jump_to: None,
            started,
            bench,
        };
        if let Some(path) = path {
            app.open(path);
        }
        app
    }

    pub(crate) fn open(&mut self, path: PathBuf) {
        self.status = format!("Abrindo {}…", path.display());
        self.worker.send(Request::Open { path, password: None });
    }

    fn on_opened(&mut self, summary: DocSummary) {
        self.status = format!("{} — {} páginas", summary.path.display(), summary.sizes.len());
        self.view = Some(DocView::new(summary.path, summary.sizes));
        self.scroll_y = 0.0;
        self.jump_to = Some(0.0);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(response) = self.worker.rx.try_recv() {
            match response {
                Response::Opened(summary) => self.on_opened(summary),
                Response::OpenFailed(e) => self.status = format!("Erro: {e}"),
                Response::Rendered { page, scale, invert, image } => {
                    if let Some(view) = self.view.as_mut() {
                        view.on_rendered(ctx, page, scale, invert, image);
                    }
                    if self.bench {
                        self.finish_bench(ctx);
                    }
                }
                Response::RenderFailed { page, error } => self.status = format!("Página {}: {error}", page + 1),
                _ => {}
            }
        }
    }

    fn finish_bench(&mut self, ctx: &egui::Context) {
        let rss_kb = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("VmRSS:"))
                    .and_then(|v| v.split_whitespace().next().map(str::to_owned))
            })
            .unwrap_or_default();
        println!("BENCH open_ms={} rss_kb={rss_kb}", self.started.elapsed().as_millis());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.into_iter().filter_map(|f| f.path) {
            if self.view.is_none() {
                self.open(file);
            } else if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(file).spawn(); // uma janela por documento
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let Some(view) = self.view.as_mut() else { return };
            let mut new_zoom = None;
            if ui.button("−").clicked() {
                new_zoom = Some(zoom::zoom_out(view.zoom));
            }
            ui.label(format!("{:.0}%", view.zoom * 100.0));
            if ui.button("+").clicked() {
                new_zoom = Some(zoom::zoom_in(view.zoom));
            }
            if ui.button("Largura").clicked() {
                view.fit_pending = true;
            }
            if let Some(z) = new_zoom {
                let ratio = view.set_zoom(z);
                self.jump_to = Some(self.scroll_y * ratio);
            }
            ui.separator();
            ui.label(format!("Página {} / {}", self.current_page + 1, view.layout.page_count()));
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.as_mut() else {
            ui.centered_and_justified(|ui| {
                ui.label(&self.status);
            });
            return;
        };
        if view.fit_pending {
            let ratio = view.fit_width(ui.available_width() - 24.0);
            view.fit_pending = false;
            self.jump_to = Some(self.scroll_y * ratio);
        }
        let frame = canvas::show_pages(ui, view, &self.worker, self.jump_to.take(), &mut |_, _, _| {});
        self.scroll_y = frame.scroll_y;
        self.current_page = frame.current_page;
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
        if let Some(view) = self.view.as_mut() {
            view.begin_frame();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        if let Some(view) = self.view.as_mut() {
            view.end_frame();
        }
    }
}
```

- [ ] **Passo 4: Escrever `main.rs`**

```rust
mod app;
mod canvas;
mod view;

use std::path::PathBuf;
use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    let mut path = None;
    let mut bench = false;
    for arg in std::env::args().skip(1) {
        if arg == "--bench-open" {
            bench = true;
        } else {
            path = Some(PathBuf::from(arg));
        }
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_title("Gera PDF Editor")
            .with_app_id("gera-pdf-editor"),
        ..Default::default()
    };
    eframe::run_native(
        "Gera PDF Editor",
        options,
        Box::new(move |cc| Ok(Box::new(app::GeraApp::new(cc, path, bench, started)))),
    )
}
```

- [ ] **Passo 5: Compilar**

Run: `cargo build -p gera-ui 2>&1 | tail -n 30`
Esperado: `Finished`. Corrija divergências de API do egui 0.36 consultando `npx ctx7@latest docs /emilk/egui "<pergunta>"`; não mude as interfaces "Produz" da tarefa. (Pontos mais prováveis: assinatura de `Panel::top(..).show`, nome de `ViewportCommand::Close`, `ScrollArea::vertical_scroll_offset`.)

- [ ] **Passo 6: Verificação manual do Spike 3**

```bash
cargo run --release -q -p pdf-core --example make_sample -- /tmp/gera-100.pdf 100   # arquivo descartável fora do projeto
cargo run -p gera-ui -- /tmp/gera-100.pdf
```
Esperado: a janela abre mostrando a página 1 ("Page 1 Hello Gera"); rolar com o mouse até a página 100 e voltar sem engasgo; o indicador mostra "Página N / 100"; `−`/`+`/`Largura` mudam o zoom mantendo a posição aproximada; arrastar outro PDF para a janela abre uma segunda janela. Anote no commit se a rolagem ficou fluida. Se engasgar, perfile (`cargo run --release`) antes de seguir: renderizar fora da thread da UI já está garantido, então olhe `ColorImage`/upload de textura e o tamanho do cache.

- [ ] **Passo 7: `simplify`, clippy e commit**

```bash
cargo clippy -p gera-ui --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(gera-ui): janela, abertura e rolagem virtualizada com render sob demanda" -m "Spike 3: <resultado da rolagem>" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 12: `gera-ui` — busca, ir para página e atalhos de navegação

**Arquivos:**
- Criar: `crates/gera-ui/src/search.rs`
- Modificar: `crates/gera-ui/src/{main.rs,app.rs,canvas.rs}`

**Interfaces:**
- Consome: `Request::Search`, `Response::{SearchHits, SearchDone}`, `CancelToken`, `PdfRect`, `Layout`.
- Produz:
```rust
// search.rs
#[derive(Default)] pub struct SearchState { pub text: String, /* privado: id, cancel, hits, current, done */ }
impl SearchState {
    pub fn start(&mut self, worker: &Worker);                     // cancela a busca anterior; texto vazio limpa
    pub fn on_hits(&mut self, id: u64, page: i32, rects: Vec<PdfRect>);   // ignora id antigo
    pub fn on_done(&mut self, id: u64);
    pub fn hits_on(&self, page: i32) -> impl Iterator<Item = (usize, &PdfRect)>;  // (índice global, retângulo)
    pub fn current(&self) -> Option<(i32, PdfRect)>;
    pub fn step(&mut self, forward: bool) -> Option<(i32, PdfRect)>;       // circular
    pub fn summary(&self) -> String;                                         // "3 / 12", "Sem resultados", "Buscando…"
}
```

- [ ] **Passo 1: Testes unitários da lógica de busca** (final de `search.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn r(y: f32) -> PdfRect {
        PdfRect::new(0.0, y, 10.0, y + 5.0)
    }

    fn state() -> SearchState {
        let mut s = SearchState::default();
        s.id = 5;
        s.on_hits(5, 0, vec![r(1.0)]);
        s.on_hits(5, 2, vec![r(2.0), r(3.0)]);
        s.on_done(5);
        s
    }

    #[test]
    fn stale_ids_are_ignored() {
        let mut s = state();
        s.on_hits(4, 9, vec![r(9.0)]);
        assert_eq!(s.hits_on(9).count(), 0);
    }

    #[test]
    fn stepping_is_circular_and_summary_follows() {
        let mut s = state();
        assert_eq!(s.current().map(|c| c.0), Some(0));
        assert_eq!(s.summary(), "1 / 3");
        assert_eq!(s.step(true).map(|c| c.0), Some(2));
        s.step(true);
        assert_eq!(s.step(true).map(|c| c.0), Some(0), "volta ao início");
        assert_eq!(s.step(false).map(|c| c.0), Some(2), "volta ao fim");
    }

    #[test]
    fn empty_results_report_clearly() {
        let mut s = SearchState::default();
        s.text = "xyz".into();
        s.id = 1;
        s.on_done(1);
        assert_eq!(s.summary(), "Sem resultados");
        s.text.clear();
        assert_eq!(s.summary(), "", "busca vazia não mostra mensagem");
        assert!(s.current().is_none() && s.step(true).is_none());
    }
}
```

- [ ] **Passo 2: Implementar `search.rs`** (acima dos testes)

```rust
use pdf_core::{CancelToken, PdfRect, Request, Worker};

#[derive(Default)]
pub struct SearchState {
    pub text: String,
    pub(crate) id: u64,
    cancel: Option<CancelToken>,
    hits: Vec<(i32, PdfRect)>,
    current: usize,
    done: bool,
}

impl SearchState {
    pub fn start(&mut self, worker: &Worker) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        self.id += 1;
        self.hits.clear();
        self.current = 0;
        self.done = false;
        if self.text.trim().is_empty() {
            self.done = true;
            return;
        }
        let cancel = CancelToken::new();
        self.cancel = Some(cancel.clone());
        worker.send(Request::Search { id: self.id, needle: self.text.clone(), cancel });
    }

    pub fn on_hits(&mut self, id: u64, page: i32, rects: Vec<PdfRect>) {
        if id == self.id {
            self.hits.extend(rects.into_iter().map(|r| (page, r)));
        }
    }

    pub fn on_done(&mut self, id: u64) {
        if id == self.id {
            self.done = true;
        }
    }

    pub fn hits_on(&self, page: i32) -> impl Iterator<Item = (usize, &PdfRect)> {
        self.hits.iter().enumerate().filter(move |(_, h)| h.0 == page).map(|(i, h)| (i, &h.1))
    }

    pub fn current(&self) -> Option<(i32, PdfRect)> {
        self.hits.get(self.current).copied()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn step(&mut self, forward: bool) -> Option<(i32, PdfRect)> {
        let n = self.hits.len();
        if n == 0 {
            return None;
        }
        self.current = if forward { (self.current + 1) % n } else { (self.current + n - 1) % n };
        self.current()
    }

    pub fn summary(&self) -> String {
        match (self.hits.len(), self.done) {
            (0, true) if self.text.trim().is_empty() => String::new(),
            (0, true) => "Sem resultados".into(),
            (0, false) => "Buscando…".into(),
            (n, _) => format!("{} / {n}", self.current + 1),
        }
    }
}
```

- [ ] **Passo 3: Rodar os testes de lógica**

Em `main.rs` acrescente `mod search;`. Run: `cargo test -p gera-ui search 2>&1 | tail -n 10`
Esperado: `3 passed`.

- [ ] **Passo 4: Ligar na interface (`app.rs`)**

Campos novos em `GeraApp`: `pub(crate) search: SearchState` (inicialize com `SearchState::default()`). No `match` de `poll`, antes de `_ => {}`, acrescente:
```rust
Response::SearchHits { id, page, rects } => self.search.on_hits(id, page, rects),
Response::SearchDone { id } => self.search.on_done(id),
```
Na `toolbar`, depois do `ui.label(format!("Página ..."))`, acrescente (dentro do mesmo `ui.horizontal`, lembrando que `view` é um empréstimo mutável de `self.view`; mova a leitura de `self.search` para depois de usar `view`, ou clone o necessário):
```rust
ui.separator();
let edit = ui.add(egui::TextEdit::singleline(&mut self.search.text).hint_text("Buscar…").desired_width(160.0));
if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
    self.search.start(&self.worker);
    edit.request_focus();
}
ui.label(self.search.summary());
let prev = ui.button("▲").clicked();
let next = ui.button("▼").clicked();
let target = if prev {
    self.search.step(false)
} else if next {
    self.search.step(true)
} else {
    None
};
if let Some((page, rect)) = target {
    self.jump_to_page_y(page, rect.y0);
}
```
Novo método em `impl GeraApp`:
```rust
pub(crate) fn jump_to_page_y(&mut self, page: i32, y_in_page_pts: f32) {
    if let Some(view) = self.view.as_ref() {
        let i = (page.max(0) as usize).min(view.layout.page_count().saturating_sub(1));
        if view.layout.page_count() > 0 {
            self.jump_to = Some((view.layout.page_top(i) + y_in_page_pts * view.zoom - 80.0).max(0.0));
        }
    }
}
```
Para pular ao primeiro resultado, o braço `SearchHits` do `poll` fica assim (guarda a posição antes de mover `rects`):
```rust
Response::SearchHits { id, page, rects } => {
    let first = rects.first().map(|r| r.y0);
    let was_empty = self.search.summary() == "Buscando…";
    self.search.on_hits(id, page, rects);
    if let (true, Some(y)) = (was_empty, first) {
        self.jump_to_page_y(page, y);
    }
}
```
Atalho `Ctrl+F` em `logic()`: `if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) { self.focus_search = true; }`. Adicione o campo `focus_search: bool` ao `GeraApp` e, na `toolbar`, logo após criar `edit`: `if std::mem::take(&mut self.focus_search) { edit.request_focus(); }`.

Para destacar os resultados, passe a `on_page` do `canvas::show_pages` (em `app.rs::canvas`) um closure que desenha, para cada `(i, rect)` de `search.hits_on(page)`, um retângulo translúcido (amarelo, o atual em laranja) no espaço da página:
```rust
&mut |ui, page, page_rect| {
    for (n, r) in self.search.hits_on(page as i32) {
        let a = egui::pos2(page_rect.min.x + r.x0 * zoom, page_rect.min.y + r.y0 * zoom);
        let b = egui::pos2(page_rect.min.x + r.x1 * zoom, page_rect.min.y + r.y1 * zoom);
        let color = if n == self.search.current_index() {
            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 230, 0, 90)
        };
        ui.painter().rect_filled(egui::Rect::from_two_pos(a, b), 2.0, color);
    }
}
```
(`zoom` = `view.zoom` copiado para uma variável antes do closure, para não emprestar `view` duas vezes.)

- [ ] **Passo 5: Compilar e verificar manualmente**

Run: `cargo run -p gera-ui -- /tmp/gera-100.pdf`
Esperado: digitar `Hello` + Enter mostra "1 / 100"; ▼/▲ percorrem os resultados rolando até cada um; trocar o texto cancela a busca anterior; texto vazio + Enter limpa o contador.

- [ ] **Passo 6: `simplify`, clippy e commit**

```bash
cargo clippy -p gera-ui --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(gera-ui): busca de texto com destaque e navegação entre resultados" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 13: `gera-ui` — ferramentas de anotação, desfazer/refazer e salvar

**Arquivos:**
- Criar: `crates/gera-ui/src/edit.rs`
- Modificar: `crates/gera-ui/src/{main.rs,app.rs}`

**Interfaces:**
- Consome: `Tool`, `PendingAnnots`, `NewAnnot`, `PdfRect`, `Word`, `Request::{Words, Save}`, `Response::{Words, Saved, SaveFailed}`.
- Produz:
```rust
// edit.rs
#[derive(Default)] pub struct EditState { pub tool: Tool, pub pending: PendingAnnots, pub saving: bool, /* privado: stroke, drag, word_jobs, next_tag, note_draft */ }
impl EditState {
    pub fn on_page(&mut self, ui: &mut egui::Ui, worker: &Worker, page: i32, rect: egui::Rect, zoom: f32);   // mouse + overlays de uma página visível
    pub fn on_words(&mut self, ctx: &egui::Context, tag: u64, page: i32, words: Vec<Word>);
    pub fn note_window(&mut self, ui: &mut egui::Ui);                    // janelinha para digitar o texto da nota
    pub fn save(&mut self, worker: &Worker, dest: std::path::PathBuf);   // ignora se não houver nada pendente ou já estiver salvando
}
```

- [ ] **Passo 1: Escrever `edit.rs`**

```rust
use app_state::{annots::PendingAnnots, tools::Tool};
use pdf_core::{NewAnnot, PdfRect, Request, Word, Worker};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct EditState {
    pub tool: Tool,
    pub pending: PendingAnnots,
    pub saving: bool,
    stroke: Vec<(f32, f32)>,
    drag_start: Option<(f32, f32)>,
    word_jobs: HashMap<u64, Tool>,
    next_tag: u64,
    note_draft: Option<(i32, (f32, f32), String)>,
}

fn to_page(pos: egui::Pos2, origin: egui::Pos2, zoom: f32) -> (f32, f32) {
    ((pos.x - origin.x) / zoom, (pos.y - origin.y) / zoom)
}

fn to_screen(p: (f32, f32), origin: egui::Pos2, zoom: f32) -> egui::Pos2 {
    egui::pos2(origin.x + p.0 * zoom, origin.y + p.1 * zoom)
}

impl EditState {
    pub fn on_page(&mut self, ui: &mut egui::Ui, worker: &Worker, page: i32, rect: egui::Rect, zoom: f32) {
        let response = ui.interact(rect, ui.id().with(("page", page)), egui::Sense::click_and_drag());
        let pointer = response.interact_pointer_pos().map(|p| to_page(p, rect.min, zoom));
        match self.tool {
            Tool::Select | Tool::Highlight | Tool::Underline => {
                if response.drag_started() {
                    self.drag_start = pointer;
                }
                if let (Some(start), Some(now)) = (self.drag_start, pointer) {
                    let area = PdfRect::from_corners(start, now);
                    let painter = ui.painter();
                    let a = to_screen((area.x0, area.y0), rect.min, zoom);
                    let b = to_screen((area.x1, area.y1), rect.min, zoom);
                    painter.rect_stroke(
                        egui::Rect::from_two_pos(a, b),
                        0.0,
                        egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
                        egui::StrokeKind::Inside,
                    );
                    if response.drag_stopped() {
                        self.drag_start = None;
                        self.next_tag += 1;
                        self.word_jobs.insert(self.next_tag, self.tool);
                        worker.send(Request::Words { tag: self.next_tag, page, area });
                    }
                }
            }
            Tool::Note => {
                if response.clicked() {
                    if let Some(at) = pointer {
                        self.note_draft = Some((page, at, String::new()));
                    }
                }
            }
            Tool::Ink => {
                if response.drag_started() {
                    self.stroke.clear();
                }
                if response.dragged() {
                    if let Some(p) = pointer {
                        self.stroke.push(p);
                    }
                }
                if response.drag_stopped() {
                    let stroke = std::mem::take(&mut self.stroke);
                    self.pending.add(NewAnnot::Ink { page, strokes: vec![stroke] });
                }
                if self.stroke.len() > 1 {
                    let pts = self.stroke.iter().map(|&p| to_screen(p, rect.min, zoom)).collect();
                    ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.0, egui::Color32::RED)));
                }
            }
        }
        self.paint_pending(ui.painter(), page, rect.min, zoom);
    }

    fn paint_pending(&self, painter: &egui::Painter, page: i32, origin: egui::Pos2, zoom: f32) {
        let rect_of = |r: &PdfRect| {
            egui::Rect::from_two_pos(to_screen((r.x0, r.y0), origin, zoom), to_screen((r.x1, r.y1), origin, zoom))
        };
        for item in self.pending.items().iter().filter(|a| a.page() == page) {
            match item {
                NewAnnot::Highlight { rects, .. } => {
                    for r in rects {
                        painter.rect_filled(rect_of(r), 0.0, egui::Color32::from_rgba_unmultiplied(255, 235, 0, 110));
                    }
                }
                NewAnnot::Underline { rects, .. } => {
                    for r in rects {
                        let r = rect_of(r);
                        painter.line_segment([r.left_bottom(), r.right_bottom()], egui::Stroke::new(1.5, egui::Color32::BLUE));
                    }
                }
                NewAnnot::Note { at, .. } => {
                    let p = to_screen(*at, origin, zoom);
                    painter.rect_filled(
                        egui::Rect::from_min_size(p, egui::vec2(24.0 * zoom, 24.0 * zoom)),
                        3.0,
                        egui::Color32::from_rgb(255, 200, 0),
                    );
                }
                NewAnnot::Ink { strokes, .. } => {
                    for s in strokes {
                        let pts = s.iter().map(|&p| to_screen(p, origin, zoom)).collect();
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(2.0, egui::Color32::RED)));
                    }
                }
            }
        }
    }

    pub fn on_words(&mut self, ctx: &egui::Context, tag: u64, page: i32, words: Vec<Word>) {
        let Some(tool) = self.word_jobs.remove(&tag) else { return };
        if words.is_empty() {
            return;
        }
        match tool {
            Tool::Select => ctx.copy_text(words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")),
            Tool::Highlight => {
                self.pending.add(NewAnnot::Highlight { page, rects: words.into_iter().map(|w| w.rect).collect() });
            }
            Tool::Underline => {
                self.pending.add(NewAnnot::Underline { page, rects: words.into_iter().map(|w| w.rect).collect() });
            }
            Tool::Note | Tool::Ink => {}
        }
    }

    pub fn note_window(&mut self, ui: &mut egui::Ui) {
        let Some((page, at, mut text)) = self.note_draft.take() else { return };
        let mut keep = true;
        let mut confirmed = false;
        egui::Window::new("Nova nota").collapsible(false).show(ui, |ui| {
            ui.text_edit_multiline(&mut text);
            ui.horizontal(|ui| {
                confirmed = ui.button("Adicionar").clicked();
                keep = !ui.button("Cancelar").clicked() && !confirmed;
            });
        });
        if confirmed {
            self.pending.add(NewAnnot::Note { page, at, text });
        } else if keep {
            self.note_draft = Some((page, at, text));
        }
    }

    pub fn save(&mut self, worker: &Worker, dest: PathBuf) {
        if self.saving || !self.pending.is_dirty() {
            return;
        }
        self.saving = true;
        worker.send(Request::Save { pending: self.pending.items().to_vec(), dest });
    }
}
```

- [ ] **Passo 2: Ligar no `app.rs`**

`main.rs`: acrescente `mod edit;`. Em `GeraApp` acrescente o campo `pub(crate) edit: EditState` (`EditState::default()` no `new`). No `match` de `poll`, antes de `_ => {}`:
```rust
Response::Words { tag, page, words } => self.edit.on_words(ctx, tag, page, words),
Response::Saved { annots } => {
    self.edit.saving = false;
    self.edit.pending.clear();
    self.saved_annots = annots;
    if let Some(view) = self.view.as_mut() {
        view.clear_cache();
    }
    self.status = "Salvo.".into();
}
Response::SaveFailed(msg) => {
    self.edit.saving = false;
    self.status = format!("Não foi possível salvar: {msg}");
}
```
(adicione `pub(crate) saved_annots: Vec<pdf_core::AnnotInfo>` ao `GeraApp`, inicializado vazio, e preencha-o também em `on_opened` com `summary.annots`.)

Na `toolbar`, antes do zoom, acrescente os botões de ferramenta, desfazer/refazer e salvar:
```rust
for tool in Tool::ALL {
    ui.selectable_value(&mut self.edit.tool, tool, tool.label());
}
ui.separator();
if ui.add_enabled(self.edit.pending.is_dirty(), egui::Button::new("↶")).clicked() {
    self.edit.pending.undo();
}
if ui.button("↷").clicked() {
    self.edit.pending.redo();
}
let label = if self.edit.pending.is_dirty() { "Salvar •" } else { "Salvar" };
if ui.add_enabled(!self.edit.saving, egui::Button::new(label)).clicked() {
    if let Some(view) = self.view.as_ref() {
        self.edit.save(&self.worker, view.path.clone());
    }
}
```
(Importe `app_state::tools::Tool`. Como `view` é emprestado mutuamente no início da `toolbar`, reorganize a função: desenhe primeiro os botões que usam `self.edit`, depois obtenha `view`.)

Em `canvas()`, troque o closure vazio pelo que delega ao `EditState` e desenha as buscas (combine com o closure da Tarefa 12):
```rust
let zoom = view.zoom;
let (worker, edit, search) = (&self.worker, &mut self.edit, &self.search);
let frame = canvas::show_pages(ui, view, worker, self.jump_to.take(), &mut |ui, page, rect| {
    // destaque da busca (Tarefa 12), depois o mouse e as anotações pendentes:
    edit.on_page(ui, worker, page as i32, rect, zoom);
});
```
(Mantenha o desenho dos resultados da busca dentro do mesmo closure, usando `search`.) Depois do `CentralPanel`, em `ui()`, chame `self.edit.note_window(ui);`. Em `logic()`, trate atalhos: `Ctrl+Z` → `undo`, `Ctrl+Shift+Z` → `redo`, `Ctrl+S` → salvar:
```rust
let (undo, redo, save) = ctx.input_mut(|i| {
    (
        i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
        i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z),
        i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
    )
});
```
(Consuma `redo` antes de `undo` para que `Ctrl+Shift+Z` não dispare os dois.)

Mostre no status as alterações não salvas: quando `pending.is_dirty()`, `format!("{} alterações não salvas", n)`.

- [ ] **Passo 3: Compilar e verificar manualmente**

Run: `cargo run -p gera-ui -- /tmp/gera-100.pdf`
Esperado (sempre sobre uma **cópia** descartável do PDF de exemplo, nunca um documento seu):
1. Ferramenta "Destacar": arrastar sobre "Hello" cria um retângulo amarelo; "Sublinhar" cria linha azul; "Nota": clicar abre a janelinha, "Adicionar" cria o ícone; "Desenhar": arrastar desenha traço vermelho; "Selecionar": arrastar e colar (Ctrl+V em outro app) traz o texto.
2. `Ctrl+Z`/`Ctrl+Shift+Z` desfazem e refazem.
3. `Ctrl+S` grava; o botão volta para "Salvar"; reabrir o arquivo mostra as anotações (abra também em outro leitor, como o Okular, para confirmar que são anotações padrão).
4. Tentar salvar em local sem permissão mostra "Não foi possível salvar" e mantém as anotações pendentes.

- [ ] **Passo 4: `simplify`, clippy e commit**

```bash
cargo clippy -p gera-ui --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(gera-ui): ferramentas de anotação, desfazer/refazer e salvar" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 14: `gera-ui` — barra lateral, modo escuro e senha

**Arquivos:**
- Criar: `crates/gera-ui/src/sidebar.rs`
- Modificar: `crates/gera-ui/src/{main.rs,app.rs}`

**Interfaces:**
- Consome: `DocView::{texture_at, request_at, clear_cache}`, `OutlineItem`, `AnnotInfo`, `NewAnnot`, `Request::Open { password }`, `PdfError::PasswordRequired`.
- Produz:
```rust
// sidebar.rs
#[derive(Default, Clone, Copy, PartialEq, Eq)] pub enum SidebarTab { #[default] Thumbnails, Outline, Annotations }
pub enum SidebarAction { None, JumpToPage(usize) }
pub fn show(ui: &mut egui::Ui, tab: &mut SidebarTab, view: &mut DocView, worker: &Worker,
            outline: &[OutlineItem], saved: &[AnnotInfo], pending: &[NewAnnot]) -> SidebarAction;
```

- [ ] **Passo 1: Escrever `sidebar.rs`**

```rust
use crate::view::DocView;
use pdf_core::{AnnotInfo, AnnotKind, NewAnnot, OutlineItem, Worker};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Thumbnails,
    Outline,
    Annotations,
}

pub enum SidebarAction {
    None,
    JumpToPage(usize),
}

const THUMB_WIDTH: f32 = 110.0;

pub fn show(
    ui: &mut egui::Ui,
    tab: &mut SidebarTab,
    view: &mut DocView,
    worker: &Worker,
    outline: &[OutlineItem],
    saved: &[AnnotInfo],
    pending: &[NewAnnot],
) -> SidebarAction {
    ui.horizontal(|ui| {
        ui.selectable_value(tab, SidebarTab::Thumbnails, "Páginas");
        ui.selectable_value(tab, SidebarTab::Outline, "Índice");
        ui.selectable_value(tab, SidebarTab::Annotations, "Anotações");
    });
    ui.separator();
    match *tab {
        SidebarTab::Thumbnails => thumbnails(ui, view, worker),
        SidebarTab::Outline => outline_list(ui, outline),
        SidebarTab::Annotations => annotation_list(ui, saved, pending),
    }
}

fn thumbnails(ui: &mut egui::Ui, view: &mut DocView, worker: &Worker) -> SidebarAction {
    let first = view.sizes.first().copied();
    let Some(first) = first else { return SidebarAction::None };
    let thumb_h = THUMB_WIDTH * first.height / first.width.max(1.0);
    let row_h = thumb_h + 22.0;
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, view.sizes.len(), |ui, rows| {
        for i in rows {
            let scale = THUMB_WIDTH / view.sizes[i].width.max(1.0);
            view.request_at(worker, i as i32, scale);
            let (rect, response) = ui.allocate_exact_size(egui::vec2(THUMB_WIDTH, row_h), egui::Sense::click());
            let img = egui::Rect::from_min_size(rect.min, egui::vec2(THUMB_WIDTH, thumb_h));
            match view.texture_at(i as i32, scale) {
                Some(id) => {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(id, img, uv, egui::Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(img, 0.0, egui::Color32::from_gray(200));
                }
            }
            ui.painter().text(
                egui::pos2(rect.center().x, img.bottom() + 10.0),
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            if response.clicked() {
                action = SidebarAction::JumpToPage(i);
            }
        }
    });
    action
}

fn outline_list(ui: &mut egui::Ui, outline: &[OutlineItem]) -> SidebarAction {
    if outline.is_empty() {
        ui.label("Este documento não tem índice.");
        return SidebarAction::None;
    }
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for item in outline {
            ui.horizontal(|ui| {
                ui.add_space(12.0 * item.depth as f32);
                let enabled = item.page.is_some();
                if ui.add_enabled(enabled, egui::Button::new(&item.title).frame(false)).clicked() {
                    if let Some(p) = item.page {
                        action = SidebarAction::JumpToPage(p.max(0) as usize);
                    }
                }
            });
        }
    });
    action
}

fn kind_label(kind: AnnotKind) -> &'static str {
    match kind {
        AnnotKind::Highlight => "Destaque",
        AnnotKind::Underline => "Sublinhado",
        AnnotKind::Note => "Nota",
        AnnotKind::Ink => "Desenho",
    }
}

fn annotation_list(ui: &mut egui::Ui, saved: &[AnnotInfo], pending: &[NewAnnot]) -> SidebarAction {
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if saved.is_empty() && pending.is_empty() {
            ui.label("Nenhuma anotação.");
        }
        for a in saved {
            let text = if a.contents.is_empty() {
                format!("{} — p. {}", kind_label(a.kind), a.page + 1)
            } else {
                format!("{} — p. {}: {}", kind_label(a.kind), a.page + 1, a.contents)
            };
            if ui.button(text).clicked() {
                action = SidebarAction::JumpToPage(a.page.max(0) as usize);
            }
        }
        for a in pending {
            let (label, page) = match a {
                NewAnnot::Highlight { page, .. } => ("Destaque", *page),
                NewAnnot::Underline { page, .. } => ("Sublinhado", *page),
                NewAnnot::Note { page, .. } => ("Nota", *page),
                NewAnnot::Ink { page, .. } => ("Desenho", *page),
            };
            if ui.button(format!("{label} — p. {} (não salvo)", page + 1)).clicked() {
                action = SidebarAction::JumpToPage(page.max(0) as usize);
            }
        }
    });
    action
}
```

- [ ] **Passo 2: Ligar no `app.rs`**

`main.rs`: `mod sidebar;`. Em `GeraApp` acrescente `pub(crate) tab: SidebarTab`, `pub(crate) outline: Vec<OutlineItem>` (preenchido em `on_opened` com `summary.outline`), `pub(crate) show_sidebar: bool` (padrão `true`), `opening: Option<PathBuf>` e `password_prompt: Option<(PathBuf, String)>`.

Em `ui()`, antes do `CentralPanel`:
```rust
if self.show_sidebar && self.view.is_some() {
    let mut jump = None;
    egui::Panel::left("sidebar").default_size(150.0).show(ui, |ui| {
        if let Some(view) = self.view.as_mut() {
            let action = sidebar::show(ui, &mut self.tab, view, &self.worker, &self.outline,
                                       &self.saved_annots, self.edit.pending.items());
            if let sidebar::SidebarAction::JumpToPage(p) = action {
                jump = Some(p);
            }
        }
    });
    if let Some(p) = jump {
        self.jump_to_page_y(p as i32, 0.0);
    }
}
```
**Modo escuro:** na `toolbar`, um botão `🌙`/`☀`:
```rust
if ui.button(if view.dark { "☀" } else { "🌙" }).clicked() {
    view.dark = !view.dark;
    view.clear_cache();
    ui.ctx().set_visuals(if view.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
}
```
**Senha:** em `open()` guarde `self.opening = Some(path.clone())`. No `poll`, troque o braço de `OpenFailed`:
```rust
Response::OpenFailed(pdf_core::PdfError::PasswordRequired | pdf_core::PdfError::WrongPassword) => {
    if let Some(path) = self.opening.clone() {
        self.password_prompt = Some((path, String::new()));
    }
}
Response::OpenFailed(e) => self.status = format!("Erro: {e}"),
```
e em `ui()` desenhe a janelinha:
```rust
if let Some((path, mut pwd)) = self.password_prompt.take() {
    let mut submit = false;
    let mut cancel = false;
    egui::Window::new("PDF protegido por senha").collapsible(false).show(ui, |ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut pwd).password(true));
        submit = ui.button("Abrir").clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        cancel = ui.button("Cancelar").clicked();
    });
    if submit {
        self.worker.send(Request::Open { path, password: Some(pwd) });
    } else if !cancel {
        self.password_prompt = Some((path, pwd));
    }
}
```
(A senha nunca é registrada em log nem no status.)

- [ ] **Passo 3: Compilar e verificar manualmente**

Run: `cargo run -p gera-ui -- /tmp/gera-100.pdf`
Esperado: aba "Páginas" mostra miniaturas só das linhas visíveis e clicar leva à página; "Índice" lista "Capitulo N" e clicar pula; "Anotações" lista as pendentes e as salvas; `🌙` inverte as páginas e o tema; um PDF com senha (gere um com `qpdf --encrypt segredo segredo 256 -- entrada.pdf saida.pdf` a partir de uma **cópia** do exemplo, se tiver o `qpdf`) abre a janelinha de senha, rejeita senha errada e abre com a certa.

- [ ] **Passo 4: `simplify`, clippy e commit**

```bash
cargo clippy -p gera-ui --all-targets -- -D warnings 2>&1 | tail -n 5
git add -A && git commit -m "feat(gera-ui): barra lateral (miniaturas, índice, anotações), modo escuro e senha" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Tarefa 15: Medição das metas, empacotamento AppImage e verificação final (Spikes 2 e metas)

**Arquivos:**
- Criar: `scripts/measure.sh`, `packaging/{build-appimage.sh,gera-pdf-editor.desktop,gera-pdf-editor.svg}`, `THIRD_PARTY_NOTICES.md`

**Interfaces:**
- Consome: binário `gera-pdf-editor` com a flag `--bench-open`, `pdf-core` example `make_sample`.
- Produz: `scripts/measure.sh` (sai com erro se qualquer meta falhar) e `dist/Gera_PDF_Editor-x86_64.AppImage`.

- [ ] **Passo 1: Escrever `scripts/measure.sh`**

```bash
#!/usr/bin/env bash
# Mede as metas de leveza do spec. Precisa de uma sessão gráfica (abre uma janela por instantes).
set -euo pipefail
cd "$(dirname "$0")/.."

MAX_BIN=$((25 * 1024 * 1024))
MAX_MS=300
MAX_RSS_KB=$((150 * 1024))

cargo build --release -p gera-ui -q
BIN=target/release/gera-pdf-editor
SIZE=$(stat -c%s "$BIN")
echo "binario_bytes=$SIZE (limite $MAX_BIN)"

SAMPLE=$(mktemp --suffix=.pdf)
trap 'rm -f "$SAMPLE"' EXIT
cargo run --release -q -p pdf-core --example make_sample -- "$SAMPLE" 100

# a primeira execução aquece o cache de disco; a medida vale é a segunda
"$BIN" --bench-open "$SAMPLE" >/dev/null
OUT=$("$BIN" --bench-open "$SAMPLE" | grep '^BENCH')
echo "$OUT (limites: open_ms<$MAX_MS rss_kb<$MAX_RSS_KB)"
MS=$(sed -n 's/.*open_ms=\([0-9]*\).*/\1/p' <<<"$OUT")
RSS=$(sed -n 's/.*rss_kb=\([0-9]*\).*/\1/p' <<<"$OUT")

FAIL=0
[ "$SIZE" -lt "$MAX_BIN" ] || { echo "FALHA: binario acima de 25 MiB"; FAIL=1; }
[ "${MS:-99999}" -lt "$MAX_MS" ] || { echo "FALHA: abertura acima de ${MAX_MS} ms"; FAIL=1; }
[ "${RSS:-99999999}" -lt "$MAX_RSS_KB" ] || { echo "FALHA: memoria acima de 150 MB"; FAIL=1; }
exit $FAIL
```
```bash
chmod +x scripts/measure.sh
```

- [ ] **Passo 2: Rodar a medição**

Run: `scripts/measure.sh`
Esperado: as três linhas e saída 0. **Se o binário passar de 25 MiB (Spike 2):** em `crates/pdf-core/Cargo.toml` tire `system-fonts` das features do `mupdf` e meça de novo; se ainda passar, desligue mais recursos no `mupdf` (liste as features com `sed -n 11,90p ~/.cargo/registry/src/*/mupdf-0.8.0/Cargo.toml`) e confira com `cargo bloat --release -p gera-ui --crates` (instale com `cargo install cargo-bloat` se precisar). **Se a abertura passar de 300 ms:** o suspeito principal é o cálculo do tamanho de **todas** as páginas em `PdfDoc::summary`; troque por carregar só o tamanho da primeira página e as demais sob demanda (e ajuste `Layout`/`Opened` de acordo) antes de mexer em qualquer outra coisa. **Se a memória passar de 150 MB:** reduza `CACHE_BUDGET_BYTES` em `view.rs` e repita. Registre os números finais no commit.

- [ ] **Passo 3: Empacotamento AppImage**

`packaging/gera-pdf-editor.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=Gera PDF Editor
Comment=Visualizador leve de PDF com anotações
Exec=gera-pdf-editor %f
Icon=gera-pdf-editor
Terminal=false
Categories=Office;Viewer;
MimeType=application/pdf;
StartupWMClass=gera-pdf-editor
```
`packaging/gera-pdf-editor.svg`:
```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><rect x="40" y="16" width="176" height="224" rx="16" fill="#c0392b"/><text x="128" y="150" font-family="sans-serif" font-size="64" font-weight="bold" text-anchor="middle" fill="#fff">PDF</text></svg>
```
`packaging/build-appimage.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p gera-ui
APPDIR=target/AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin" dist
cp target/release/gera-pdf-editor "$APPDIR/usr/bin/"
cp packaging/gera-pdf-editor.desktop packaging/gera-pdf-editor.svg "$APPDIR/"
ln -s usr/bin/gera-pdf-editor "$APPDIR/AppRun"

TOOL=target/appimagetool-x86_64.AppImage
if [ ! -x "$TOOL" ]; then
  curl -sSfL -o "$TOOL" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$TOOL"
fi
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "$APPDIR" dist/Gera_PDF_Editor-x86_64.AppImage
ls -l dist/Gera_PDF_Editor-x86_64.AppImage
```
```bash
chmod +x packaging/build-appimage.sh
packaging/build-appimage.sh
```
Esperado: o arquivo `dist/Gera_PDF_Editor-x86_64.AppImage` criado. Teste: `./dist/Gera_PDF_Editor-x86_64.AppImage /tmp/gera-100.pdf` abre o PDF. Acrescente `dist/` ao `.gitignore`. O AppImage usa as bibliotecas do sistema (OpenGL, X11/Wayland, fontconfig); isso é esperado em qualquer desktop Linux.

- [ ] **Passo 4: Avisos de terceiros**

`THIRD_PARTY_NOTICES.md`:
```markdown
# Avisos de terceiros

- **MuPDF** (Artifex Software) — AGPL-3.0. Este projeto é distribuído sob a mesma licença; o código-fonte completo precisa acompanhar qualquer distribuição do binário/AppImage.
- **egui / eframe** (Emil Ernerfeldt e contribuidores) — MIT ou Apache-2.0.
- **thiserror** (David Tolnay) — MIT ou Apache-2.0.

As demais dependências transitivas e suas licenças são listadas por `cargo tree` e `Cargo.lock`.
```

- [ ] **Passo 5: Verificação final (suíte do usuário, proporcional ao risco: estrutural + salvar arquivos)**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -n 5
cargo test --workspace 2>&1 | tail -n 15
cargo build --release -p gera-ui 2>&1 | tail -n 3
(cargo audit 2>&1 | tail -n 10) || echo "cargo-audit não instalado (N/A); rode: cargo install cargo-audit"
git status --short && git diff --stat
grep -rIn --exclude-dir=target --exclude-dir=.git -E "BEGIN PRIVATE KEY|service_role|API_KEY" . || echo "sem segredos"
```
Esperado: fmt/clippy limpos, todos os testes passando, build release OK, nenhum vulnerável **crítico/alto** no `cargo audit` (se houver, avalie se afeta o projeto antes de atualizar), `git status` limpo (exceto o que for commitar), nenhum segredo. Use a skill `superpowers:verification-before-completion` antes de declarar a v1 concluída.

- [ ] **Passo 6: Commit**

```bash
git add -A && git commit -m "build: medição das metas, AppImage e avisos de terceiros" -m "Medido: binario=<bytes> open_ms=<n> rss_kb=<n>" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```
(Preencha os números reais.)

---

## Auto-revisão do plano

**Cobertura do spec:** abrir (arrastar, CLI, "abrir com" via argumento; uma janela por documento via novo processo) → T11; navegação (rolagem contínua, zoom largura/%, ir para página, índice, miniaturas) → T11, T12, T14 (girar a visualização ficou **fora**: não está nos critérios de sucesso e pode entrar depois); busca com destaque → T4, T12; seleção e cópia de texto → T4, T13; anotações (destacar, sublinhar, nota, desenho), salvar no PDF e listar na lateral → T5, T13, T14; modo escuro → T14; fluxo com worker/cache LRU/cancelamento/baixa resolução → T7, T9, T11; erros e senha → T6, T14; salvar seguro → T5; testes unitários/integração/robustez/desempenho → T2–T10, T15; empacotamento/licença/CI → T1, T15; spikes 1, 2, 3 → T5, T15, T11.

**Desvios do spec (declarados):** uma thread de worker em vez de "worker(s)" (o `Document` não é `Send`); a v1 não tem "girar a visualização" nem "abrir arquivo" por diálogo (evita dependência pesada de diálogo nativo; use arrastar, linha de comando ou "abrir com"); `app-state` depende de `pdf-core` apenas pelos tipos de dados.

**Placeholders:** nenhum "TBD". Os campos "<resultado>" nos commits são para preencher com a medição real, de propósito.

**Consistência de tipos:** `RenderKey`, `NewAnnot`, `PdfRect`, `Request`/`Response` e as assinaturas de `DocView`, `SearchState`, `EditState` e `sidebar::show` são as mesmas em todas as tarefas que as usam. Os trechos de UI das Tarefas 12–14 modificam `app.rs` por descrição e fragmentos; ao integrá-los, mantenha o arquivo compilando a cada tarefa.
