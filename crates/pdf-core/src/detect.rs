//! Detecção automática de campos (como a do Acrobat ao abrir o Preparar formulário num documento
//! sem formulário): linhas para escrever, sequências de "_", caixas e células vazias viram campos,
//! com o nome do rótulo ao lado. Regras sobre os traços e o texto da página, em coordenadas do
//! MuPDF (y para baixo).
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::prepare::NewFieldKind;
use crate::types::{PdfRect, Word};
use std::cell::RefCell;
use std::rc::Rc;

/// Traço da página: segmento de reta ou retângulo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    Line { a: (f32, f32), b: (f32, f32) },
    Rect(PdfRect),
}

/// Campo a criar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    pub rect: PdfRect,
    pub kind: NewFieldKind,
    /// Nome tirado do rótulo (sem rótulo, o automático).
    pub name: Option<String>,
}

/// Página com mais traços que isso é pulada (desenho técnico, PDF hostil).
pub(crate) const MAX_SHAPES: usize = 20_000;
/// Palavras, propostas analisadas e campos existentes por página, no máximo (o custo cresce com o
/// produto deles).
const MAX_WORDS: usize = 20_000;
const MAX_PROPOSALS: usize = 2_000;
const MAX_EXISTING: usize = 2_000;
/// Campos detectados por página, no máximo.
const MAX_FIELDS: usize = 400;
/// Altura do campo de texto sobre uma linha.
const LINE_FIELD_H: f32 = 18.0;

/// Altura do campo de assinatura sobre uma linha.
const SIGNATURE_H: f32 = 30.0;

/// O centro de `inner` está dentro de `outer`?
fn center_in(inner: &PdfRect, outer: &PdfRect) -> bool {
    outer.contains((inner.x0 + inner.x1) / 2.0, (inner.y0 + inner.y1) / 2.0)
}

/// Os dois se sobrepõem em mais de 20% do menor?
fn overlaps(a: &PdfRect, b: &PdfRect) -> bool {
    let w = a.x1.min(b.x1) - a.x0.max(b.x0);
    let h = a.y1.min(b.y1) - a.y0.max(b.y0);
    if w <= 0.0 || h <= 0.0 {
        return false;
    }
    let area = |r: &PdfRect| (r.width() * r.height()).max(0.01);
    w * h > 0.2 * area(a).min(area(b))
}

/// Uma palavra que é só sublinhado (a linha de escrever, não um rótulo).
fn is_blank(w: &Word) -> bool {
    !w.text.is_empty() && w.text.chars().all(|c| c == '_')
}

/// Rótulo limpo: sem ":" e "_" no fim, sem ponto (que faria um nome hierárquico), até 40
/// caracteres.
fn clean(label: &str) -> Option<String> {
    let t = label
        .trim()
        .trim_end_matches(|c: char| c == ':' || c == '_' || c.is_whitespace())
        .replace('.', " ");
    let t: String = t.trim().chars().take(40).collect();
    (!t.is_empty()).then_some(t)
}

/// As palavras da mesma linha do campo, juntas a partir da mais perto (para a esquerda ou para
/// a direita), até um espaço maior que 12 pt.
fn beside(rect: &PdfRect, words: &[Word], right: bool) -> Option<String> {
    let line = |w: &&Word| {
        let cy = (w.rect.y0 + w.rect.y1) / 2.0;
        cy >= rect.y0 - 4.0 && cy <= rect.y1 + 4.0 && !is_blank(w)
    };
    let mut row: Vec<&Word> = words
        .iter()
        .filter(line)
        .filter(|w| {
            if right {
                w.rect.x0 >= rect.x1 - 2.0 && w.rect.x0 - rect.x1 <= 100.0
            } else {
                w.rect.x1 <= rect.x0 + 2.0 && rect.x0 - w.rect.x1 <= 200.0
            }
        })
        .collect();
    // da mais perto para a mais longe
    if right {
        row.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    } else {
        row.sort_by(|a, b| b.rect.x1.total_cmp(&a.rect.x1));
    }
    let mut picked: Vec<&Word> = Vec::new();
    for w in row {
        if let Some(last) = picked.last() {
            let gap = if right {
                w.rect.x0 - last.rect.x1
            } else {
                last.rect.x0 - w.rect.x1
            };
            if gap > 12.0 {
                break;
            }
        }
        picked.push(w);
    }
    if !right {
        picked.reverse();
    }
    let text: Vec<&str> = picked.iter().map(|w| w.text.as_str()).collect();
    clean(&text.join(" "))
}

