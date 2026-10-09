# Gera PDF Editor v1.1-B1 — Seleção de texto, impressão e propriedades — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** selecionar texto pelo fluxo (com copiar, selecionar tudo, menu de contexto e riscado), imprimir por diálogo próprio via CUPS e mostrar as propriedades do documento.

**Architecture:** o `pdf-core` ganha seleção de texto (MuPDF `highlight_selection`), texto do documento, riscado, propriedades e exportação para impressão; o worker expõe isso por mensagens. O `app-state` ganha a lógica pura da impressão (intervalos, opções → argumentos do `lp`) e das datas PDF. O `gera-ui` liga seleção, diálogo de impressão (com execução do `lp` em segundo plano) e janela de propriedades.

**Tech Stack:** Rust 2021, egui/eframe 0.36, mupdf 0.8, CUPS (`lp`/`lpstat`).

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1.1b1-texto-impressao-design.md`

## Global Constraints

- pt-BR em textos, comentários e commits; trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `simplify` **só na última tarefa**, sobre o diff inteiro da etapa (regra deste projeto).
- Testes de interface são feitos pelo executor em Xvfb `:99` + openbox + xdotool, sem `DBUS_SESSION_BUS_ADDRESS`, com `XDG_CONFIG_HOME` temporário (regra deste projeto).
- Ao fim de cada tarefa: `cargo fmt --all`, `cargo clippy --workspace --all-targets -q -- -D warnings`, `cargo test --workspace -q` limpos.
- Atalhos iguais aos do Acrobat: Ctrl+C copiar, Ctrl+A selecionar tudo, Ctrl+P imprimir, Ctrl+D propriedades.
- Ícones só com glifos que existem nas fontes do egui (conferir na captura E2E).
- O PDF temporário de impressão vai para `$XDG_RUNTIME_DIR/gera-pdf-editor/` (pasta 0700; cai para `std::env::temp_dir()` se a variável faltar) e é sempre apagado depois do `lp`.
- Metas: abertura < 300 ms (binário), memória < 150 MB, binário < 25 MB.

## Review Focus

1. **Seleção com a vista girada:** arrastar a 90° tem que marcar o texto que está sob o mouse (pontos convertidos por `PageMap::to_page`). Coberto no E2E da tarefa 9.
2. **PDF protegido por senha na impressão:** o CUPS não abre PDF criptografado; a cópia temporária sai sem criptografia (`export_for_print` usa a escrita padrão, sem `Encryption::Keep`) e fica numa pasta 0700. Teste `export_for_print_writes_a_readable_unencrypted_copy` (tarefa 4).
3. **Imprimir com anotações pendentes não pode gravá-las no documento aberto:** a exportação usa uma cópia aberta do disco; o documento em memória não muda. Teste `export_for_print_does_not_touch_the_open_document` (tarefa 4).
4. **Intervalos de páginas digitados de qualquer jeito** ("1 - 3 ,7", "0", "5-2", "abc", "99"): erro claro e Imprimir desativado. Testes da tarefa 1.
5. **Respostas de seleção atrasadas** (arrasto rápido) não podem sobrescrever a seleção mais nova: tag crescente, respostas antigas descartadas. Teste `stale_selection_responses_are_ignored` (tarefa 6).

---

### Tarefa 1: `app-state` — opções de impressão e argumentos do `lp`

**Files:** Create `crates/app-state/src/print.rs`; Modify `crates/app-state/src/lib.rs` (`pub mod print;`).

**Interfaces — Produces:**
- `pub enum Pages { All, Current, Ranges(String) }`, `pub enum Subset { All, Odd, Even }`, `pub enum Sizing { Fit, Actual, ShrinkLarge, Custom(u16) }`, `pub enum Duplex { None, LongEdge, ShortEdge }`, `pub enum Orientation { Auto, Portrait, Landscape }` (todas `Debug, Clone, PartialEq`, com `Default` = All/All/Fit/None/Auto e `label()` em pt-BR para as de UI).
- `pub struct PrintOptions { pub printer: String, pub copies: u16, pub grayscale: bool, pub pages: Pages, pub subset: Subset, pub reverse: bool, pub sizing: Sizing, pub per_sheet: u8, pub duplex: Duplex, pub orientation: Orientation, pub with_annots: bool }` com `Default` (copies 1, per_sheet 1, with_annots true).
- `pub fn parse_ranges(text: &str, total: usize) -> Result<Vec<(usize, usize)>, String>`
- `pub fn lp_args(o: &PrintOptions, current: usize, total: usize, oversized: bool, file: &Path) -> Result<Vec<String>, String>` (`current` 1-based).

- [ ] **Passo 1: testes que falham** (em `print.rs`, `#[cfg(test)] mod tests`):

