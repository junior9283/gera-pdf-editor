# v1.1-B2b — Caixa de texto, formas, linhas e carimbos: plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Acrescentar caixa de texto, retângulo, elipse, linha, seta e carimbos (padrão e dinâmicos) à linha Comentar, todos editáveis no modelo da v1.1-B2a.

**Architecture:** pdf-core cria/lê os tipos novos e ganha edições reversíveis novas (pontos da linha, preenchimento, fonte); o carimbo é uma imagem gerada pelo app (PDF minúsculo rasterizado a 300 dpi) gravada como aparência do Stamp. app-state ganha ferramentas, geometria pura e o catálogo de carimbos. gera-ui ganha gestos de criação, edição da caixa de texto sobre a página, propriedades por tipo e o diálogo "Identidade".

**Tech Stack:** Rust 2021; mupdf 0.8 (`add_free_text_annotation`, `add_square_annotation`, `add_circle_annotation`, `add_line_annotation`, `add_stamp_annotation`, `set_line`, `set_line_ending_styles`, `set_interior_color`, `set_default_appearance`, `set_author`, `Document::from_bytes`, `Page::to_pixmap`, `Image::from_pixmap`, `PdfDocument::add_image`, `Font::new`/`encode_character`/`advance_glyph`); egui/eframe 0.36.

**Spec:** `docs/superpowers/specs/2026-10-09-gera-pdf-editor-v1.1b2b-formas-carimbos-design.md`

## Global Constraints