/// A linha de texto logo acima do campo (até 20 pt), sobre ele.
fn above(rect: &PdfRect, words: &[Word]) -> Option<String> {
    let near: Vec<&Word> = words
        .iter()
        .filter(|w| !is_blank(w))
        .filter(|w| w.rect.y1 <= rect.y0 + 2.0 && rect.y0 - w.rect.y1 <= 20.0)
        .filter(|w| w.rect.x1 > rect.x0 && w.rect.x0 < rect.x1)
        .collect();
    let lowest = near.iter().map(|w| w.rect.y1).fold(f32::MIN, f32::max);
    let mut line: Vec<&&Word> = near
        .iter()
        .filter(|w| (w.rect.y1 - lowest).abs() < 3.0)
        .collect();
    line.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    let text: Vec<&str> = line.iter().map(|w| w.text.as_str()).collect();
    clean(&text.join(" "))
}

/// O tipo pelo rótulo: assinatura e data, como o Acrobat.
fn kind_of(label: Option<&str>) -> NewFieldKind {
    let words: Vec<String> = label
        .unwrap_or_default()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_string)
        .collect();
    let has = |names: &[&str]| words.iter().any(|w| names.contains(&w.as_str()));
    if has(&["assinatura", "signature", "assinado"]) {
        NewFieldKind::Signature
    } else if has(&["data", "date"]) {
        NewFieldKind::Date
    } else {
        NewFieldKind::Text
    }
}

pub(crate) fn detect_candidates(
    shapes: &[Shape],
    words: &[Word],
    existing: &[PdfRect],
) -> Vec<Candidate> {
    // página hostil (ou desenho técnico): grande demais para olhar campo a campo
    if shapes.len() > MAX_SHAPES || words.len() > MAX_WORDS || existing.len() > MAX_EXISTING {
        return Vec::new();
    }
    // cada consulta ao texto percorre as palavras: o número de consultas também tem teto
    let checks = std::cell::Cell::new(0usize);
    let has_text = |r: &PdfRect| {
        checks.set(checks.get() + 1);
        words.iter().any(|w| !is_blank(w) && center_in(&w.rect, r))
    };
    let full = || checks.get() >= MAX_PROPOSALS;
    // (retângulo, é caixa de seleção, nome que veio junto do sublinhado)
    let mut proposals: Vec<(PdfRect, bool, Option<String>)> = Vec::new();
    let mut lines: Vec<(f32, f32, f32)> = Vec::new();
    for s in shapes {
        if full() {
            break;
        }
        match *s {
            Shape::Rect(r) => {
                let (w, h) = (r.width(), r.height());
                if (6.0..=20.0).contains(&w) && (6.0..=20.0).contains(&h) && (w - h).abs() <= 3.0 {
                    proposals.push((r, true, None));
                } else if h <= 2.0 && w >= 36.0 {
                    // retângulo fino: uma linha
                    lines.push((r.x0, r.x1, r.y1));
                } else if w >= 40.0 && (12.0..=80.0).contains(&h) && !has_text(&r) {
                    proposals.push((
                        PdfRect::new(r.x0 + 1.0, r.y0 + 1.0, r.x1 - 1.0, r.y1 - 1.0),
                        false,
                        None,
                    ));
                }
            }
            Shape::Line { a, b } => {
                if (a.1 - b.1).abs() <= 1.0 && (b.0 - a.0).abs() >= 36.0 {
                    lines.push((a.0.min(b.0), a.0.max(b.0), a.1.max(b.1)));
                }
            }
        }
    }
    // "_____" no texto: o campo sobre a sequência (o texto antes dela, na mesma palavra, é o rótulo)
    for w in words {
        if proposals.len() >= MAX_PROPOSALS {
            break;
        }
        let chars: Vec<char> = w.text.chars().collect();
        let Some(start) = chars.iter().position(|c| *c == '_') else {
            continue;
        };
        let run = chars[start..].iter().take_while(|c| **c == '_').count();
        if run < 5 {
            continue;
        }
        let per = w.rect.width() / chars.len().max(1) as f32;
        let x0 = w.rect.x0 + per * start as f32;
        let x1 = x0 + per * run as f32;
        let prefix: String = chars[..start].iter().collect();
        let rect = PdfRect::new(x0, w.rect.y1 - LINE_FIELD_H, x1, w.rect.y1);
        proposals.push((rect, false, clean(&prefix)));
    }
    for (x0, x1, y) in lines {
        if full() || proposals.len() >= MAX_PROPOSALS {
            break;
        }
        let rect = PdfRect::new(x0, y - LINE_FIELD_H, x1, y - 0.5);
        if !has_text(&rect) {
            proposals.push((rect, false, None));
        }
    }
    let mut taken: Vec<PdfRect> = existing.to_vec();
    let mut out = Vec::new();
    for (mut rect, checkbox, name) in proposals {
        if out.len() >= MAX_FIELDS || taken.iter().any(|t| overlaps(t, &rect)) {
            continue;
        }
        let name = name.or_else(|| {
            if checkbox {
                beside(&rect, words, true).or_else(|| beside(&rect, words, false))
            } else {
                beside(&rect, words, false).or_else(|| above(&rect, words))
            }
        });
        let kind = if checkbox {
            NewFieldKind::Checkbox
        } else {
            kind_of(name.as_deref())
        };
        if kind == NewFieldKind::Signature && rect.height() < SIGNATURE_H {
            rect.y0 = rect.y1 - SIGNATURE_H;
        }
        taken.push(rect);
        out.push(Candidate { rect, kind, name });
    }
    out
}