```rust
use super::*;
use std::path::Path;

#[test]
fn ranges_accept_lists_with_spaces() {
    assert_eq!(parse_ranges("1-3, 7", 10), Ok(vec![(1, 3), (7, 7)]));
    assert_eq!(parse_ranges(" 2 - 4 ,9 ", 10), Ok(vec![(2, 4), (9, 9)]));
    assert_eq!(parse_ranges("5", 5), Ok(vec![(5, 5)]));
}

#[test]
fn bad_ranges_explain_the_problem() {
    for (text, total) in [("", 10), ("abc", 10), ("0", 10), ("5-2", 10), ("11", 10), ("1-", 10), ("1,,2", 10)] {
        let err = parse_ranges(text, total).unwrap_err();
        assert!(!err.is_empty(), "{text:?}");
    }
    assert!(parse_ranges("11", 10).unwrap_err().contains("10"), "diz quantas páginas há");
}

fn opts() -> PrintOptions {
    PrintOptions { printer: "HP".into(), ..Default::default() }
}

fn args(o: &PrintOptions) -> Vec<String> {
    lp_args(o, 4, 10, false, Path::new("/tmp/x.pdf")).unwrap()
}

#[test]
fn defaults_print_everything_fitted_on_the_chosen_printer() {
    assert_eq!(args(&opts()), ["-d", "HP", "-n", "1", "-o", "fit-to-page", "/tmp/x.pdf"]);
}

#[test]
fn each_option_maps_to_its_cups_argument() {
    let o = PrintOptions {
        copies: 3,
        grayscale: true,
        pages: Pages::Ranges("1-3, 7".into()),
        subset: Subset::Odd,
        reverse: true,
        sizing: Sizing::Custom(80),
        per_sheet: 4,
        duplex: Duplex::LongEdge,
        orientation: Orientation::Landscape,
        ..opts()
    };
    let a = args(&o).join(" ");
    for part in [
        "-n 3", "-o page-ranges=1-3,7", "-o page-set=odd", "-o outputorder=reverse",
        "-o scaling=80", "-o number-up=4", "-o sides=two-sided-long-edge",
        "-o orientation-requested=4", "-o print-color-mode=monochrome",
    ] {
        assert!(a.contains(part), "{part} em {a}");
    }
    assert!(!a.contains("fit-to-page"));
    assert!(a.ends_with("/tmp/x.pdf"));
}

#[test]
fn current_page_actual_size_and_shrink_large() {
    let cur = PrintOptions { pages: Pages::Current, sizing: Sizing::Actual, ..opts() };
    let a = args(&cur).join(" ");
    assert!(a.contains("-o page-ranges=4") && !a.contains("fit-to-page"), "{a}");
    let shrink = PrintOptions { sizing: Sizing::ShrinkLarge, ..opts() };
    assert!(!args(&shrink).join(" ").contains("fit-to-page"), "nada a reduzir");
    let big = lp_args(&shrink, 1, 10, true, Path::new("/tmp/x.pdf")).unwrap().join(" ");
    assert!(big.contains("fit-to-page"), "página maior que o papel é reduzida");
}

#[test]
fn invalid_ranges_or_missing_printer_are_errors() {
    let bad = PrintOptions { pages: Pages::Ranges("0".into()), ..opts() };
    assert!(lp_args(&bad, 1, 10, false, Path::new("/tmp/x.pdf")).is_err());
    let none = PrintOptions { printer: String::new(), ..Default::default() };
    assert!(lp_args(&none, 1, 10, false, Path::new("/tmp/x.pdf")).is_err());
}
```

- [ ] **Passo 2:** `cargo test -p app-state print -q` → falha de compilação.
- [ ] **Passo 3: implementar** (acima dos testes):

```rust
//! Impressão: opções do diálogo (no estilo do Acrobat) e os argumentos do `lp` (CUPS).
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Pages { #[default] All, Current, Ranges(String) }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Subset { #[default] All, Odd, Even }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sizing { #[default] Fit, Actual, ShrinkLarge, Custom(u16) }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Duplex { #[default] None, LongEdge, ShortEdge }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation { #[default] Auto, Portrait, Landscape }

impl Subset { pub const ALL: [Self; 3] = [Self::All, Self::Odd, Self::Even];
    pub fn label(self) -> &'static str { match self { Self::All => "Todas as páginas do intervalo", Self::Odd => "Só ímpares", Self::Even => "Só pares" } } }
impl Duplex { pub const ALL: [Self; 3] = [Self::None, Self::LongEdge, Self::ShortEdge];
    pub fn label(self) -> &'static str { match self { Self::None => "Só frente", Self::LongEdge => "Frente e verso (borda longa)", Self::ShortEdge => "Frente e verso (borda curta)" } } }
impl Orientation { pub const ALL: [Self; 3] = [Self::Auto, Self::Portrait, Self::Landscape];
    pub fn label(self) -> &'static str { match self { Self::Auto => "Automática", Self::Portrait => "Retrato", Self::Landscape => "Paisagem" } } }

#[derive(Debug, Clone, PartialEq)]
pub struct PrintOptions {
    pub printer: String,
    pub copies: u16,
    pub grayscale: bool,
    pub pages: Pages,
    pub subset: Subset,
    pub reverse: bool,
    pub sizing: Sizing,
    /// Páginas por folha (1, 2, 4, 6, 9).
    pub per_sheet: u8,
    pub duplex: Duplex,
    pub orientation: Orientation,
    /// "Documento e marcações" (true) ou só "Documento".
    pub with_annots: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self { printer: String::new(), copies: 1, grayscale: false, pages: Pages::All, subset: Subset::All,
               reverse: false, sizing: Sizing::Fit, per_sheet: 1, duplex: Duplex::None,
               orientation: Orientation::Auto, with_annots: true }
    }
}

const RANGE_HINT: &str = "Use números de página, como 1-3, 7";

/// "1-3, 7" → [(1, 3), (7, 7)], com páginas de 1 a `total`.
pub fn parse_ranges(text: &str, total: usize) -> Result<Vec<(usize, usize)>, String> {
    let num = |s: &str| s.trim().parse::<usize>().map_err(|_| RANGE_HINT.to_string());
    let mut out = Vec::new();
    for part in text.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => { let n = num(part)?; (n, n) }
        };
        if a == 0 { return Err("A primeira página é a 1.".into()); }
        if a > b { return Err(format!("Intervalo invertido: {a}-{b}.")); }
        if b > total { return Err(format!("O documento tem {total} páginas.")); }
        out.push((a, b));
    }
    Ok(out)
}

/// Argumentos do `lp` para imprimir `file` com as opções `o`. `current` é a página atual (1-based);
/// `oversized` diz se alguma página é maior que o papel (para "Reduzir páginas grandes").
pub fn lp_args(o: &PrintOptions, current: usize, total: usize, oversized: bool, file: &Path) -> Result<Vec<String>, String> {
    if o.printer.is_empty() { return Err("Nenhuma impressora escolhida.".into()); }
    let mut a: Vec<String> = vec!["-d".into(), o.printer.clone(), "-n".into(), o.copies.max(1).to_string()];
    let mut opt = |v: String| { a.push("-o".into()); a.push(v); };
    match &o.pages {
        Pages::All => {}
        Pages::Current => opt(format!("page-ranges={current}")),
        Pages::Ranges(text) => {
            let list = parse_ranges(text, total)?
                .iter()
                .map(|&(x, y)| if x == y { x.to_string() } else { format!("{x}-{y}") })
                .collect::<Vec<_>>()
                .join(",");
            opt(format!("page-ranges={list}"));
        }
    }
    match o.subset { Subset::All => {}, Subset::Odd => opt("page-set=odd".into()), Subset::Even => opt("page-set=even".into()) }
    if o.reverse { opt("outputorder=reverse".into()); }
    match o.sizing {
        Sizing::Fit => opt("fit-to-page".into()),
        Sizing::ShrinkLarge if oversized => opt("fit-to-page".into()),
        Sizing::Custom(pct) => opt(format!("scaling={pct}")),
        Sizing::Actual | Sizing::ShrinkLarge => {}
    }
    if o.per_sheet > 1 { opt(format!("number-up={}", o.per_sheet)); }
    match o.duplex { Duplex::None => {}, Duplex::LongEdge => opt("sides=two-sided-long-edge".into()), Duplex::ShortEdge => opt("sides=two-sided-short-edge".into()) }
    match o.orientation { Orientation::Auto => {}, Orientation::Portrait => opt("orientation-requested=3".into()), Orientation::Landscape => opt("orientation-requested=4".into()) }
    if o.grayscale { opt("print-color-mode=monochrome".into()); }
    a.push(file.display().to_string());
    Ok(a)
}
```

