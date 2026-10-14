//! Montagem das folhas para imprimir quando o próprio app desenha cada folha (Windows): quais
//! páginas, em que ordem, quantas por folha, a orientação do papel e onde cada página fica. No
//! Linux, o CUPS faz isso a partir dos argumentos do `lp`.
use crate::print::{parse_ranges, Orientation, Pages, PrintOptions, Sizing, Subset};

/// Retângulo em pontos (1/72 pol.) no papel, com a origem no canto superior esquerdo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Uma página na folha: índice (0-based) e onde desenhá-la (já escalada e centrada).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub page: usize,
    pub at: Area,
}

/// As páginas a imprimir (0-based), na ordem de impressão.
pub fn page_list(o: &PrintOptions, current: usize, total: usize) -> Result<Vec<usize>, String> {
    let mut pages: Vec<usize> = match o.pages {
        Pages::All => (1..=total).collect(),
        Pages::Current => vec![current.clamp(1, total.max(1))],
        Pages::Ranges => parse_ranges(&o.ranges, total)?
            .into_iter()
            .flat_map(|(a, b)| a..=b)
            .collect(),
    };
    pages.retain(|n| match o.subset {
        Subset::All => true,
        Subset::Odd => n % 2 == 1,
        Subset::Even => n % 2 == 0,
    });
    if pages.is_empty() {
        return Err("Nenhuma página a imprimir.".into());
    }
    if o.reverse {
        pages.reverse();
    }
    Ok(pages.into_iter().map(|n| n - 1).collect())
}

/// Colunas e linhas de `per_sheet` páginas no papel deitado (paisagem) ou em pé.
fn grid(per_sheet: u8, landscape: bool) -> (usize, usize) {
    let (long, short) = match per_sheet {
        2 => (2, 1),
        4 => (2, 2),
        6 => (3, 2),
        9 => (3, 3),
        _ => (1, 1),
    };
    if landscape {
        (long, short)
    } else {
        (short, long)
    }
}

/// O papel vai deitado? Automática: como a primeira página (deitada se for mais larga), e com 2
/// ou 6 por folha, o contrário dela (duas páginas em pé lado a lado pedem o papel deitado).
pub fn landscape(o: &PrintOptions, first: (f32, f32)) -> bool {
    match o.orientation {
        Orientation::Portrait => false,
        Orientation::Landscape => true,
        Orientation::Auto => {
            let wide = first.0 > first.1;
            if matches!(o.per_sheet, 2 | 6) {
                !wide
            } else {
                wide
            }
        }
    }
}