- Textos e comentários em pt-BR; commits terminam com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `simplify` só na última tarefa (regra do projeto); E2E feito pelo executor em Xvfb + openbox + xdotool, sem D-Bus, `XDG_CONFIG_HOME` temporário.
- Ao fim de cada tarefa: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` verdes.
- Nada muda o arquivo do usuário sem Salvar; gravação segura, `Encryption::Keep` e garbage nível 1 da B2a continuam.
- Toda edição nova devolve a inversa **exata** (cores com `ColorEdit`, inclusive "sem cor/sem preenchimento").
- Comportamentos copiam o Acrobat (tabela da seção 2 da spec). Padrões: caixa de texto preta Helvetica 12 pt, borda vermelha 1 pt; formas e linhas vermelhas 1 pt sem preenchimento; carimbo 190 pt de largura.
- Metas: abertura < 300 ms, memória < 150 MB, binário < 25 MB.
- **Ruling do plano:** `Tool::Stamp` sem dados; o carimbo escolhido fica em `EditState.stamp: StampKind` (a spec dizia `Tool::Stamp(StampKind)`; assim `Tool::ALL` continua uma lista simples).

## Review Focus

1. **Linha diagonal longa**: clicar longe da linha, mas dentro do retângulo que a envolve, **não** pode selecioná-la (acerto pela distância ao segmento): `hit_on_a_diagonal_line_uses_the_distance_to_the_segment` (tarefa 5).
2. **Caixa de texto com acentos e várias linhas** volta idêntica ao reabrir: `text_box_with_accents_and_lines_round_trips` (tarefa 1).
3. **Carimbo em página girada (/Rotate 90)** continua legível e no lugar depois de mover e salvar: `stamp_survives_move_and_save_on_a_rotated_page` (tarefa 3).
4. **Desfazer preenchimento de forma sem preenchimento** remove o preenchimento (não deixa branco): `undoing_fill_on_a_shape_without_fill_removes_it` (tarefa 2).
5. **Arrasto minúsculo** (clique simples com Retângulo/Linha) não cria anotação invisível: `tiny_shape_drags_create_nothing` (tarefa 5).

---

### Tarefa 1: `pdf-core` — tipos novos, criação, leitura e autor

**Files:**
- Modify: `crates/pdf-core/src/types.rs`, `crates/pdf-core/src/doc.rs`, `crates/pdf-core/src/worker.rs`
- Test: `crates/pdf-core/tests/shapes.rs` (novo)

**Interfaces:**
- Produces:
  - `NewAnnot::TextBox { page: i32, rect: PdfRect, text: String, size: f32, color: [f32; 3] }`
  - `NewAnnot::Shape { page: i32, rect: PdfRect, ellipse: bool }`
  - `NewAnnot::Line { page: i32, a: (f32, f32), b: (f32, f32), arrow: bool }`
  - `NewAnnot::Stamp { page: i32, rect: PdfRect, title: String, subtitle: Option<String>, color: [f32; 3] }` (criação na tarefa 3; aqui só o tipo e `page()`/`is_empty()`)
  - `AnnotKind::{TextBox, Rect, Ellipse, Line, Stamp}`
  - `AnnotInfo` ganha `fill: Option<[f32; 3]>`, `line: Option<((f32, f32), (f32, f32))>`, `font: Option<(f32, [f32; 3])>`, `author: String`
  - `AnnotInfo::movable()` → todos menos Highlight/Underline/StrikeOut; `resizable()` → Ink, TextBox, Rect, Ellipse, Stamp (linha usa alças próprias)
  - `PdfDoc::set_author(&mut self, author: Option<String>)`; `Request::SetAuthor(Option<String>)` no worker (sem resposta)

- [ ] **Passo 1: testes** em `tests/shapes.rs`:

```rust
mod common;
use common::write_sample;
use pdf_core::{AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn reopen_only(path: &std::path::Path) -> pdf_core::AnnotInfo {
    let mut v = PdfDoc::open(path, None).unwrap().annotations(0).unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

#[test]
fn shapes_and_lines_are_created_and_read_back() {
    for (annot, kind) in [
        (NewAnnot::Shape { page: 0, rect: PdfRect::new(100.0, 100.0, 200.0, 150.0), ellipse: false }, AnnotKind::Rect),
        (NewAnnot::Shape { page: 0, rect: PdfRect::new(100.0, 100.0, 200.0, 150.0), ellipse: true }, AnnotKind::Ellipse),
        (NewAnnot::Line { page: 0, a: (100.0, 100.0), b: (300.0, 200.0), arrow: true }, AnnotKind::Line),
    ] {
        let (_d, path, mut d) = doc();
        d.set_author(Some("Maria Silva".into()));
        d.apply(AnnotOp::Create(annot)).unwrap();
        d.save(&path).unwrap();
        let a = reopen_only(&path);
        assert_eq!(a.kind, kind);
        assert_eq!(a.author, "Maria Silva");
        assert_eq!(a.color, Some([1.0, 0.0, 0.0]), "vermelho padrão");
        assert_eq!(a.width, Some(1.0));
        assert_eq!(a.fill, None);
        if kind == AnnotKind::Line {
            assert_eq!(a.line, Some(((100.0, 100.0), (300.0, 200.0))));
        }
    }
}

#[test]
fn text_box_with_accents_and_lines_round_trips() {
    let (_d, path, mut d) = doc();
    d.apply(AnnotOp::Create(NewAnnot::TextBox {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        text: "Atenção: revisão\nsegunda linha".into(),
        size: 12.0,
        color: [0.0, 0.0, 0.0],
    }))
    .unwrap();
    d.save(&path).unwrap();
    let a = reopen_only(&path);
    assert_eq!(a.kind, AnnotKind::TextBox);
    assert_eq!(a.contents, "Atenção: revisão\nsegunda linha");
    assert_eq!(a.font, Some((12.0, [0.0, 0.0, 0.0])));
    assert_eq!(a.color, Some([1.0, 0.0, 0.0]), "borda vermelha");
}

#[test]
fn empty_text_boxes_and_degenerate_shapes_are_refused() {
    let (_d, _p, mut d) = doc();
    for a in [
        NewAnnot::TextBox { page: 0, rect: PdfRect::new(1.0, 1.0, 50.0, 20.0), text: " ".into(), size: 12.0, color: [0.0; 3] },
        NewAnnot::Shape { page: 0, rect: PdfRect::new(5.0, 5.0, 6.0, 6.0), ellipse: false },
        NewAnnot::Line { page: 0, a: (5.0, 5.0), b: (6.0, 6.0), arrow: false },
    ] {
        assert!(d.apply(AnnotOp::Create(a)).is_err());
    }
    assert!(d.annotations(0).unwrap().is_empty());
}
```

- [ ] **Passo 2:** `cargo test -p pdf-core --test shapes` → falha de compilação (tipos não existem).
- [ ] **Passo 3: implementar.**
  - `types.rs`: variantes acima; `page()` cobre todas; `is_empty()`:
    ```rust
    Self::TextBox { text, .. } => text.trim().is_empty(),
    Self::Shape { rect, .. } | Self::Stamp { rect, .. } => rect.width() < 3.0 || rect.height() < 3.0,
    Self::Line { a, b, .. } => (b.0 - a.0).hypot(b.1 - a.1) < 3.0,
    ```
  - `doc.rs` `add_one`:
    ```rust
    NewAnnot::TextBox { rect, text, size, color, .. } => {
        let mut a = page.add_free_text_annotation(Rect::from(*rect), text)?;
        let [red, green, blue] = *color;
        a.set_default_appearance("Helv", *size, Some(AnnotationColor::Rgb { red, green, blue }))?;
        a.set_color(RED)?;
        a.set_border_width(1.0)?;
        a
    }
    NewAnnot::Shape { rect, ellipse, .. } => {
        let mut a = if *ellipse { page.add_circle_annotation(Rect::from(*rect))? } else { page.add_square_annotation(Rect::from(*rect))? };
        a.set_color(RED)?;
        a.set_border_width(1.0)?;
        a
    }
    NewAnnot::Line { a: p, b: q, arrow, .. } => {
        let mut a = page.add_line_annotation(Point::new(p.0, p.1), Point::new(q.0, q.1))?;
        a.set_color(RED)?;
        a.set_border_width(1.0)?;
        if *arrow { a.set_line_ending_styles(LineEndingStyle::None, LineEndingStyle::OpenArrow)?; }
        a
    }
    NewAnnot::Stamp { .. } => return Err(PdfError::InvalidAnnotation("carimbo: tarefa 3".into())),
    ```
    com `const RED: AnnotationColor = AnnotationColor::Rgb { red: 1.0, green: 0.0, blue: 0.0 };`.
  - `apply(Create)`: depois de `add_one`, se `self.author` existir, `annot.set_author(name)?` e `annot.update()?`.
  - `kind_of`: `FreeText → TextBox`, `Square → Rect`, `Circle → Ellipse`, `Line → Line`, `Stamp → Stamp`.
  - `annot_info`: `fill: annot.interior_color()?.map(rgb_of)`; `line: (kind == Line).then(|| annot.line()).transpose()?.map(|(p, q)| ((p.x, p.y), (q.x, q.y)))`; `font: (kind == TextBox).then(|| annot.default_appearance()).transpose()?.flatten().map(|da| (da.size, da.color.map(rgb_of).unwrap_or([0.0; 3])))`; `author: annot.author()?.unwrap_or_default().to_owned()`; `width` para `Ink | TextBox | Rect | Ellipse | Line`.
  - `PdfDoc { author: Option<String> }`, `set_author`; worker `Request::SetAuthor(a) => if let Some(d) = self.doc.as_mut() { d.set_author(a) }` — e o worker guarda o último autor para aplicar a documentos abertos depois (`self.author`), chamando `d.set_author(self.author.clone())` no `Open`.
- [ ] **Passo 4:** `cargo test -p pdf-core` → verde.
- [ ] **Passo 5:** commit `feat(pdf-core): caixa de texto, formas e linhas com autor`.

---

### Tarefa 2: `pdf-core` — edições novas e geometria das formas

**Files:** Modify `crates/pdf-core/src/annot_edit.rs`, `crates/pdf-core/src/doc.rs`, `crates/pdf-core/src/types.rs`; Test `crates/pdf-core/tests/shapes.rs`.

**Interfaces:**
- Produces:
  - `pub enum LineEnd { Start, End }` (types.rs)
  - `AnnotOp::MoveLinePoint { page, id, end: LineEnd, to: (f32, f32) }`
  - `AnnotOp::Fill { page, id, color: Option<[f32; 3]> }` (None = sem preenchimento)
  - `AnnotOp::Font { page, id, size: f32, color: [f32; 3] }`
  - `Edit::LinePoints { page, id, a, b }`, `Edit::Fill { page, id, color: ColorEdit }`, `Edit::Font { page, id, size, color: [f32; 3] }`
  - `AnnotOp::Move` em linha move os dois pontos; `Resize` vale para TextBox/Rect/Ellipse/Stamp via `set_rect` (caminho `Geometry` existente).

- [ ] **Passo 1: testes** (em `tests/shapes.rs`):

```rust
fn created(d: &mut PdfDoc, a: NewAnnot) -> pdf_core::AnnotInfo {
    d.apply(AnnotOp::Create(a)).unwrap();
    d.annotations(0).unwrap().pop().unwrap()
}

#[test]
fn line_points_move_and_undo_exactly() {
    let (_d, _p, mut d) = doc();
    let l = created(&mut d, NewAnnot::Line { page: 0, a: (100.0, 100.0), b: (300.0, 200.0), arrow: true });
    d.apply(AnnotOp::MoveLinePoint { page: 0, id: l.id, end: pdf_core::LineEnd::End, to: (400.0, 120.0) }).unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].line, Some(((100.0, 100.0), (400.0, 120.0))));
    d.apply(AnnotOp::Move { page: 0, id: l.id, dx: 10.0, dy: 5.0 }).unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].line, Some(((110.0, 105.0), (410.0, 125.0))));
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], l, "volta idêntica");
}