(Formatar com `cargo fmt`; o borrow de `a` no closure `opt` pode exigir trocar o closure por uma função local `fn push_opt(a: &mut Vec<String>, v: String)` — use a forma que compilar, sem mudar o resultado.)

- [ ] **Passo 4:** `cargo test -p app-state print -q` → 6 testes passam.
- [ ] **Passo 5:** commit `feat(app-state): opções de impressão e argumentos do lp`.

---

### Tarefa 2: `app-state` — datas do PDF

**Files:** Create `crates/app-state/src/pdf_date.rs`; `lib.rs` (`pub mod pdf_date;`).

**Produces:** `pub fn format_pdf_date(raw: &str) -> String`.

- [ ] **Passo 1: testes**

```rust
use super::*;

#[test]
fn full_dates_become_brazilian_format() {
    assert_eq!(format_pdf_date("D:20240131143005-03'00'"), "31/01/2024 14:30");
    assert_eq!(format_pdf_date("D:20240131143005Z"), "31/01/2024 14:30");
    assert_eq!(format_pdf_date("20240131143005"), "31/01/2024 14:30");
}

#[test]
fn partial_dates_show_what_exists() {
    assert_eq!(format_pdf_date("D:20240131"), "31/01/2024");
    assert_eq!(format_pdf_date("D:2024"), "2024");
}

#[test]
fn missing_or_invalid_dates() {
    assert_eq!(format_pdf_date(""), "—");
    assert_eq!(format_pdf_date("ontem"), "ontem");
    assert_eq!(format_pdf_date("D:20241399"), "D:20241399", "mês 13 é inválido");
}
```

- [ ] **Passo 2:** ver falhar.
- [ ] **Passo 3: implementar**

```rust
//! Datas do dicionário Info do PDF (`D:AAAAMMDDhhmmss±hh'mm'`) em formato brasileiro.

/// "D:20240131143005-03'00'" → "31/01/2024 14:30"; parcial mostra o que existe; vazio → "—";
/// qualquer coisa que não seja uma data PDF válida volta como veio.
pub fn format_pdf_date(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() { return "—".into(); }
    let digits: String = s.strip_prefix("D:").unwrap_or(s).chars().take_while(char::is_ascii_digit).collect();
    let part = |r: std::ops::Range<usize>| digits.get(r).and_then(|v| v.parse::<u32>().ok());
    let (Some(year), month, day) = (part(0..4), part(4..6), part(6..8)) else { return s.into() };
    match (month, day) {
        (None, _) => year.to_string(),
        (Some(m), Some(d)) if (1..=12).contains(&m) && (1..=31).contains(&d) => {
            match (part(8..10), part(10..12)) {
                (Some(h), Some(min)) if h < 24 && min < 60 => format!("{d:02}/{m:02}/{year} {h:02}:{min:02}"),
                _ => format!("{d:02}/{m:02}/{year}"),
            }
        }
        _ => s.into(),
    }
}
```

- [ ] **Passo 4:** testes passam. **Passo 5:** commit `feat(app-state): datas do PDF em formato brasileiro`.

---

### Tarefa 3: `pdf-core` — seleção de texto e texto do documento

**Files:** `crates/pdf-core/src/types.rs` (struct), `crates/pdf-core/src/doc.rs`; Test `crates/pdf-core/tests/select.rs`.

**Produces:** `pub struct TextSelection { pub quads: Vec<PdfRect>, pub text: String }` (Debug, Clone, PartialEq, Default); `PdfDoc::select_text(&self, page: i32, a: (f32, f32), b: (f32, f32)) -> Result<TextSelection>`; `PdfDoc::document_text(&self) -> Result<String>`.