/// Junta os segmentos de um caminho: dois pontos são uma linha; quatro (fechado ou não) num
/// retângulo alinhado aos eixos, um retângulo.
#[derive(Default)]
struct Collector {
    m: mupdf::Matrix,
    sub: Vec<(f32, f32)>,
    /// O subcaminho tem curva: não é linha nem caixa de formulário.
    curved: bool,
    out: Vec<Shape>,
}

impl Collector {
    fn new(m: mupdf::Matrix) -> Self {
        Self {
            m,
            sub: Vec::new(),
            curved: false,
            out: Vec::new(),
        }
    }

    fn flush(&mut self) {
        let pts = std::mem::take(&mut self.sub);
        let curved = std::mem::take(&mut self.curved);
        // um caminho só com milhões de pedaços: para no teto (a página é pulada)
        if curved
            || self.out.len() > MAX_SHAPES
            || !pts.iter().all(|p| p.0.is_finite() && p.1.is_finite())
        {
            return;
        }
        match pts.len() {
            2 => self.out.push(Shape::Line {
                a: pts[0],
                b: pts[1],
            }),
            4 | 5 => {
                let (x0, x1, y0, y1) = pts.iter().fold(
                    (f32::MAX, f32::MIN, f32::MAX, f32::MIN),
                    |(x0, x1, y0, y1), p| (x0.min(p.0), x1.max(p.0), y0.min(p.1), y1.max(p.1)),
                );
                // só os alinhados aos eixos: cada ponto num canto
                let corner = |p: &(f32, f32)| {
                    ((p.0 - x0).abs() < 0.5 || (p.0 - x1).abs() < 0.5)
                        && ((p.1 - y0).abs() < 0.5 || (p.1 - y1).abs() < 0.5)
                };
                if pts.iter().all(corner) {
                    self.out.push(Shape::Rect(PdfRect::new(x0, y0, x1, y1)));
                }
            }
            _ => {}
        }
    }

    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.m;
        (m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f)
    }
}

impl mupdf::PathWalker for Collector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        let p = self.point(x, y);
        self.sub.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.sub.push(p);
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, ex: f32, ey: f32) {
        self.curved = true;
        let p = self.point(ex, ey);
        self.sub.push(p);
    }
    fn close(&mut self) {
        self.flush();
    }
}

/// Dispositivo que só guarda os traços (até o teto).
struct ShapeDevice(Rc<RefCell<Vec<Shape>>>);

impl ShapeDevice {
    fn add(&self, path: &mupdf::Path, m: mupdf::Matrix) {
        if self.0.borrow().len() > MAX_SHAPES {
            return;
        }
        let mut c = Collector::new(m);
        if path.walk(&mut c).is_ok() {
            c.flush();
            self.0.borrow_mut().extend(c.out);
        }
    }
}

impl mupdf::NativeDevice for ShapeDevice {
    fn fill_path(
        &mut self,
        path: &mupdf::Path,
        _even_odd: bool,
        cmt: mupdf::Matrix,
        _cs: &mupdf::Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: mupdf::ColorParams,
    ) {
        self.add(path, cmt);
    }

    fn stroke_path(
        &mut self,
        path: &mupdf::Path,
        _stroke: &mupdf::StrokeState,
        cmt: mupdf::Matrix,
        _cs: &mupdf::Colorspace,
        _color: &[f32],
        _alpha: f32,
        _cp: mupdf::ColorParams,
    ) {
        self.add(path, cmt);
    }
}