#[test]
fn undoing_fill_on_a_shape_without_fill_removes_it() {
    let (_d, _p, mut d) = doc();
    let s = created(&mut d, NewAnnot::Shape { page: 0, rect: PdfRect::new(100.0, 100.0, 200.0, 150.0), ellipse: false });
    d.apply(AnnotOp::Fill { page: 0, id: s.id, color: Some([0.1, 0.45, 0.95]) }).unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].fill, Some([0.1, 0.45, 0.95]));
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].fill, None);
}

#[test]
fn text_box_font_and_resize_undo_exactly() {
    let (_d, _p, mut d) = doc();
    let t = created(&mut d, NewAnnot::TextBox { page: 0, rect: PdfRect::new(100.0, 100.0, 300.0, 160.0), text: "oi".into(), size: 12.0, color: [0.0; 3] });
    d.apply(AnnotOp::Font { page: 0, id: t.id, size: 20.0, color: [0.9, 0.1, 0.1] }).unwrap();
    d.apply(AnnotOp::Resize { page: 0, id: t.id, rect: PdfRect::new(100.0, 100.0, 400.0, 200.0) }).unwrap();
    let now = &d.annotations(0).unwrap()[0];
    assert_eq!(now.font, Some((20.0, [0.9, 0.1, 0.1])));
    assert!((now.rect.x1 - 400.0).abs() < 1.0);
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], t);
}
```

- [ ] **Passo 2:** rodar → falha (ops não existem).
- [ ] **Passo 3: implementar.**
  - `annot_edit.rs`: novos braços em `apply_edit`, cada um lendo o estado antigo primeiro:
    ```rust
    Edit::LinePoints { a: p, b: q, .. } => {
        let (op, oq) = a.line()?;
        a.set_line(Point::new(p.0, p.1), Point::new(q.0, q.1))?;
        Edit::LinePoints { page, id, a: (op.x, op.y), b: (oq.x, oq.y) }
    }
    Edit::Fill { color, .. } => {
        let old = match a.interior_color()? { Some(c) => ColorEdit::Set(c), None => ColorEdit::Clear };
        match color {
            ColorEdit::Keep => {}
            ColorEdit::Set(c) => a.set_interior_color(c)?,
            ColorEdit::Clear => a.object().dict_delete("IC")?,
        }
        Edit::Fill { page, id, color: old }
    }
    Edit::Font { size, color: [red, green, blue], .. } => {
        let da = a.default_appearance()?.ok_or_else(|| PdfError::InvalidAnnotation("sem fonte".into()))?;
        let old = da.color.map(rgb_of).unwrap_or([0.0; 3]);
        a.set_default_appearance(&da.font_name, size, Some(AnnotationColor::Rgb { red, green, blue }))?;
        Edit::Font { page, id, size: da.size, color: old }
    }
    ```
    (`rgb_of` volta a ser importado de `crate::doc`).
  - `doc.rs::apply`: `MoveLinePoint` lê `line()` atual e monta `Edit::LinePoints` com a ponta trocada; `Move` de linha (`kind == Line`) monta `LinePoints` deslocado em vez de `Geometry`; `Fill` → `Edit::Fill` com `ColorEdit::Set(Rgb)`/`Clear`; `Font` → `Edit::Font`.
  - `Edit::page()` e o `(page, id)` do topo de `apply_edit` cobrem as variantes novas.
- [ ] **Passo 4:** `cargo test -p pdf-core` → verde.
- [ ] **Passo 5:** commit `feat(pdf-core): pontos da linha, preenchimento e fonte com desfazer`.

---

### Tarefa 3: `pdf-core` — carimbo como imagem

**Files:** Create `crates/pdf-core/src/stamp.rs`; Modify `crates/pdf-core/src/lib.rs`, `crates/pdf-core/src/doc.rs`; Test `crates/pdf-core/tests/shapes.rs`.

**Interfaces:**
- Consumes: `NewAnnot::Stamp` (tarefa 1).
- Produces: `stamp::image(title: &str, subtitle: Option<&str>, color: [f32; 3]) -> Result<mupdf::Image>` (pub(crate)); criação de `NewAnnot::Stamp` em `add_one` (agora recebe `&mut PdfDocument` além da página).

- [ ] **Passo 1: testes:**

```rust
fn stamp(page: i32, sub: Option<&str>) -> NewAnnot {
    NewAnnot::Stamp {
        page,
        rect: PdfRect::new(100.0, 100.0, 290.0, 160.0),
        title: "NÃO APROVADO".into(),
        subtitle: sub.map(str::to_owned),
        color: [0.8, 0.1, 0.1],
    }
}