- [ ] **Passo 1: testes** (`tests/select.rs`):

```rust
mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfRect};

fn doc(pages: usize) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&write_sample(&dir, "a.pdf", pages, 0), None).unwrap();
    (dir, d)
}

/// Retângulo de uma palavra da página 0 (para mirar a seleção).
fn word(d: &PdfDoc, text: &str) -> PdfRect {
    d.words_in_rect(0, PdfRect::new(0.0, 0.0, 10_000.0, 10_000.0)).unwrap()
        .into_iter().find(|w| w.text == text).unwrap().rect
}

#[test]
fn selecting_from_first_to_last_word_takes_the_whole_line() {
    let (_d, d) = doc(1);
    let (a, b) = (word(&d, "Page"), word(&d, "Gera"));
    let sel = d.select_text(0, (a.x0 + 1.0, (a.y0 + a.y1) / 2.0), (b.x1 - 1.0, (b.y0 + b.y1) / 2.0)).unwrap();
    assert_eq!(sel.text.trim(), "Page 1 Hello Gera");
    assert!(!sel.quads.is_empty());
}

#[test]
fn selection_works_backwards_and_partially() {
    let (_d, d) = doc(1);
    let (h, g) = (word(&d, "Hello"), word(&d, "Gera"));
    let mid = |r: PdfRect| (r.x0 + 1.0, (r.y0 + r.y1) / 2.0);
    let fwd = d.select_text(0, mid(h), (g.x1 - 1.0, (g.y0 + g.y1) / 2.0)).unwrap();
    let back = d.select_text(0, (g.x1 - 1.0, (g.y0 + g.y1) / 2.0), mid(h)).unwrap();
    assert_eq!(fwd.text.trim(), "Hello Gera");
    assert_eq!(back.text, fwd.text, "a direção do arrasto não importa");
}

#[test]
fn empty_area_selects_nothing_and_bad_pages_are_errors() {
    let (_d, d) = doc(1);
    let sel = d.select_text(0, (5.0, 700.0), (5.0, 700.0)).unwrap();
    assert!(sel.text.trim().is_empty());
    assert!(d.select_text(3, (0.0, 0.0), (1.0, 1.0)).is_err());
}

#[test]
fn document_text_has_every_page_in_order() {
    let (_d, d) = doc(3);
    let text = d.document_text().unwrap();
    let (p1, p3) = (text.find("Page 1").unwrap(), text.find("Page 3").unwrap());
    assert!(p1 < p3);
}
```

- [ ] **Passo 2:** ver falhar.
- [ ] **Passo 3: implementar.** Em `types.rs`:

```rust
/// Texto escolhido numa página: as áreas marcadas (em ordem de leitura) e o texto delas.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextSelection {
    pub quads: Vec<PdfRect>,
    pub text: String,
}
```

Em `doc.rs` (imports: `mupdf::{TextPageFlags}`, `TextSelection`):

```rust
/// Máximo de áreas numa seleção (linhas longas com muitos pedaços).
const MAX_SELECTION_QUADS: usize = 4096;

impl PdfDoc {
    /// Seleção pelo fluxo do texto entre os pontos `a` e `b` (coordenadas da página), como o
    /// Acrobat: linhas inteiras entre a linha de `a` e a de `b`.
    pub fn select_text(&self, page: i32, a: (f32, f32), b: (f32, f32)) -> Result<TextSelection> {
        let page = self.load(page)?;
        let mut tp = page.to_text_page(TextPageFlags::empty())?;
        let mut buf = vec![Rect::new(0.0, 0.0, 0.0, 0.0).quad(); MAX_SELECTION_QUADS];
        let n = tp.highlight_selection(Point::new(a.0, a.1), Point::new(b.0, b.1), &mut buf)?;
        let quads: Vec<PdfRect> = buf[..(n.max(0) as usize).min(buf.len())]
            .iter()
            .map(|q| Rect::from(*q).into())
            .collect();
        let inside = |x: f32, y: f32| quads.iter().any(|r| x >= r.x0 - 0.5 && x <= r.x1 + 0.5 && y >= r.y0 - 0.5 && y <= r.y1 + 0.5);
        let mut text = String::new();
        for block in tp.blocks() {
            for line in block.lines() {
                let before = text.len();
                for ch in line.chars() {
                    let r = Rect::from(ch.quad());
                    if let (Some(c), true) = (ch.char(), inside((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)) {
                        text.push(c);
                    }
                }
                if text.len() > before { text.push('\n'); }
            }
        }
        Ok(TextSelection { quads, text })
    }

    /// Texto do documento inteiro (Selecionar tudo), página após página.
    pub fn document_text(&self) -> Result<String> {
        let mut out = String::new();
        for p in 0..self.page_count()? {
            out.push_str(&self.load(p)?.to_text_page(TextPageFlags::empty())?.to_text()?);
            out.push('\n');
        }
        Ok(out)
    }
}
```

(`highlight_selection` recebe a fatia de saída; se a assinatura da 0.8 for `&[Quad]`, passe `&buf` — ela escreve no buffer pela FFI. Ajuste só o que o compilador pedir.)

- [ ] **Passo 4:** `cargo test -p pdf-core --test select -q` → 4 passam.
- [ ] **Passo 5:** commit `feat(pdf-core): seleção de texto pelo fluxo e texto do documento`.

---

### Tarefa 4: `pdf-core` — riscado, propriedades e exportação para impressão

**Files:** `types.rs`, `doc.rs`; Tests `tests/annotations.rs` (riscado), `tests/properties.rs`, `tests/print_export.rs`.

