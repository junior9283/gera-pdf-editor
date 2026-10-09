//! Datas do dicionário Info do PDF (`D:AAAAMMDDhhmmss±hh'mm'`) em formato brasileiro.

/// "D:20240131143005-03'00'" → "31/01/2024 14:30"; parcial mostra o que existe; vazio → "—";
/// qualquer coisa que não seja uma data PDF válida volta como veio.
pub fn format_pdf_date(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return "—".into();
    }
    let digits: String = s
        .strip_prefix("D:")
        .unwrap_or(s)
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let part = |r: std::ops::Range<usize>| digits.get(r).and_then(|v| v.parse::<u32>().ok());
    let Some(year) = part(0..4) else {
        return s.into();
    };
    match (part(4..6), part(6..8)) {
        (None, _) => year.to_string(),
        (Some(m), Some(d)) if (1..=12).contains(&m) && (1..=31).contains(&d) => {
            match (part(8..10), part(10..12)) {
                (Some(h), Some(min)) if h < 24 && min < 60 => {
                    format!("{d:02}/{m:02}/{year} {h:02}:{min:02}")
                }
                _ => format!("{d:02}/{m:02}/{year}"),
            }
        }
        _ => s.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_dates_become_brazilian_format() {
        assert_eq!(
            format_pdf_date("D:20240131143005-03'00'"),
            "31/01/2024 14:30"
        );
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
        assert_eq!(
            format_pdf_date("D:20241399"),
            "D:20241399",
            "mês 13 é inválido"
        );
    }
}