/// Há pixels avermelhados dentro do retângulo (em pontos) da página renderizada a 1:1?
fn has_red_inside(d: &PdfDoc, r: PdfRect) -> bool {
    let img = d.render(0, 1.0, false).unwrap();
    let w = img.width as usize;
    (r.y0 as usize..r.y1 as usize).any(|y| (r.x0 as usize..r.x1 as usize).any(|x| {
        let p = &img.rgb[(y * w + x) * 3..][..3];
        p[0] > 150 && p[1] < 90 && p[2] < 90
    }))
}

#[test]
fn stamps_are_drawn_and_survive_move_and_save() {
    let (_d, path, mut d) = doc();
    let s = created(&mut d, stamp(0, Some("Maria Silva, 09/10/2026 14:30")));
    assert_eq!(s.kind, AnnotKind::Stamp);
    assert!(has_red_inside(&d, s.rect));
    d.apply(AnnotOp::Move { page: 0, id: s.id, dx: 100.0, dy: 200.0 }).unwrap();
    d.save(&path).unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    let a = again.annotations(0).unwrap().pop().unwrap();
    assert!(has_red_inside(&again, a.rect));
    assert!(a.rect.y0 > 250.0);
}

#[test]
fn stamp_survives_move_and_save_on_a_rotated_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "g.pdf", 1, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let s = created(&mut d, stamp(0, None));
    d.apply(AnnotOp::Move { page: 0, id: s.id, dx: 20.0, dy: 20.0 }).unwrap();
    d.save(&path).unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    assert_eq!(again.annotations(0).unwrap().len(), 1);
}
```
(Para a página girada basta sobreviver e continuar uma anotação; a legibilidade é conferida no E2E.)

- [ ] **Passo 2:** rodar → falha (`carimbo: tarefa 3`).
- [ ] **Passo 3: implementar `stamp.rs`:**

```rust
//! Carimbo desenhado pelo app: um PDF de 190×60 pt em memória, rasterizado a 300 dpi com fundo
//! transparente. O MuPDF só desenha carimbos de uma linha e corta nomes longos; a imagem dá a
//! moldura e as duas linhas do carimbo dinâmico, e o MuPDF a reajusta ao mover e redimensionar.
use crate::error::Result;
use mupdf::{Colorspace, Document, Font, Image, Matrix};