**Produces:**
- `NewAnnot::StrikeOut { page, rects }`, `AnnotKind::StrikeOut`.
- `pub struct DocProperties { pub path: PathBuf, pub title: String, pub author: String, pub subject: String, pub keywords: String, pub creator: String, pub producer: String, pub created: String, pub modified: String, pub format: String, pub encrypted: bool, pub pages: i32, pub first_page: PageSize, pub file_size: u64 }` (Debug, Clone, PartialEq; datas cruas, a UI formata).
- `PdfDoc::properties(&self) -> Result<DocProperties>`
- `PdfDoc::export_for_print(&self, pending: &[NewAnnot], dest: &Path, with_annots: bool) -> Result<()>`

- [ ] **Passo 1: testes**

`tests/annotations.rs` (acrescentar):

```rust
#[test]
fn strikeout_is_saved_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "r.pdf", 1, 0);
    let mut doc = PdfDoc::open(&path, None).unwrap();
    let r = PdfRect::new(70.0, 90.0, 200.0, 110.0);
    doc.save_with(&[NewAnnot::StrikeOut { page: 0, rects: vec![r] }], &path).unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    assert_eq!(again.annotations(0).unwrap()[0].kind, AnnotKind::StrikeOut);
}
```

`tests/properties.rs`:

```rust
mod common;
use common::write_sample;
use pdf_core::PdfDoc;

#[test]
fn properties_describe_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "p.pdf", 3, 0);
    let p = PdfDoc::open(&path, None).unwrap().properties().unwrap();
    assert_eq!(p.pages, 3);
    assert_eq!((p.first_page.width, p.first_page.height), (612.0, 792.0));
    assert!(p.format.starts_with("PDF"), "{}", p.format);
    assert!(!p.encrypted);
    assert_eq!(p.file_size, std::fs::metadata(&path).unwrap().len());
    assert_eq!(p.path, path);
}
```

`tests/print_export.rs`:

```rust
mod common;
use common::write_sample;
use pdf_core::{NewAnnot, PdfDoc, PdfRect};

fn note() -> NewAnnot {
    NewAnnot::Note { page: 0, at: (50.0, 50.0), text: "pendente".into() }
}

#[test]
fn export_for_print_includes_pending_annotations_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    let dest = dir.path().join("imprimir.pdf");
    doc.export_for_print(&[note()], &dest, true).unwrap();
    assert_eq!(PdfDoc::open(&dest, None).unwrap().annotations(0).unwrap().len(), 1);
    doc.export_for_print(&[note()], &dest, false).unwrap();
    assert!(PdfDoc::open(&dest, None).unwrap().annotations(0).unwrap().is_empty(), "só o documento");
}

#[test]
fn export_for_print_does_not_touch_the_open_document() {
    let dir = tempfile::tempdir().unwrap();
    let doc = PdfDoc::open(&write_sample(&dir, "a.pdf", 1, 0), None).unwrap();
    doc.export_for_print(&[note()], &dir.path().join("x.pdf"), true).unwrap();
    assert!(doc.annotations(0).unwrap().is_empty(), "a pendente não entra no documento aberto");
}

#[test]
fn export_for_print_writes_a_readable_unencrypted_copy() {
    // reaproveita o PDF protegido dos testes de senha, se o helper existir; senão, cria um com qpdf
    let dir = tempfile::tempdir().unwrap();
    let Some(locked) = common::encrypted_sample(&dir, "segredo") else { return };
    let doc = PdfDoc::open(&locked, Some("segredo")).unwrap();
    let dest = dir.path().join("claro.pdf");
    doc.export_for_print(&[], &dest, true).unwrap();
    assert!(PdfDoc::open(&dest, None).is_ok(), "abre sem senha (o CUPS precisa ler)");
}
```

(Se `tests/common` ainda não tiver um helper de PDF protegido, crie `pub fn encrypted_sample(dir, password) -> Option<PathBuf>` reaproveitando o que os testes de senha da v1 usam — `tests/open.rs`; se eles usam `qpdf`, devolva `None` quando o `qpdf` não existir.)

- [ ] **Passo 2:** ver falhar.
- [ ] **Passo 3: implementar.**
  - `types.rs`: variante `StrikeOut { page: i32, rects: Vec<PdfRect> }` em `NewAnnot` (cobrir em `page()` e `is_empty()` junto de Highlight/Underline); `AnnotKind::StrikeOut`; struct `DocProperties`.
  - `doc.rs` `add_one`: `NewAnnot::StrikeOut { rects, .. } => { page.add_strikeout_annotation(quads(rects)?)?; }`; `kind_of`: `PdfAnnotationType::StrikeOut => Some(AnnotKind::StrikeOut)`.
  - `properties`:

```rust
    pub fn properties(&self) -> Result<DocProperties> {
        let meta = |name| self.inner.metadata(name).unwrap_or_default();
        Ok(DocProperties {
            path: self.path.clone(),
            title: meta(MetadataName::Title),
            author: meta(MetadataName::Author),
            subject: meta(MetadataName::Subject),
            keywords: meta(MetadataName::Keywords),
            creator: meta(MetadataName::Creator),
            producer: meta(MetadataName::Producer),
            created: meta(MetadataName::CreationDate),
            modified: meta(MetadataName::ModDate),
            format: meta(MetadataName::Format),
            encrypted: self.password.is_some() || !meta(MetadataName::Encryption).starts_with("None"),
            pages: self.page_count()?,
            first_page: self.page_size(0)?,
            file_size: std::fs::metadata(&self.path).map_or(0, |m| m.len()),
        })
    }
```

  - `export_for_print` (abre **outra** instância do arquivo, aplica ou remove, grava sem criptografia):