impl PdfDoc {
    /// Os campos que a detecção acha na página (sem os que se sobrepõem aos existentes).
    pub(crate) fn field_candidates(&self, page: i32) -> Result<Vec<Candidate>> {
        let p = self.load(page)?;
        let shapes = Rc::new(RefCell::new(Vec::new()));
        let device = mupdf::Device::from_native(ShapeDevice(shapes.clone()))?;
        p.run(&device, &mupdf::Matrix::IDENTITY)?;
        drop(device);
        let shapes = std::mem::take(&mut *shapes.borrow_mut());
        let words = crate::doc::page_words(&p)?;
        let existing: Vec<PdfRect> = self.fields(page)?.into_iter().map(|f| f.rect).collect();
        Ok(detect_candidates(&shapes, &words, &existing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(x0: f32, y0: f32, text: &str) -> Word {
        let w = text.chars().count() as f32 * 5.0;
        Word {
            rect: PdfRect::new(x0, y0, x0 + w, y0 + 10.0),
            text: text.into(),
        }
    }

    fn line(x0: f32, x1: f32, y: f32) -> Shape {
        Shape::Line {
            a: (x0, y),
            b: (x1, y),
        }
    }

    #[test]
    fn line_with_label_becomes_named_text_field() {
        let words = [word(50.0, 100.0, "Nome:")];
        let out = detect_candidates(&[line(90.0, 300.0, 110.0)], &words, &[]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, NewFieldKind::Text);
        assert_eq!(out[0].name.as_deref(), Some("Nome"));
        assert!(out[0].rect.y1 <= 110.0 && out[0].rect.y0 < 100.0);
        assert!((out[0].rect.x0 - 90.0).abs() < 1.0 && (out[0].rect.x1 - 300.0).abs() < 1.0);
        // linha curta (sublinhado de uma palavra) não vira campo
        assert!(detect_candidates(&[line(90.0, 110.0, 110.0)], &[], &[]).is_empty());
    }

    #[test]
    fn underscores_become_a_field() {
        let words = [word(50.0, 200.0, "Cidade"), word(85.0, 200.0, "__________")];
        let out = detect_candidates(&[], &words, &[]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].name.as_deref(), Some("Cidade"));
        assert!(out[0].rect.x0 >= 84.0);
    }

    #[test]
    fn small_square_becomes_checkbox() {
        let words = [word(70.0, 302.0, "Sim")];
        let out = detect_candidates(
            &[Shape::Rect(PdfRect::new(50.0, 300.0, 62.0, 312.0))],
            &words,
            &[],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, NewFieldKind::Checkbox);
        assert_eq!(
            out[0].name.as_deref(),
            Some("Sim"),
            "o rótulo da caixa fica à direita"
        );
    }

    #[test]
    fn box_with_text_is_not_a_field() {
        let cell = PdfRect::new(50.0, 400.0, 250.0, 420.0);
        let inside = [word(60.0, 405.0, "Cabeçalho")];
        assert!(detect_candidates(&[Shape::Rect(cell)], &inside, &[]).is_empty());
        let out = detect_candidates(&[Shape::Rect(cell)], &[], &[]);
        assert_eq!(out.len(), 1, "célula vazia vira campo");
        assert_eq!(out[0].kind, NewFieldKind::Text);
        // retângulo grande (moldura da página) não
        let frame = PdfRect::new(20.0, 20.0, 580.0, 780.0);
        assert!(detect_candidates(&[Shape::Rect(frame)], &[], &[]).is_empty());
    }

    #[test]
    fn signature_and_date_labels() {
        let words = [
            word(50.0, 500.0, "Assinatura:"),
            word(50.0, 550.0, "Data"),
            word(75.0, 550.0, "de"),
            word(90.0, 550.0, "nascimento:"),
        ];
        let out = detect_candidates(
            &[line(120.0, 300.0, 510.0), line(160.0, 300.0, 560.0)],
            &words,
            &[],
        );
        assert_eq!(out.len(), 2, "{out:?}");
        let by = |n: &str| out.iter().find(|c| c.name.as_deref() == Some(n)).unwrap();
        assert_eq!(by("Assinatura").kind, NewFieldKind::Signature);
        assert_eq!(by("Data de nascimento").kind, NewFieldKind::Date);
    }

    #[test]
    fn no_overlap_with_existing() {
        let existing = [PdfRect::new(90.0, 92.0, 300.0, 110.0)];
        assert!(detect_candidates(&[line(90.0, 300.0, 110.0)], &[], &existing).is_empty());
        // dois traços no mesmo lugar (borda de caixa e linha) dão um campo só
        let cell = PdfRect::new(50.0, 400.0, 250.0, 420.0);
        let out = detect_candidates(&[Shape::Rect(cell), line(50.0, 250.0, 420.0)], &[], &[]);
        assert_eq!(out.len(), 1);
    }
}