const W: f32 = 190.0;
const H: f32 = 60.0;

/// Texto num literal PDF em WinAnsi (os acentos do português cabem nele).
fn literal(s: &str) -> String {
    s.chars().map(|c| match c {
        '(' | ')' | '\\' => format!("\\{c}"),
        c if (c as u32) < 128 => c.to_string(),
        c if (c as u32) < 256 => format!("\\{:03o}", c as u32),
        _ => "?".into(),
    }).collect()
}

/// Largura do texto em pontos, na fonte e tamanho dados.
fn text_width(font: &Font, s: &str, size: f32) -> Result<f32> {
    let mut w = 0.0;
    for c in s.chars() {
        w += font.advance_glyph(font.encode_character(c as i32)?)?;
    }
    Ok(w * size)
}

/// Tamanho que faz `s` caber em `room` pontos (até `max`).
fn fitted(font: &Font, s: &str, max: f32, room: f32) -> Result<f32> {
    let w1 = text_width(font, s, 1.0)?.max(0.01);
    Ok(max.min(room / w1))
}

pub(crate) fn image(title: &str, subtitle: Option<&str>, [r, g, b]: [f32; 3]) -> Result<Image> {
    let font = Font::new("Times-Bold")?;
    let mut ops = format!("q {r} {g} {b} RG {r} {g} {b} rg 3 w 3 3 {} {} re S\n", W - 6.0, H - 6.0);
    let mut line = |text: &str, max: f32, y: f32| -> Result<()> {
        let size = fitted(&font, text, max, W - 24.0)?;
        let x = (W - text_width(&font, text, size)?) / 2.0;
        ops.push_str(&format!("BT /F1 {size} Tf {x} {y} Td ({}) Tj ET\n", literal(text)));
        Ok(())
    };
    match subtitle {
        Some(sub) => { line(title, 26.0, 28.0)?; line(sub, 11.0, 12.0)?; }
        None => line(title, 30.0, 20.0)?,
    }
    ops.push_str("Q");
    let pdf = format!(
        "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 {W} {H}]/Resources<</Font<</F1 5 0 R>>>>/Contents 4 0 R>>endobj\n\
         4 0 obj<</Length {}>>stream\n{ops}\nendstream endobj\n\
         5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Times-Bold/Encoding/WinAnsiEncoding>>endobj\ntrailer<</Root 1 0 R>>\n",
        ops.len()
    );
    let page = Document::from_bytes(pdf.as_bytes(), "pdf")?.load_page(0)?;
    let scale = 300.0 / 72.0;
    let pix = page.to_pixmap(&Matrix::new_scale(scale, scale), &Colorspace::device_rgb(), true, false)?;
    Ok(Image::from_pixmap(&pix)?)
}
```
  - Em `add_one` (que passa a receber `doc: &mut PdfDocument`):
    ```rust
    NewAnnot::Stamp { rect, title, subtitle, color, .. } => {
        let img = stamp::image(title, subtitle.as_deref(), *color)?;
        let image_ref = doc.add_image(&img)?;
        let mut a = page.add_stamp_annotation(Rect::from(*rect), title)?;
        let mut xobjects = doc.new_dict()?;
        xobjects.dict_put("I", image_ref)?;
        let mut res = doc.new_dict()?;
        res.dict_put("XObject", xobjects)?;
        let mut normal = doc.new_dict()?;
        normal.dict_put("Resources", res)?;
        let mut ap = doc.new_dict()?;
        ap.dict_put("N", normal)?;
        a.object().dict_put("AP", ap)?;
        a
    }
    ```
    (O nome do carimbo vai como `/Name`, só para identificação; a aparência vem da imagem.)
- [ ] **Passo 4:** `cargo test -p pdf-core` → verde.
- [ ] **Passo 5:** commit `feat(pdf-core): carimbos com moldura e texto dinâmico`.

---

### Tarefa 4: `app-state` — ferramentas, geometria, catálogo de carimbos e autor

**Files:** Modify `crates/app-state/src/tools.rs`, `crates/app-state/src/annot_geom.rs`, `crates/app-state/src/prefs.rs`, `crates/app-state/src/lib.rs`; Create `crates/app-state/src/stamps.rs`.

**Interfaces:**
- Produces:
  - `Tool::{TextBox, Rect, Ellipse, Line, Arrow, Stamp}`, `Tool::ALL` com 11 itens, rótulos "Caixa de texto", "Retângulo", "Elipse", "Linha", "Seta", "Carimbo".
  - `annot_geom::rect_from_drag(a: (f32,f32), b: (f32,f32), square: bool) -> PdfRect`
  - `annot_geom::snap_45(a: (f32,f32), b: (f32,f32)) -> (f32,f32)`
  - `annot_geom::dist_to_segment(p: (f32,f32), a: (f32,f32), b: (f32,f32)) -> f32`
  - `annot_geom::line_handle_at(a, b, p, size: f32) -> Option<pdf_core::LineEnd>`
  - `stamps::StampKind` (enum com os 3 dinâmicos e os 14 padrão), `StampKind::{DYNAMIC, STANDARD}: [StampKind; N]`, `title(self) -> &'static str`, `color(self) -> [f32; 3]`, `is_dynamic(self) -> bool`, `stamps::subtitle(name: &str, now: (i32, u32, u32, u32, u32)) -> String`, `stamps::STAMP_WIDTH = 190.0`, `stamps::STAMP_HEIGHT = 60.0`.
  - `Prefs.author: Option<String>` (serde default).

