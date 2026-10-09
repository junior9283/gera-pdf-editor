//! Impressão: opções do diálogo (no estilo do Acrobat) e os argumentos do `lp` (CUPS).
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Pages {
    #[default]
    All,
    Current,
    Ranges(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Subset {
    #[default]
    All,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sizing {
    #[default]
    Fit,
    Actual,
    ShrinkLarge,
    Custom(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Duplex {
    #[default]
    None,
    LongEdge,
    ShortEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Auto,
    Portrait,
    Landscape,
}

impl Subset {
    pub const ALL: [Self; 3] = [Self::All, Self::Odd, Self::Even];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Todas as páginas do intervalo",
            Self::Odd => "Só ímpares",
            Self::Even => "Só pares",
        }
    }
}

impl Duplex {
    pub const ALL: [Self; 3] = [Self::None, Self::LongEdge, Self::ShortEdge];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Só frente",
            Self::LongEdge => "Frente e verso (borda longa)",
            Self::ShortEdge => "Frente e verso (borda curta)",
        }
    }
}

impl Orientation {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Portrait, Self::Landscape];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automática",
            Self::Portrait => "Retrato",
            Self::Landscape => "Paisagem",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        Self {
            printer: String::new(),
            copies: 1,
            grayscale: false,
            pages: Pages::All,
            subset: Subset::All,
            reverse: false,
            sizing: Sizing::Fit,
            per_sheet: 1,
            duplex: Duplex::None,
            orientation: Orientation::Auto,
            with_annots: true,
        }
    }
}

const RANGE_HINT: &str = "Use números de página, como 1-3, 7.";

/// "1-3, 7" → [(1, 3), (7, 7)], com páginas de 1 a `total`.
pub fn parse_ranges(text: &str, total: usize) -> Result<Vec<(usize, usize)>, String> {
    let num = |s: &str| {
        s.trim()
            .parse::<usize>()
            .map_err(|_| RANGE_HINT.to_string())
    };
    let mut out = Vec::new();
    for part in text.split(',') {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(part)?;
                (n, n)
            }
        };
        if a == 0 {
            return Err("A primeira página é a 1.".into());
        }
        if a > b {
            return Err(format!("Intervalo invertido: {a}-{b}."));
        }
        if b > total {
            return Err(format!("O documento tem {total} páginas."));
        }
        out.push((a, b));
    }
    Ok(out)
}

/// Argumentos do `lp` para imprimir `file` com as opções `o`. `current` é a página atual (1-based);
/// `oversized` diz se alguma página é maior que o papel (para "Reduzir páginas grandes").
pub fn lp_args(
    o: &PrintOptions,
    current: usize,
    total: usize,
    oversized: bool,
    file: &Path,
) -> Result<Vec<String>, String> {
    if o.printer.is_empty() {
        return Err("Nenhuma impressora escolhida.".into());
    }
    let mut opts: Vec<String> = Vec::new();
    match &o.pages {
        Pages::All => {}
        Pages::Current => opts.push(format!("page-ranges={current}")),
        Pages::Ranges(text) => {
            let list = parse_ranges(text, total)?
                .iter()
                .map(|&(x, y)| {
                    if x == y {
                        x.to_string()
                    } else {
                        format!("{x}-{y}")
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            opts.push(format!("page-ranges={list}"));
        }
    }
    match o.subset {
        Subset::All => {}
        Subset::Odd => opts.push("page-set=odd".into()),
        Subset::Even => opts.push("page-set=even".into()),
    }
    if o.reverse {
        opts.push("outputorder=reverse".into());
    }
    match o.sizing {
        Sizing::Fit => opts.push("fit-to-page".into()),
        Sizing::ShrinkLarge if oversized => opts.push("fit-to-page".into()),
        Sizing::Custom(pct) => opts.push(format!("scaling={pct}")),
        Sizing::Actual | Sizing::ShrinkLarge => {}
    }
    if o.per_sheet > 1 {
        opts.push(format!("number-up={}", o.per_sheet));
    }
    match o.duplex {
        Duplex::None => {}
        Duplex::LongEdge => opts.push("sides=two-sided-long-edge".into()),
        Duplex::ShortEdge => opts.push("sides=two-sided-short-edge".into()),
    }
    match o.orientation {
        Orientation::Auto => {}
        Orientation::Portrait => opts.push("orientation-requested=3".into()),
        Orientation::Landscape => opts.push("orientation-requested=4".into()),
    }
    if o.grayscale {
        opts.push("print-color-mode=monochrome".into());
    }
    let mut args = vec![
        "-d".to_string(),
        o.printer.clone(),
        "-n".to_string(),
        o.copies.max(1).to_string(),
    ];
    for opt in opts {
        args.push("-o".into());
        args.push(opt);
    }
    args.push(file.display().to_string());
    Ok(args)
}

#[cfg(test)]
mod tests {
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
        for (text, total) in [
            ("", 10),
            ("abc", 10),
            ("0", 10),
            ("5-2", 10),
            ("11", 10),
            ("1-", 10),
            ("1,,2", 10),
        ] {
            let err = parse_ranges(text, total).unwrap_err();
            assert!(!err.is_empty(), "{text:?}");
        }
        assert!(
            parse_ranges("11", 10).unwrap_err().contains("10"),
            "diz quantas páginas há"
        );
    }

    fn opts() -> PrintOptions {
        PrintOptions {
            printer: "HP".into(),
            ..Default::default()
        }
    }

    fn args(o: &PrintOptions) -> Vec<String> {
        lp_args(o, 4, 10, false, Path::new("/tmp/x.pdf")).unwrap()
    }

    #[test]
    fn defaults_print_everything_fitted_on_the_chosen_printer() {
        assert_eq!(
            args(&opts()),
            ["-d", "HP", "-n", "1", "-o", "fit-to-page", "/tmp/x.pdf"]
        );
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
            "-n 3",
            "-o page-ranges=1-3,7",
            "-o page-set=odd",
            "-o outputorder=reverse",
            "-o scaling=80",
            "-o number-up=4",
            "-o sides=two-sided-long-edge",
            "-o orientation-requested=4",
            "-o print-color-mode=monochrome",
        ] {
            assert!(a.contains(part), "{part} em {a}");
        }
        assert!(!a.contains("fit-to-page"));
        assert!(a.ends_with("/tmp/x.pdf"));
    }

    #[test]
    fn current_page_actual_size_and_shrink_large() {
        let cur = PrintOptions {
            pages: Pages::Current,
            sizing: Sizing::Actual,
            ..opts()
        };
        let a = args(&cur).join(" ");
        assert!(
            a.contains("-o page-ranges=4") && !a.contains("fit-to-page"),
            "{a}"
        );
        let shrink = PrintOptions {
            sizing: Sizing::ShrinkLarge,
            ..opts()
        };
        assert!(
            !args(&shrink).join(" ").contains("fit-to-page"),
            "nada a reduzir"
        );
        let big = lp_args(&shrink, 1, 10, true, Path::new("/tmp/x.pdf"))
            .unwrap()
            .join(" ");
        assert!(
            big.contains("fit-to-page"),
            "página maior que o papel é reduzida"
        );
    }

    #[test]
    fn invalid_ranges_or_missing_printer_are_errors() {
        let bad = PrintOptions {
            pages: Pages::Ranges("0".into()),
            ..opts()
        };
        assert!(lp_args(&bad, 1, 10, false, Path::new("/tmp/x.pdf")).is_err());
        let none = PrintOptions::default();
        assert!(lp_args(&none, 1, 10, false, Path::new("/tmp/x.pdf")).is_err());
    }
}