/// As folhas: cada uma com as páginas e onde ficam dentro de `area` (a parte imprimível do papel,
/// já na orientação escolhida). `sizes` são os tamanhos das páginas em pontos.
pub fn sheets(
    o: &PrintOptions,
    pages: &[usize],
    sizes: &[(f32, f32)],
    area: Area,
    landscape: bool,
) -> Vec<Vec<Placement>> {
    let (cols, rows) = grid(o.per_sheet, landscape);
    let per = cols * rows;
    let (cw, ch) = (area.w / cols as f32, area.h / rows as f32);
    pages
        .chunks(per)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .map(|(k, &page)| {
                    let (pw, ph) = sizes.get(page).copied().unwrap_or((612.0, 792.0));
                    let fit = (cw / pw.max(1.0)).min(ch / ph.max(1.0));
                    // várias por folha: sempre ajustadas à célula (como o Acrobat)
                    let scale = if per > 1 {
                        fit
                    } else {
                        match o.sizing {
                            Sizing::Fit => fit,
                            Sizing::Actual => 1.0,
                            Sizing::ShrinkLarge => fit.min(1.0),
                            Sizing::Custom(pct) => f32::from(pct.clamp(1, 1000)) / 100.0,
                        }
                    };
                    let (w, h) = (pw * scale, ph * scale);
                    let (col, row) = (k % cols, k / cols);
                    Placement {
                        page,
                        at: Area {
                            x: area.x + col as f32 * cw + (cw - w) / 2.0,
                            y: area.y + row as f32 * ch + (ch - h) / 2.0,
                            w,
                            h,
                        },
                    }
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: (f32, f32) = (612.0, 792.0);
    const A4_AREA: Area = Area {
        x: 0.0,
        y: 0.0,
        w: 595.0,
        h: 842.0,
    };

    fn opts() -> PrintOptions {
        PrintOptions {
            printer: "X".into(),
            ..Default::default()
        }
    }

    #[test]
    fn one_per_sheet_fit_shrink_actual() {
        let mut o = opts();
        let small = (300.0, 400.0);
        let one = |o: &PrintOptions, size| sheets(o, &[0], &[size], A4_AREA, false)[0][0].at;
        // Ajustar amplia a página pequena até a largura do papel, centrada
        let fit = one(&o, small);
        assert!(
            (fit.w - 595.0).abs() < 0.5 && (fit.h - 793.3).abs() < 0.5,
            "{fit:?}"
        );
        assert!((fit.y - (842.0 - fit.h) / 2.0).abs() < 0.01);
        // Tamanho real: como está, centrada
        o.sizing = Sizing::Actual;
        let actual = one(&o, small);
        assert_eq!((actual.w, actual.h), small);
        assert_eq!(
            (actual.x, actual.y),
            ((595.0 - 300.0) / 2.0, (842.0 - 400.0) / 2.0)
        );
        // Reduzir páginas grandes: a pequena fica como está, a grande encolhe
        o.sizing = Sizing::ShrinkLarge;
        assert_eq!(one(&o, small).w, 300.0);
        assert!(one(&o, (1190.0, 1684.0)).w <= 595.0);
        // Personalizado
        o.sizing = Sizing::Custom(50);
        assert_eq!(one(&o, LETTER).w, 306.0);
    }

    #[test]
    fn n_up_grid_and_orientation() {
        let mut o = opts();
        o.per_sheet = 2;
        // duas páginas em pé: papel deitado, lado a lado
        assert!(landscape(&o, LETTER));
        let area = Area {
            x: 0.0,
            y: 0.0,
            w: 842.0,
            h: 595.0,
        };
        let s = sheets(&o, &[0, 1, 2], &[LETTER; 3], area, true);
        assert_eq!(s.len(), 2, "3 páginas, 2 por folha");
        assert!(s[0][0].at.x < s[0][1].at.x && s[0][0].at.y == s[0][1].at.y);
        assert!(s[0][1].at.x + s[0][1].at.w <= 842.0 + 0.01);
        o.per_sheet = 4;
        assert!(!landscape(&o, LETTER));
        let s = sheets(&o, &[0, 1, 2, 3], &[LETTER; 4], A4_AREA, false);
        let at: Vec<_> = s[0]
            .iter()
            .map(|p| (p.at.x < 297.0, p.at.y < 421.0))
            .collect();
        assert_eq!(
            at,
            [(true, true), (false, true), (true, false), (false, false)]
        );
        o.per_sheet = 6;
        assert_eq!(grid(6, false), (2, 3));
        o.per_sheet = 1;
        assert!(
            landscape(&o, (792.0, 612.0)),
            "página deitada, papel deitado"
        );
        o.orientation = Orientation::Portrait;
        assert!(!landscape(&o, (792.0, 612.0)));
    }

    #[test]
    fn reverse_and_ranges() {
        let mut o = opts();
        assert_eq!(page_list(&o, 1, 3).unwrap(), [0, 1, 2]);
        o.pages = Pages::Current;
        assert_eq!(page_list(&o, 2, 3).unwrap(), [1]);
        o.pages = Pages::Ranges;
        o.ranges = "1-3, 7".into();
        o.reverse = true;
        assert_eq!(page_list(&o, 1, 9).unwrap(), [6, 2, 1, 0]);
        o.reverse = false;
        o.subset = Subset::Even;
        assert_eq!(page_list(&o, 1, 9).unwrap(), [1]);
        o.ranges = "1".into();
        assert!(
            page_list(&o, 1, 9).is_err(),
            "nada sobra: só páginas pares de 1"
        );
        o.ranges = "abc".into();
        assert!(page_list(&o, 1, 9).is_err());
    }
}