- [ ] **Passo 1: testes:**

```rust
// annot_geom.rs
#[test]
fn drag_rect_works_in_any_direction_and_squares_with_shift() {
    assert_eq!(rect_from_drag((50.0, 80.0), (10.0, 20.0), false), PdfRect::new(10.0, 20.0, 50.0, 80.0));
    assert_eq!(rect_from_drag((10.0, 10.0), (50.0, 30.0), true), PdfRect::new(10.0, 10.0, 50.0, 50.0));
    assert_eq!(rect_from_drag((50.0, 50.0), (10.0, 40.0), true), PdfRect::new(10.0, 10.0, 50.0, 50.0));
}

#[test]
fn snap_45_keeps_length_direction_on_the_nearest_eighth() {
    assert_eq!(snap_45((0.0, 0.0), (100.0, 8.0)), (100.0, 0.0));
    let (x, y) = snap_45((0.0, 0.0), (70.0, 75.0));
    assert!((x - y).abs() < 0.01 && x > 0.0);
    assert_eq!(snap_45((0.0, 0.0), (-3.0, -90.0)), (0.0, -90.0));
}

#[test]
fn distance_to_segment_clamps_to_the_ends() {
    assert_eq!(dist_to_segment((5.0, 5.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
    assert_eq!(dist_to_segment((13.0, 4.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
}

#[test]
fn line_handles_are_at_the_two_ends() {
    use pdf_core::LineEnd;
    let (a, b) = ((0.0, 0.0), (100.0, 50.0));
    assert_eq!(line_handle_at(a, b, (2.0, -1.0), 4.0), Some(LineEnd::Start));
    assert_eq!(line_handle_at(a, b, (99.0, 52.0), 4.0), Some(LineEnd::End));
    assert_eq!(line_handle_at(a, b, (50.0, 25.0), 4.0), None);
}

// stamps.rs
#[test]
fn catalog_titles_are_unique_and_colored() {
    let all: Vec<_> = StampKind::DYNAMIC.iter().chain(&StampKind::STANDARD).collect();
    assert_eq!(all.len(), 17);
    for (n, a) in all.iter().enumerate() {
        assert!(all[n + 1..].iter().all(|b| b.title() != a.title() || b.is_dynamic() != a.is_dynamic()));
    }
    assert_eq!(StampKind::NotApproved.color(), [0.8, 0.1, 0.1]);
    assert!(StampKind::DYNAMIC.iter().all(|s| s.is_dynamic()));
}

#[test]
fn dynamic_subtitle_has_name_date_and_time() {
    assert_eq!(subtitle("Maria Silva", (2026, 10, 9, 14, 5)), "Maria Silva, 09/10/2026 14:05");
}
```

- [ ] **Passo 2:** rodar `cargo test -p app-state` → falha de compilação.
- [ ] **Passo 3: implementar.**
  - `rect_from_drag`: `let (dx, dy) = (b.0 - a.0, b.1 - a.1); let b = if square { let s = dx.abs().max(dy.abs()); (a.0 + s * dx.signum(), a.1 + s * dy.signum()) } else { b }; PdfRect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))` (com `signum` de 0 tratado como 1).
  - `snap_45`: ângulo `atan2(dy, dx)` arredondado a múltiplos de π/4, mesmo comprimento; arredondar os componentes a 0,01 para que eixos saiam exatos.
  - `dist_to_segment`: projeção `t` limitada a [0, 1].
  - `line_handle_at`: compara `|p − a|` e `|p − b|` com `size` (no máximo um dos dois; o mais próximo).
  - `stamps.rs`: `StampKind` com variantes `DynApproved, DynReviewed, DynReceived, Approved, NotApproved, Confidential, Draft, Final, Expired, ForComment, TopSecret, AsIs, Sold, Departmental, Experimental, ForPublicRelease, NotForPublicRelease`; títulos "APROVADO", "REVISADO", "RECEBIDO", "APROVADO", "NÃO APROVADO", "CONFIDENCIAL", "RASCUNHO", "FINAL", "VENCIDO", "PARA COMENTÁRIO", "ULTRASSECRETO", "CONFORME", "VENDIDO", "DEPARTAMENTAL", "EXPERIMENTAL", "DIVULGAÇÃO PÚBLICA", "NÃO DIVULGAR"; cores pela spec (verde: aprovado/conforme/final/recebido/revisado; vermelho: não aprovado/confidencial/vencido/ultrassecreto/não divulgar; azul: demais); `subtitle` formata `"{name}, {d:02}/{m:02}/{y} {h:02}:{min:02}"`.
  - `Prefs.author: Option<String>` (default `None`).