```rust
    /// Cópia para imprimir: o arquivo do disco + as pendentes (ou sem nenhuma anotação),
    /// gravada sem criptografia (o CUPS precisa ler). O documento aberto não muda.
    pub fn export_for_print(&self, pending: &[NewAnnot], dest: &Path, with_annots: bool) -> Result<()> {
        let mut copy = open_authenticated(&self.path, self.password.as_deref())?;
        for p in 0..copy.page_count()? {
            let mut page = copy.load_pdf_page(p)?;
            if with_annots {
                for a in pending.iter().filter(|a| a.page() == p) {
                    add_one(&mut page, a)?;
                }
            } else {
                let all: Vec<_> = page.annotations().collect();
                for a in all {
                    page.delete_annotation(a)?;
                }
            }
            page.update()?;
        }
        let dest_str = dest.to_str().ok_or_else(|| PdfError::Engine("caminho não é UTF-8 válido".into()))?;
        copy.save_with_options(dest_str, PdfWriteOptions::default())?;
        Ok(())
    }
```

(`open_authenticated` é o helper que `save_with` já usa para reabrir; `PdfWriteOptions::default()` grava sem criptografia. Se `annotations()` devolver referências e não donos, use o que `delete_annotation` aceitar — ex.: recolher num `Vec` antes.)

  - `gera-ui`: `sidebar.rs` (`kind_label`/`pending_label` → "Riscado") e `edit.rs::paint_pending` (StrikeOut: linha horizontal no meio de cada retângulo, vermelho) — para compilar.

- [ ] **Passo 4:** `cargo test --workspace -q` → tudo passa.
- [ ] **Passo 5:** commit `feat(pdf-core): riscado, propriedades e exportação para impressão`.

---

### Tarefa 5: `pdf-core` — mensagens do worker

**Files:** `crates/pdf-core/src/worker.rs`; Test `crates/pdf-core/tests/worker.rs`.

**Produces:**
- `Request::SelectText { tag: u64, page: i32, a: (f32, f32), b: (f32, f32) }` → `Response::Selection { tag: u64, page: i32, selection: TextSelection }`
- `Request::DocumentText { tag: u64 }` → `Response::DocumentText { tag: u64, text: String }`
- `Request::Properties` → `Response::Properties(DocProperties)` (erro: `Response::PropertiesFailed(String)`)
- `Request::ExportForPrint { pending: Vec<NewAnnot>, dest: PathBuf, with_annots: bool }` → `Response::Exported { dest: PathBuf }` / `Response::ExportFailed(String)`

- [ ] **Passo 1: teste** (acrescentar a `tests/worker.rs`, usando os helpers `opened`/`recv` existentes):

```rust
#[test]
fn selection_text_and_properties_through_the_worker() {
    let (_d, w, _) = opened(2);
    w.send(Request::SelectText { tag: 7, page: 0, a: (0.0, 0.0), b: (10_000.0, 10_000.0) });
    match recv(&w) {
        Response::Selection { tag, page, selection } => {
            assert_eq!((tag, page), (7, 0));
            assert!(selection.text.contains("Hello"));
        }
        other => panic!("{other:?}"),
    }
    w.send(Request::Properties);
    match recv(&w) {
        Response::Properties(p) => assert_eq!(p.pages, 2),
        other => panic!("{other:?}"),
    }
}
```

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar os braços em `State::handle` (sem documento aberto: `Selection` vazia, `DocumentText` vazio, `PropertiesFailed`/`ExportFailed("nenhum documento aberto")`). **Passo 4:** testes passam. **Passo 5:** commit `feat(pdf-core): seleção, texto, propriedades e exportação pelo worker`.

---

### Tarefa 6: `gera-ui` — seleção de texto como no Acrobat

**Files:** `crates/gera-ui/src/edit.rs`, `doc_tab.rs`, `shell/commands.rs`, `shell/menus.rs`.

**Consumes:** `Request::SelectText`/`DocumentText`, `Response::Selection`/`DocumentText` (tarefa 5); `NewAnnot::StrikeOut` (tarefa 4).

**Produces:**
- Em `EditState`: `pub selection: Option<Selection>` com `pub struct Selection { pub page: Option<i32> /* None = documento inteiro */, pub quads: Vec<PdfRect>, pub text: String }`; `fn on_selection(&mut self, tag, page, sel: TextSelection)`; `fn on_document_text(&mut self, tag, text)`; `pub fn copy(&self, ctx)`; `pub fn select_all(&mut self, worker)`; `fn mark_selection(&mut self, kind: Markup)` com `enum Markup { Highlight, Underline, StrikeOut }`.
- `DocCommand::Copy`, `DocCommand::SelectAll`; atalhos Ctrl+C e Ctrl+A (na lista de atalhos que pertencem a campos de texto quando o usuário digita); Editar → Copiar e Selecionar tudo ativos.

Comportamento:
1. **Arrastar** com Selecionar/Destacar/Sublinhar: no início guarda `(page, a)`; a cada quadro em que o ponteiro andou ≥ 1 pt, envia `SelectText { tag: ++tag, page, a, b }` e guarda `latest_tag`; respostas com `tag < latest_tag` são ignoradas.
2. Ao soltar com **Destacar/Sublinhar**: marca `finish_tag = latest_tag` e a ferramenta; quando a resposta dessa tag chega, cria a anotação com `selection.quads` e limpa a seleção.
3. **Clique simples** (sem arrastar) em Selecionar limpa a seleção.
4. **Pintura:** quads da seleção desta página em azul translúcido (`Color32::from_rgba_unmultiplied(60, 120, 255, 80)`), via `screen_rect(r, map)`.
5. **Menu de contexto** (`response.context_menu`) quando há seleção nesta página: Copiar · Destacar texto · Sublinhar texto · Riscar texto.
6. **Ctrl+C** copia `selection.text` (`ctx.copy_text`); **Ctrl+A** envia `DocumentText` e guarda a seleção com `page: None`.
7. Ao trocar de documento/abrir, `reset()` limpa a seleção.
8. Remover do `EditState` o caminho antigo de `Request::Words` (`word_jobs`, `on_words`, `annot_from_words`, `selection_text`) e seus testes, já que a seleção pelo fluxo os substitui; manter `words_in_rect` no `pdf-core` (usado em testes de alvo).

