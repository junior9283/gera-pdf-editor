//! Rótulos de página (`/PageLabels`): numeração lógica como "i, ii, iii" no prefácio e "1, 2, 3"
//! depois, como no diálogo Rótulos de página do Acrobat.

/// Maior rótulo (o prefixo vem do arquivo e se repete em cada página).
pub const MAX_LABEL_CHARS: usize = 64;

/// Estilo da numeração de uma faixa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLabelStyle {
    /// Só o prefixo.
    None,
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperAlpha,
    LowerAlpha,
}

/// Faixa de rótulos: da página `start_page` (a partir de 0) até a próxima faixa, numerando a partir
/// de `first`.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLabelRange {
    pub start_page: i32,
    pub style: PageLabelStyle,
    pub prefix: String,
    pub first: i32,
}

fn roman(mut n: i32) -> String {
    const TABLE: [(i32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, digits) in TABLE {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

/// Letras como no PDF: a…z, depois aa…zz, aaa… (a letra repetida).
fn letters(n: i32) -> String {
    let n = (n.max(1) - 1) as usize;
    let letter = (b'A' + (n % 26) as u8) as char;
    // teto: um número enorme não vira um texto enorme
    letter.to_string().repeat((n / 26 + 1).min(64))
}

/// Rótulo da página de número `n` (1, 2, 3… dentro da faixa).
pub fn format_label(style: PageLabelStyle, prefix: &str, n: i32) -> String {
    let number = match style {
        PageLabelStyle::None => String::new(),
        PageLabelStyle::Decimal => n.to_string(),
        PageLabelStyle::UpperRoman | PageLabelStyle::LowerRoman if !(1..4000).contains(&n) => {
            n.to_string()
        }
        PageLabelStyle::UpperRoman => roman(n),
        PageLabelStyle::LowerRoman => roman(n).to_lowercase(),
        PageLabelStyle::UpperAlpha => letters(n),
        PageLabelStyle::LowerAlpha => letters(n).to_lowercase(),
    };
    prefix
        .chars()
        .chain(number.chars())
        .take(MAX_LABEL_CHARS)
        .collect()
}

/// Faixa que vale na página `page` (a última que começa nela ou antes).
fn range_at(ranges: &[PageLabelRange], page: i32) -> Option<&PageLabelRange> {
    ranges.iter().rev().find(|r| r.start_page <= page)
}

/// Rótulo de cada uma das `total` páginas (vazio se não houver faixas). As faixas vêm em ordem:
/// páginas e faixas andam juntas (sem buscar a faixa de cada página).
pub fn label_list(ranges: &[PageLabelRange], total: i32) -> Vec<String> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut next = 0;
    let mut current: Option<&PageLabelRange> = None;
    (0..total.max(0))
        .map(|p| {
            while ranges.get(next).is_some_and(|r| r.start_page <= p) {
                current = ranges.get(next);
                next += 1;
            }
            match current {
                Some(r) => format_label(r.style, &r.prefix, r.first + (p - r.start_page)),
                None => (p + 1).to_string(),
            }
        })
        .collect()
}

/// Faixas em ordem, sem repetir início, começando na página 0 (antes da primeira faixa: 1, 2, 3).
fn normalized(mut ranges: Vec<PageLabelRange>, total: i32) -> Vec<PageLabelRange> {
    ranges.retain(|r| (0..total).contains(&r.start_page));
    ranges.sort_by_key(|r| r.start_page);
    // mesma página com duas faixas: vale a última
    ranges.reverse();
    ranges.dedup_by_key(|r| r.start_page);
    ranges.reverse();
    if ranges.first().is_some_and(|r| r.start_page > 0) {
        ranges.insert(
            0,
            PageLabelRange {
                start_page: 0,
                style: PageLabelStyle::Decimal,
                prefix: String::new(),
                first: 1,
            },
        );
    }
    ranges
}

/// Aplica o diálogo Rótulos de página às páginas `from..=to`: `new` começa uma seção nova (o
/// `start_page` dela é ignorado); `None` estende a seção anterior. A página depois de `to`
/// continua com o rótulo que tinha.
pub fn apply_labels(
    ranges: &[PageLabelRange],
    from: i32,
    to: i32,
    new: Option<PageLabelRange>,
    total: i32,
) -> Vec<PageLabelRange> {
    let (from, to) = (from.clamp(0, total - 1), to.clamp(0, total - 1));
    let after = to + 1;
    // o que a página seguinte tinha, para continuar igual
    let keep_after = (after < total)
        .then(|| range_at(ranges, after))
        .flatten()
        .map(|r| PageLabelRange {
            start_page: after,
            first: r.first + (after - r.start_page),
            ..r.clone()
        });
    let keep_after = keep_after.or_else(|| {
        (after < total && ranges.is_empty() && new.is_some()).then(|| PageLabelRange {
            start_page: after,
            style: PageLabelStyle::Decimal,
            prefix: String::new(),
            first: after + 1,
        })
    });
    let mut out: Vec<PageLabelRange> = ranges
        .iter()
        .filter(|r| r.start_page < from || r.start_page > to)
        .cloned()
        .collect();
    if let Some(n) = new {
        out.push(PageLabelRange {
            start_page: from,
            first: n.first.max(1),
            ..n
        });
    }
    if let Some(k) = keep_after {
        if !out.iter().any(|r| r.start_page == after) {
            out.push(k);
        }
    }
    normalized(out, total)
}

use crate::doc::PdfDoc;
use crate::error::Result;
use crate::pdf_obj::{get, name_of, text};
use mupdf::pdf::PdfObject;

/// Tetos da leitura da árvore numérica.
const MAX_RANGES: usize = 10_000;
const MAX_DEPTH: usize = 32;

fn style_of(dict: &PdfObject) -> PageLabelStyle {
    match get(dict, "S").and_then(|s| name_of(&s)).as_deref() {
        Some("D") => PageLabelStyle::Decimal,
        Some("R") => PageLabelStyle::UpperRoman,
        Some("r") => PageLabelStyle::LowerRoman,
        Some("A") => PageLabelStyle::UpperAlpha,
        Some("a") => PageLabelStyle::LowerAlpha,
        _ => PageLabelStyle::None,
    }
}

fn style_name(style: PageLabelStyle) -> Option<&'static str> {
    match style {
        PageLabelStyle::None => None,
        PageLabelStyle::Decimal => Some("D"),
        PageLabelStyle::UpperRoman => Some("R"),
        PageLabelStyle::LowerRoman => Some("r"),
        PageLabelStyle::UpperAlpha => Some("A"),
        PageLabelStyle::LowerAlpha => Some("a"),
    }
}

impl PdfDoc {
    /// Faixas de rótulos do documento, em ordem (vazio sem `/PageLabels`).
    pub fn page_labels(&self) -> Vec<PageLabelRange> {
        let Some(root) = self
            .inner
            .catalog()
            .ok()
            .and_then(|c| get(&c, "PageLabels"))
        else {
            return Vec::new();
        };
        let out = crate::pdf_obj::tree_pairs(&root, "Nums", MAX_RANGES, MAX_DEPTH)
            .into_iter()
            .filter_map(|(page, dict)| {
                let first = get(&dict, "St")
                    .and_then(|v| v.as_int().ok())
                    .filter(|v| *v >= 1)
                    .unwrap_or(1);
                Some(PageLabelRange {
                    start_page: page.as_int().ok()?,
                    style: style_of(&dict),
                    prefix: text(&dict, "P").chars().take(MAX_LABEL_CHARS).collect(),
                    first,
                })
            })
            .collect();
        normalized(out, self.page_count().unwrap_or(0))
    }

    /// Rótulo de cada página (vazio sem `/PageLabels`).
    pub fn labels(&self) -> Vec<String> {
        label_list(&self.page_labels(), self.page_count().unwrap_or(0))
    }

    /// Grava as faixas (uma etapa do desfazer); lista vazia tira os rótulos.
    pub fn set_page_labels(&mut self, ranges: Vec<PageLabelRange>) -> Result<()> {
        self.require_modifiable("alterar os rótulos de página")?;
        self.with_snapshot(|d| d.write_page_labels(ranges))
    }

    /// Diálogo Rótulos de página: as páginas `from..=to` começam uma seção nova (`new`) ou estendem
    /// a anterior (`None`); as demais faixas continuam (uma etapa do desfazer).
    pub fn apply_page_labels(
        &mut self,
        from: i32,
        to: i32,
        new: Option<PageLabelRange>,
    ) -> Result<()> {
        let total = self.page_count()?;
        let ranges = apply_labels(&self.page_labels(), from, to, new, total);
        self.set_page_labels(ranges)
    }

    /// Grava as faixas sem histórico (as operações de página põem de volta as que havia: o MuPDF
    /// ajusta e até cria rótulos ao mover, inserir e excluir, e o Acrobat não).
    pub(crate) fn write_page_labels(&mut self, ranges: Vec<PageLabelRange>) -> Result<()> {
        let ranges = normalized(ranges, self.page_count()?);
        let mut catalog = self.inner.catalog()?;
        if ranges.is_empty() {
            catalog.dict_delete("PageLabels")?;
            return Ok(());
        }
        let mut nums = self.inner.new_array()?;
        for r in &ranges {
            let mut dict = self.inner.new_dict()?;
            if let Some(s) = style_name(r.style) {
                dict.dict_put("S", PdfObject::new_name(s)?)?;
            }
            if !r.prefix.is_empty() {
                dict.dict_put("P", PdfObject::new_string(&r.prefix)?)?;
            }
            if r.first != 1 {
                dict.dict_put("St", PdfObject::new_int(r.first)?)?;
            }
            nums.array_push(PdfObject::new_int(r.start_page)?)?;
            nums.array_push(dict)?;
        }
        let mut tree = self.inner.new_dict()?;
        tree.dict_put("Nums", nums)?;
        let tree = self.inner.add_object(&tree)?;
        catalog.dict_put("PageLabels", tree)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_numerals() {
        let r = |n| format_label(PageLabelStyle::UpperRoman, "", n);
        assert_eq!(
            [1, 4, 9, 14, 40, 90, 400, 3999].map(r),
            ["I", "IV", "IX", "XIV", "XL", "XC", "CD", "MMMCMXCIX"]
        );
        assert_eq!(r(4000), "4000", "acima de 3999 vira decimal");
        assert_eq!(format_label(PageLabelStyle::LowerRoman, "p-", 3), "p-iii");
    }

    #[test]
    fn a_huge_prefix_does_not_multiply_by_the_page_count() {
        let big = PageLabelRange {
            start_page: 0,
            style: PageLabelStyle::Decimal,
            prefix: "x".repeat(1 << 20),
            first: 1,
        };
        let labels = label_list(&[big], 1000);
        assert!(labels.iter().all(|l| l.chars().count() <= MAX_LABEL_CHARS));
    }

    #[test]
    fn letters_repeat_after_z() {
        let a = |n| format_label(PageLabelStyle::LowerAlpha, "", n);
        assert_eq!([1, 26, 27, 52, 53].map(a), ["a", "z", "aa", "zz", "aaa"]);
        assert_eq!(format_label(PageLabelStyle::UpperAlpha, "", 2), "B");
        assert_eq!(format_label(PageLabelStyle::None, "Capa", 1), "Capa");
        assert_eq!(format_label(PageLabelStyle::Decimal, "", 7), "7");
    }

    fn range(start_page: i32, style: PageLabelStyle, first: i32) -> PageLabelRange {
        PageLabelRange {
            start_page,
            style,
            prefix: String::new(),
            first,
        }
    }

    #[test]
    fn a_new_section_in_the_middle_keeps_what_follows() {
        use PageLabelStyle::*;
        let old = vec![range(0, LowerRoman, 1), range(4, Decimal, 1)];
        let new = apply_labels(&old, 2, 2, Some(range(0, UpperAlpha, 1)), 10);
        assert_eq!(
            label_list(&new, 10),
            ["i", "ii", "A", "iv", "1", "2", "3", "4", "5", "6"]
        );
    }

    #[test]
    fn extending_continues_the_previous_section() {
        use PageLabelStyle::*;
        let old = vec![range(0, LowerRoman, 1), range(4, Decimal, 1)];
        // páginas 5 a 7 (índices 4..=6) estendem os romanos; a 8 continua "4"
        let new = apply_labels(&old, 4, 6, Option::None, 10);
        assert_eq!(
            label_list(&new, 10),
            ["i", "ii", "iii", "iv", "v", "vi", "vii", "4", "5", "6"]
        );
    }

    #[test]
    fn a_first_section_not_at_page_one_gets_decimal_before_it() {
        let new = apply_labels(&[], 3, 9, Some(range(0, PageLabelStyle::LowerRoman, 1)), 10);
        assert_eq!(new[0], range(0, PageLabelStyle::Decimal, 1));
        assert_eq!(label_list(&new, 10)[..4], ["1", "2", "3", "i"]);
    }
}