- [ ] **Passo 4:** `cargo test -p app-state` → verde.
- [ ] **Passo 5:** commit `feat(app-state): ferramentas de formas, geometria e catálogo de carimbos`.

---

### Tarefa 5: `gera-ui` — criar caixa de texto, formas, linhas e carimbos

**Files:** Modify `crates/gera-ui/src/edit.rs`, `crates/gera-ui/src/doc_tab.rs`, `crates/gera-ui/src/shell/toolbar.rs`, `crates/gera-ui/src/app.rs`; Create `crates/gera-ui/src/shell/identity.rs`.

**Interfaces:**
- Consumes: tarefas 1, 3, 4.
- Produces: `EditState.stamp: StampKind`; `EditState::shape_drag: Option<(i32, (f32,f32), (f32,f32))>`; `EditState::text_draft: Option<TextDraft>` (página, retângulo, alvo Novo/Existente(id), texto); `fn hit_at` passa a usar `dist_to_segment` para `AnnotKind::Line`; `Request::SetAuthor` enviado pela aba ao abrir e quando o nome muda; `identity::window(ui, &mut String) -> Option<bool>` (OK/Cancelar).

Comportamento:
1. **Retângulo/Elipse/Linha/Seta**: arrasto Primary guarda início (`press_origin`) e fim; Shift (`i.modifiers.shift`) aplica `rect_from_drag(.., true)` ou `snap_45`; pré-visualização tracejada (retângulo, elipse com `Shape::ellipse_stroke`, linha) na cor vermelha; ao soltar, `add(NewAnnot::Shape/Line)` (vazias são descartadas por `is_empty`).
2. **Caixa de texto**: clique cria `text_draft` com retângulo 200×24 pt a partir do ponto; arrasto define o retângulo. O rascunho é editado sobre a página (ver tarefa 6, mesmo componente); ao terminar → `add(NewAnnot::TextBox { size: 12.0, color: [0.0; 3], .. })`.
3. **Carimbo**: clique coloca `NewAnnot::Stamp` centrado no ponto com `STAMP_WIDTH × STAMP_HEIGHT`, título/cor do catálogo e, se `is_dynamic`, `subtitle(autor, agora)`; sem autor, abre "Identidade" e só cria após OK (guardando o pedido pendente).
4. **Toolbar**: botões novos depois de Desenhar; Carimbo é um `menu_button` com seções "Dinâmicos" e "Padrão comercial"; escolher define `edit.stamp` e `edit.tool = Tool::Stamp`.
5. **Esc** (fora de campo de texto e sem rascunho aberto): `edit.tool = Tool::Select`.
6. Hora local: crate `time` 0.3 com `local-offset` (MIT/Apache) — `time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc())`.

- [ ] **Passo 1: testes** (em `edit.rs`, puros sobre o estado):

```rust
#[test]
fn tiny_shape_drags_create_nothing() {
    let mut e = EditState::default();
    e.finish_shape(Tool::Rect, 0, (10.0, 10.0), (11.0, 11.5), false);
    e.finish_shape(Tool::Line, 0, (10.0, 10.0), (11.0, 11.0), false);
    assert!(e.take_ops().is_empty());
}

#[test]
fn shapes_follow_the_tool_and_shift() {
    let mut e = EditState::default();
    e.finish_shape(Tool::Ellipse, 1, (0.0, 0.0), (40.0, 20.0), true);
    e.finish_shape(Tool::Arrow, 1, (0.0, 0.0), (100.0, 7.0), true);
    assert!(matches!(&e.take_ops()[..], [
        AnnotOp::Create(NewAnnot::Shape { ellipse: true, rect, .. }),
        AnnotOp::Create(NewAnnot::Line { arrow: true, b: (100.0, 0.0), .. }),
    ] if rect.width() == 40.0 && rect.height() == 40.0));
}

#[test]
fn dynamic_stamps_wait_for_a_name() {
    let mut e = EditState { stamp: StampKind::DynApproved, ..Default::default() };
    assert!(!e.place_stamp(0, (200.0, 200.0), None, (2026, 10, 9, 14, 30)));
    assert!(e.take_ops().is_empty(), "sem nome: pede a identidade antes");
    assert!(e.place_stamp(0, (200.0, 200.0), Some("Maria"), (2026, 10, 9, 14, 30)));
    assert!(matches!(&e.take_ops()[..], [AnnotOp::Create(NewAnnot::Stamp { subtitle: Some(s), .. })] if s == "Maria, 09/10/2026 14:30"));
}

#[test]
fn hit_on_a_diagonal_line_uses_the_distance_to_the_segment() {
    let map = PageMap { origin: (0.0, 0.0), zoom: 1.0, size: (612.0, 792.0), rotation: pdf_core::Rotation::R0 };
    let mut l = info(0, 7);
    l.kind = AnnotKind::Line;
    l.rect = PdfRect::new(0.0, 0.0, 200.0, 200.0);
    l.line = Some(((0.0, 0.0), (200.0, 200.0)));
    assert_eq!(hit_at(&[l.clone()], 0, (100.0, 101.0), &map), Some((0, 7)));
    assert_eq!(hit_at(&[l], 0, (180.0, 20.0), &map), None, "dentro do retângulo, longe da linha");
}
```