- [ ] **Passo 1: testes** (em `edit.rs`):

```rust
#[test]
fn stale_selection_responses_are_ignored() {
    let mut e = EditState::default();
    e.latest_tag = 5;
    e.on_selection(4, 0, TextSelection { quads: vec![], text: "velha".into() });
    assert!(e.selection.is_none());
    e.on_selection(5, 0, TextSelection { quads: vec![PdfRect::new(0.0, 0.0, 1.0, 1.0)], text: "nova".into() });
    assert_eq!(e.selection.as_ref().map(|s| s.text.as_str()), Some("nova"));
}

#[test]
fn finishing_a_highlight_drag_turns_the_selection_into_an_annotation() {
    let mut e = EditState::default();
    e.tool = Tool::Highlight;
    e.latest_tag = 3;
    e.finish_tag = Some((3, Markup::Highlight));
    e.on_selection(3, 2, TextSelection { quads: vec![PdfRect::new(1.0, 1.0, 5.0, 5.0)], text: "x".into() });
    assert!(matches!(e.pending.items(), [NewAnnot::Highlight { page: 2, .. }]));
    assert!(e.selection.is_none());
}

#[test]
fn context_menu_marks_the_current_selection() {
    let mut e = EditState::default();
    e.selection = Some(Selection { page: Some(1), quads: vec![PdfRect::new(1.0, 1.0, 5.0, 5.0)], text: "x".into() });
    e.mark_selection(Markup::StrikeOut);
    assert!(matches!(e.pending.items(), [NewAnnot::StrikeOut { page: 1, .. }]));
}

#[test]
fn whole_document_selection_cannot_be_marked() {
    let mut e = EditState::default();
    e.selection = Some(Selection { page: None, quads: vec![], text: "tudo".into() });
    e.mark_selection(Markup::Highlight);
    assert!(e.pending.items().is_empty());
}
```

(Os campos `latest_tag` e `finish_tag` ficam `pub(crate)` para os testes.)

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar conforme o comportamento acima; em `doc_tab.rs::poll`, trocar `Response::Words` por `Response::Selection` → `self.edit.on_selection(...)` e `Response::DocumentText` → `self.edit.on_document_text(...)`; `DocTab::run` trata `Copy` (precisa do `ctx`: guarde `copy_requested` e faça a cópia no `show_canvas`, que tem `ui.ctx()`) e `SelectAll`. **Passo 4:** testes passam. **Passo 5:** commit `feat(gera-ui): seleção de texto pelo fluxo, copiar, selecionar tudo e menu de contexto`.

---

### Tarefa 7: `gera-ui` — Propriedades (Ctrl+D)

**Files:** Create `crates/gera-ui/src/shell/properties.rs`; Modify `doc_tab.rs`, `app.rs`, `shell/commands.rs`, `shell/menus.rs`, `shell/mod.rs`.

**Produces:** `Command::Properties` (Ctrl+D, menu Arquivo → Propriedades ativo); `DocTab::request_properties()`; `DocTab.properties: Option<DocProperties>`; `properties::window(ui, &DocProperties, open: &mut bool)`; funções puras `fn size_text(bytes: u64) -> String` e `fn page_size_text(PageSize) -> String`.

- [ ] **Passo 1: testes** (em `properties.rs`):

```rust
#[test]
fn sizes_are_human() {
    assert_eq!(size_text(900), "900 bytes");
    assert_eq!(size_text(1_536), "1,5 KB");
    assert_eq!(size_text(1_166_161), "1,11 MB");
}

#[test]
fn page_size_in_mm_and_inches() {
    let a4 = PageSize { width: 595.0, height: 842.0 };
    assert_eq!(page_size_text(a4), "210 × 297 mm (8,26 × 11,69 pol)");
}
```

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar a janela (`egui::Window::new("Propriedades do documento").open(open)`), seções **Descrição** e **Avançado** num `egui::Grid`, campos vazios como "—", datas com `app_state::pdf_date::format_pdf_date`, "Protegido por senha: Sim/Não". `run(Command::Properties)` pede ao worker e abre a janela quando a resposta chega (`PropertiesFailed` → status). **Passo 4:** testes passam. **Passo 5:** commit `feat(gera-ui): janela de propriedades do documento`.

---

### Tarefa 8: `gera-ui` — Imprimir (Ctrl+P)

**Files:** Create `crates/gera-ui/src/shell/print_dialog.rs`, `crates/gera-ui/src/print_job.rs`; Modify `doc_tab.rs`, `app.rs`, `shell/commands.rs`, `shell/menus.rs`, `shell/toolbar.rs`, `main.rs`.

**Consumes:** `app_state::print::*` (tarefa 1); `Request::ExportForPrint`/`Response::Exported` (tarefa 5).

