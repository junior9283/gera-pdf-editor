//! Organizar campos no Preparar formulário (como no Acrobat): alinhar pela âncora, centralizar na
//! página, distribuir, igualar o tamanho, mover com as setas, grade de cópias e guias. Funções
//! puras: recebem os retângulos (coordenadas do MuPDF, y para baixo) e devolvem os novos.
use pdf_core::PdfRect;

/// Alinhar: as bordas, ou os centros numa linha horizontal (`CenterH`) ou vertical (`CenterV`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    CenterH,
    CenterV,
}

/// Direção: horizontal, vertical ou ambas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    H,
    V,
    Both,
}

/// Igualar a largura, a altura ou ambas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeKind {
    Width,
    Height,
    Both,
}

/// Linha de guia: vertical (x) ou horizontal (y), em coordenadas da página.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Guide {
    X(f32),
    Y(f32),
}

/// Teto de "Criar várias cópias": 20 linhas × 20 colunas.
const MAX_GRID: usize = 20;

fn center(r: &PdfRect) -> (f32, f32) {
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

/// Alinha os campos pela âncora (`rects[anchor]`, que não se move).
pub fn align(rects: &[PdfRect], anchor: usize, edge: Edge) -> Vec<PdfRect> {
    let Some(a) = rects.get(anchor).copied() else {
        return rects.to_vec();
    };
    let (acx, acy) = center(&a);
    rects
        .iter()
        .map(|r| {
            let (cx, cy) = center(r);
            match edge {
                Edge::Left => r.translated(a.x0 - r.x0, 0.0),
                Edge::Right => r.translated(a.x1 - r.x1, 0.0),
                Edge::Top => r.translated(0.0, a.y0 - r.y0),
                Edge::Bottom => r.translated(0.0, a.y1 - r.y1),
                Edge::CenterH => r.translated(0.0, acy - cy),
                Edge::CenterV => r.translated(acx - cx, 0.0),
            }
        })
        .collect()
}

/// Caixa que envolve todos os retângulos.
fn bounds(rects: &[PdfRect]) -> Option<PdfRect> {
    rects.iter().copied().reduce(|a, b| {
        PdfRect::new(
            a.x0.min(b.x0),
            a.y0.min(b.y0),
            a.x1.max(b.x1),
            a.y1.max(b.y1),
        )
    })
}

/// Centraliza o grupo na página (as distâncias entre os campos ficam).
pub fn center_on_page(rects: &[PdfRect], page: PdfRect, axis: Axis) -> Vec<PdfRect> {
    let Some(b) = bounds(rects) else {
        return Vec::new();
    };
    let (bx, by) = center(&b);
    let (px, py) = center(&page);
    let dx = if axis == Axis::V { 0.0 } else { px - bx };
    let dy = if axis == Axis::H { 0.0 } else { py - by };
    nudge(rects, dx, dy)
}

/// Distribui os centros a distâncias iguais entre o primeiro e o último (pela posição). Com
/// menos de 3 campos, nada muda. `Both` distribui nas duas direções.
pub fn distribute(rects: &[PdfRect], axis: Axis) -> Vec<PdfRect> {
    let mut out = rects.to_vec();
    if rects.len() < 3 {
        return out;
    }
    for horizontal in [true, false] {
        if (horizontal && axis == Axis::V) || (!horizontal && axis == Axis::H) {
            continue;
        }
        let pos = |r: &PdfRect| if horizontal { center(r).0 } else { center(r).1 };
        let mut order: Vec<usize> = (0..out.len()).collect();
        order.sort_by(|&a, &b| pos(&out[a]).total_cmp(&pos(&out[b])));
        let (first, last) = (pos(&out[order[0]]), pos(&out[order[order.len() - 1]]));
        let step = (last - first) / (order.len() - 1) as f32;
        for (k, &i) in order.iter().enumerate() {
            let d = first + step * k as f32 - pos(&out[i]);
            out[i] = if horizontal {
                out[i].translated(d, 0.0)
            } else {
                out[i].translated(0.0, d)
            };
        }
    }
    out
}

/// Dá a largura e/ou a altura da âncora a todos, mantendo o canto superior esquerdo.
pub fn match_size(rects: &[PdfRect], anchor: usize, kind: SizeKind) -> Vec<PdfRect> {
    let Some(a) = rects.get(anchor).copied() else {
        return rects.to_vec();
    };
    rects
        .iter()
        .map(|r| {
            let w = if kind == SizeKind::Height {
                r.width()
            } else {
                a.width()
            };
            let h = if kind == SizeKind::Width {
                r.height()
            } else {
                a.height()
            };
            PdfRect::new(r.x0, r.y0, r.x0 + w, r.y0 + h)
        })
        .collect()
}

/// Move todos (setas: 1 ponto; com Shift, 10).
pub fn nudge(rects: &[PdfRect], dx: f32, dy: f32) -> Vec<PdfRect> {
    rects.iter().map(|r| r.translated(dx, dy)).collect()
}

/// As cópias de "Criar várias cópias": `rows` linhas × `cols` colunas a partir do original (que
/// fica de fora), com `gap` entre elas. No máximo 20 × 20.
pub fn copies_grid(rect: PdfRect, rows: usize, cols: usize, gap: (f32, f32)) -> Vec<PdfRect> {
    let (rows, cols) = (rows.clamp(1, MAX_GRID), cols.clamp(1, MAX_GRID));
    let (sx, sy) = (rect.width() + gap.0, rect.height() + gap.1);
    (0..rows)
        .flat_map(|r| (0..cols).map(move |c| (r, c)))
        .filter(|&(r, c)| (r, c) != (0, 0))
        .map(|(r, c)| rect.translated(c as f32 * sx, r as f32 * sy))
        .collect()
}

/// Guias ao arrastar: se uma borda ou o centro de `moving` está a até `tol` de uma borda ou centro
/// de outro campo, encaixa nela. Devolve o retângulo e as linhas a desenhar (uma por direção).
pub fn snap(moving: PdfRect, others: &[PdfRect], tol: f32) -> (PdfRect, Vec<Guide>) {
    let xs = |r: &PdfRect| [r.x0, (r.x0 + r.x1) / 2.0, r.x1];
    let ys = |r: &PdfRect| [r.y0, (r.y0 + r.y1) / 2.0, r.y1];
    // a menor diferença (linha do outro − linha do que se move) dentro da tolerância
    let best = |mine: [f32; 3], lines: &dyn Fn(&PdfRect) -> [f32; 3]| {
        let mut found: Option<(f32, f32)> = None;
        for o in others {
            for line in lines(o) {
                for m in mine {
                    let d = line - m;
                    if d.abs() <= tol && found.is_none_or(|(bd, _)| d.abs() < bd.abs()) {
                        found = Some((d, line));
                    }
                }
            }
        }
        found
    };
    let mut out = moving;
    let mut guides = Vec::new();
    if let Some((d, line)) = best(xs(&moving), &xs) {
        out = out.translated(d, 0.0);
        guides.push(Guide::X(line));
    }
    if let Some((d, line)) = best(ys(&moving), &ys) {
        out = out.translated(0.0, d);
        guides.push(Guide::Y(line));
    }
    (out, guides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> PdfRect {
        PdfRect::new(x0, y0, x1, y1)
    }

    #[test]
    fn align_to_the_anchor() {
        let rects = [r(10.0, 10.0, 50.0, 30.0), r(100.0, 50.0, 120.0, 90.0)];
        assert_eq!(align(&rects, 1, Edge::Left)[0], r(100.0, 10.0, 140.0, 30.0));
        assert_eq!(align(&rects, 1, Edge::Right)[0], r(80.0, 10.0, 120.0, 30.0));
        assert_eq!(align(&rects, 0, Edge::Top)[1], r(100.0, 10.0, 120.0, 50.0));
        assert_eq!(
            align(&rects, 0, Edge::Bottom)[1],
            r(100.0, -10.0, 120.0, 30.0)
        );
        // centros na mesma linha horizontal (y do centro da âncora = 70)
        assert_eq!(
            align(&rects, 1, Edge::CenterH)[0],
            r(10.0, 60.0, 50.0, 80.0)
        );
        // centros na mesma linha vertical (x do centro da âncora = 110)
        assert_eq!(
            align(&rects, 1, Edge::CenterV)[0],
            r(90.0, 10.0, 130.0, 30.0)
        );
        assert_eq!(
            align(&rects, 1, Edge::Left)[1],
            rects[1],
            "a âncora não se move"
        );
    }

    #[test]
    fn center_moves_the_group() {
        let page = r(0.0, 0.0, 600.0, 800.0);
        let rects = [r(10.0, 10.0, 50.0, 30.0), r(90.0, 50.0, 110.0, 90.0)];
        // o grupo vai de x 10 a 110 (100 de largura): fica de 250 a 350
        let h = center_on_page(&rects, page, Axis::H);
        assert_eq!(
            h,
            vec![r(250.0, 10.0, 290.0, 30.0), r(330.0, 50.0, 350.0, 90.0)]
        );
        // e de y 10 a 90 (80 de altura): fica de 360 a 440
        let both = center_on_page(&rects, page, Axis::Both);
        assert_eq!(both[0], r(250.0, 360.0, 290.0, 380.0));
    }

    #[test]
    fn distribute_spaces_centers() {
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(5.0, 0.0, 15.0, 10.0),
            r(90.0, 0.0, 110.0, 10.0),
        ];
        // centros 5, 10 e 100: o do meio vai para 52,5; os da ponta ficam
        let out = distribute(&rects, Axis::H);
        assert_eq!(out[0], rects[0]);
        assert_eq!(out[2], rects[2]);
        assert_eq!(out[1], r(47.5, 0.0, 57.5, 10.0));
        // a ordem é a da posição, não a da seleção
        let shuffled = [rects[2], rects[0], rects[1]];
        assert_eq!(distribute(&shuffled, Axis::H)[2], r(47.5, 0.0, 57.5, 10.0));
        assert_eq!(
            distribute(&rects[..2], Axis::H),
            rects[..2].to_vec(),
            "dois não distribuem"
        );
    }

    #[test]
    fn match_size_keeps_the_top_left() {
        let rects = [r(10.0, 10.0, 50.0, 30.0), r(100.0, 50.0, 120.0, 90.0)];
        assert_eq!(
            match_size(&rects, 1, SizeKind::Width)[0],
            r(10.0, 10.0, 30.0, 30.0)
        );
        assert_eq!(
            match_size(&rects, 1, SizeKind::Height)[0],
            r(10.0, 10.0, 50.0, 50.0)
        );
        assert_eq!(
            match_size(&rects, 1, SizeKind::Both)[0],
            r(10.0, 10.0, 30.0, 50.0)
        );
        assert_eq!(nudge(&rects, 1.0, -10.0)[1], r(101.0, 40.0, 121.0, 80.0));
    }

    #[test]
    fn copies_grid_rows_and_columns() {
        let out = copies_grid(r(0.0, 0.0, 10.0, 5.0), 2, 3, (2.0, 1.0));
        // 2 linhas × 3 colunas, menos o original
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], r(12.0, 0.0, 22.0, 5.0));
        assert_eq!(out[2], r(0.0, 6.0, 10.0, 11.0));
        assert_eq!(out[4], r(24.0, 6.0, 34.0, 11.0));
        assert_eq!(
            copies_grid(r(0.0, 0.0, 1.0, 1.0), 50, 50, (0.0, 0.0)).len(),
            399,
            "teto 20 × 20"
        );
    }

    #[test]
    fn snap_to_a_nearby_edge() {
        let others = [r(100.0, 100.0, 200.0, 120.0)];
        // borda esquerda a 2 pontos da do outro: encaixa
        let (out, guides) = snap(r(102.0, 300.0, 152.0, 320.0), &others, 4.0);
        assert_eq!(out, r(100.0, 300.0, 150.0, 320.0));
        assert_eq!(guides, vec![Guide::X(100.0)]);
        // topo alinhado ao do outro (dy -3)
        let (out, guides) = snap(r(400.0, 103.0, 450.0, 123.0), &others, 4.0);
        assert_eq!(out, r(400.0, 100.0, 450.0, 120.0));
        assert!(guides.contains(&Guide::Y(100.0)));
        // longe: nada muda
        let far = r(500.0, 500.0, 520.0, 510.0);
        assert_eq!(snap(far, &others, 4.0), (far, vec![]));
    }
}