- [ ] **Passo 2:** rodar → falha de compilação.
- [ ] **Passo 3: implementar** `finish_shape(tool, page, a, b, shift)`, `place_stamp(page, at, author: Option<&str>, now) -> bool`, `hit_at` com linhas, gestos em `on_page` por ferramenta, toolbar, Esc, `identity.rs`, `Request::SetAuthor`, dependência `time = { version = "0.3", features = ["local-offset"] }` em gera-ui.
- [ ] **Passo 4:** `cargo test` → verde; smoke `--bench-open`.
- [ ] **Passo 5:** commit `feat(gera-ui): criar caixa de texto, formas, linhas e carimbos`.

---

### Tarefa 6: `gera-ui` — editar: alças da linha, caixa de texto sobre a página e propriedades por tipo

**Files:** Modify `crates/gera-ui/src/edit.rs`, `crates/gera-ui/src/shell/annot_props.rs`.

**Interfaces:**
- Consumes: `AnnotOp::{MoveLinePoint, Fill, Font}`, `AnnotInfo::{line, fill, font}`, `line_handle_at`.
- Produces: `Grab::LinePoint(LineEnd)` no arrasto; edição da caixa de texto (duplo clique → `text_draft` Existente → `SetContents`); janela de propriedades com seções por tipo.

Comportamento:
1. Linha selecionada: duas alças (pontas) em vez das 4; arrastar uma ponta → pré-visualização → `MoveLinePoint` (Shift: `snap_45` em relação à outra ponta).
2. Caixa de texto: duplo clique abre um `egui::TextEdit::multiline` posicionado sobre `screen_rect(rect)`, fonte proporcional ao zoom, com foco; clicar fora/Esc confirma (vazio em caixa nova = descarta; em existente = mantém o texto antigo).
3. Propriedades:
   - Rect/Ellipse: cor da linha, **Preenchimento** (botão "Nenhum" + 8 cores → `Fill`), espessura, opacidade;
   - Line: cor, espessura, opacidade;
   - TextBox: **Cor do texto** (8 cores → `Font`), **Tamanho** (slider 8–72 → `Font` ao soltar), cor da borda, preenchimento, opacidade;
   - Stamp: opacidade.

- [ ] **Passo 1: testes** (puros):

```rust
// annot_props.rs
#[test]
fn sections_follow_the_annotation_kind() {
    use pdf_core::AnnotKind::*;
    assert_eq!(sections(Rect), Sections { line: true, fill: true, width: true, font: false });
    assert_eq!(sections(Line), Sections { line: true, fill: false, width: true, font: false });
    assert_eq!(sections(TextBox), Sections { line: true, fill: true, width: false, font: true });
    assert_eq!(sections(Stamp), Sections { line: false, fill: false, width: false, font: false });
}

// edit.rs
#[test]
fn dragging_a_line_end_sends_the_new_point() {
    let d = AnnotDrag { page: 0, id: 3, rect: PdfRect::new(0.0, 0.0, 100.0, 50.0),
        grab: Grab::LinePoint(pdf_core::LineEnd::End), from: (100.0, 50.0), to: (130.0, 70.0) };
    assert!(matches!(d.op(), Some(AnnotOp::MoveLinePoint { id: 3, to: (130.0, 70.0), .. })));
}
```
- [ ] **Passo 2:** rodar → falha.
- [ ] **Passo 3:** implementar `Sections`/`sections(kind)`, as seções na janela, `Grab::LinePoint`, pintura das alças da linha, edição sobre a página.
- [ ] **Passo 4:** `cargo test` → verde; smoke.
- [ ] **Passo 5:** commit `feat(gera-ui): editar linhas, caixas de texto e propriedades por tipo`.

---

### Tarefa 7: Refatoração, E2E, medição e revisão

- [ ] `simplify` sobre `git diff main...HEAD -- crates scripts`; verde; commit.
- [ ] **E2E** (Xvfb + openbox + xdotool):
  1. Retângulo, elipse (com Shift → círculo), linha, seta: criar e conferir na tela.
  2. Caixa de texto: clicar, digitar "Atenção", clicar fora; duplo clique, editar, conferir no painel.
  3. Puxar a ponta da seta; Ctrl+Z volta.
  4. Retângulo → Propriedades → preenchimento azul; Ctrl+Z tira.
  5. Carimbo dinâmico: pede o nome, coloca com nome/data; carimbo padrão NÃO APROVADO.
  6. Salvar, reabrir, conferir tudo; Esc volta a Selecionar.
- [ ] `scripts/measure.sh`; verificação final (fmt, clippy, testes, build, audit, git status).
- [ ] Revisão independente (modelo mais capaz) e uma rodada de correções.