**Produces:**
- `print_job::printers() -> (Vec<String>, Option<String>)` (roda `$GERA_LPSTAT` ou `lpstat` com `LC_ALL=C`: `-e` lista, `-d` padrão — linha "system default destination: X"); `print_job::spool_dir() -> PathBuf` (cria `$XDG_RUNTIME_DIR/gera-pdf-editor` 0700); `print_job::PrintJob::start(ctx, args: Vec<String>, file: PathBuf)` (thread que roda `$GERA_LP` ou `lp`, apaga `file`, devolve `Result<String, String>` com a saída do `lp` ou o erro) e `poll()`.
- `print_dialog::PrintDialog { options: PrintOptions, ranges_text: String, preview: usize, printers: Vec<String> }` com `show(ui, view, worker, total, current) -> Option<PrintChoice>` (`Print(PrintOptions)` | `Cancel`).
- `Command::Print` (Ctrl+P; menu Arquivo → Imprimir e botão ⎙ ativos).

- [ ] **Passo 1: testes** (em `print_job.rs`):

```rust
#[test]
fn default_printer_is_read_from_lpstat_output() {
    assert_eq!(parse_default("system default destination: HP_LaserJet\n"), Some("HP_LaserJet".into()));
    assert_eq!(parse_default("no system default destination\n"), None);
}

#[test]
fn printer_list_skips_blank_lines() {
    assert_eq!(parse_list("HP\n\nPDF\n"), vec!["HP".to_string(), "PDF".to_string()]);
}

#[test]
fn a_print_job_runs_lp_and_removes_the_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.pdf");
    std::fs::write(&file, b"%PDF").unwrap();
    let out = run_lp("echo", &["-d".into(), "HP".into(), file.display().to_string()], &file);
    assert!(out.unwrap().contains("-d HP"));
    assert!(!file.exists(), "o temporário sempre some");
    let fail = run_lp("false", &[], &file);
    assert!(fail.is_err());
}
```

(`gera-ui` precisa de `tempfile = "3"` em `[dev-dependencies]`.)

- [ ] **Passo 2:** ver falhar. **Passo 3:** implementar:
  - `print_job.rs`: `parse_default`, `parse_list`, `run_lp(program, args, file) -> Result<String, String>` (executa, apaga o arquivo em qualquer caso, junta stdout/stderr; código ≠ 0 → `Err(stderr)`), `printers()`, `spool_dir()`, `PrintJob` (thread + canal, como o `FileDialogs`).
  - `print_dialog.rs`: janela modal com as seções do spec (impressora em `ComboBox`; cópias `DragValue` 1..=999; "Imprimir em escala de cinza"; páginas em `radio_value` + campo de intervalos que fica vermelho com a mensagem de `parse_ranges`; "Mais opções" em `CollapsingHeader` com subconjunto e ordem inversa; dimensionamento em `radio_value` + `DragValue` de % para Escala personalizada; páginas por folha em `ComboBox` [1, 2, 4, 6, 9]; frente e verso e orientação em `ComboBox`; Comentários e formulários em `ComboBox` "Documento e marcações"/"Documento"; pré-visualização: miniatura da página `preview` com `view.request_at`/`texture_at` numa escala que caiba em 220 px de largura, com ◀ ▶). Botões **Imprimir** (desativado sem impressora ou com intervalo inválido) e **Cancelar**. "Nenhuma impressora encontrada" quando a lista vier vazia.
  - Fluxo: `Command::Print` abre o diálogo da aba ativa (lista de impressoras lida ao abrir); **Imprimir** → `DocTab` envia `ExportForPrint { pending: edit.pending.items().to_vec(), dest: spool_dir()/gera-<pid>-<n>.pdf, with_annots }` e guarda as opções; em `Response::Exported { dest }` monta `lp_args(&opts, current+1, total, oversized, &dest)` (oversized = alguma página com lado > 842 pt ou ambos > 595 pt) e chama `PrintJob::start`; o resultado vai para a barra de status ("Enviado para HP." / "Não foi possível imprimir: …"). `ExportFailed` → status.
- [ ] **Passo 4:** testes passam. **Passo 5:** commit `feat(gera-ui): imprimir com diálogo próprio e CUPS`.

---

### Tarefa 9: Refatoração da etapa, testes E2E, medição e verificação

- [ ] **Passo 1:** skill `simplify` uma vez sobre `git diff main...HEAD -- crates scripts`; aplicar o que não muda comportamento; `cargo fmt/clippy/test` verdes; commit `refactor: simplificação da v1.1-B1 (simplify)` (se houver mudança).
- [ ] **Passo 2: bateria E2E** (Xvfb `:99` + openbox + xdotool; `GERA_LP`/`GERA_LPSTAT` apontando para scripts no scratchpad que gravam os argumentos e listam "HP_Teste" como padrão):
  1. Arrastar com Selecionar sobre a linha → azul na tela; Ctrl+C → colar no campo de busca (Ctrl+F, Ctrl+V) e conferir o texto na captura.
  2. Com a vista girada 90°, arrastar sobre o texto → azul sobre as palavras.
  3. Botão direito → Riscar texto → salvar → reabrir → riscado no lugar.
  4. Ctrl+A, Ctrl+C → colar na busca e conferir que começa com "Page 1".
  5. Ctrl+P → conferir o diálogo (impressora "HP_Teste", pré-visualização); escolher Páginas "1-3", Frente e verso (borda longa), 2 por folha → Imprimir → conferir o arquivo de argumentos e a mensagem "Enviado para HP_Teste".
  6. Intervalo "0" → campo vermelho e Imprimir desativado.
  7. Ctrl+D → conferir páginas, tamanho e versão.
  Corrigir o que falhar (teste que reproduz → correção) e repetir a bateria até passar.
- [ ] **Passo 3:** `scripts/measure.sh` dentro das metas.
- [ ] **Passo 4:** `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, `cargo build --release`, `cargo audit`, `git status` limpos; skill `superpowers:verification-before-completion`.
- [ ] **Passo 5:** revisão independente da branch (modelo mais capaz) e uma rodada de correções para o que for crítico/importante.
