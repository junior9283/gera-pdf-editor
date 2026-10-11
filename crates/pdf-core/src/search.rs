//! Localizar com as opções do Acrobat: palavras inteiras, maiúsculas, marcadores e comentários.
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::types::{PdfRect, SearchOptions};
use regex::Regex;

/// Regex literal do texto procurado, com as opções. `None` para texto vazio.
fn regex_for(needle: &str, opts: &SearchOptions) -> Option<Regex> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let mut pattern = regex::escape(needle);
    if opts.whole_words {
        pattern = format!(r"\b{pattern}\b");
    }
    if !opts.match_case {
        pattern = format!("(?i){pattern}");
    }
    Regex::new(&pattern).ok()
}

impl PdfDoc {
    /// Resultados da página: o texto (e, com a opção, os comentários cujo conteúdo casa).
    pub fn search_with(&self, page: i32, needle: &str, opts: &SearchOptions) -> Result<Vec<PdfRect>> {
        let plain = !opts.whole_words && !opts.match_case;
        let mut hits = if plain {
            // o padrão continua sendo a busca do MuPDF (acha frases entre linhas)
            self.search(page, needle)?
        } else {
            match regex_for(needle, opts) {
                Some(re) => self
                    .find_patterns(page, &[(None, re)])?
                    .into_iter()
                    .flat_map(|h| h.rects)
                    .collect(),
                None => Vec::new(),
            }
        };
        if opts.comments {
            if let Some(re) = regex_for(needle, opts) {
                hits.extend(
                    self.annotations(page)?
                        .into_iter()
                        .filter(|a| re.is_match(&a.contents))
                        .map(|a| a.rect),
                );
            }
        }
        Ok(hits)
    }

    /// Marcadores cujo título casa (com a opção ligada): a página de destino e o topo dela.
    pub fn search_bookmarks(&self, needle: &str, opts: &SearchOptions) -> Vec<(i32, PdfRect)> {
        let Some(re) = regex_for(needle, opts).filter(|_| opts.bookmarks) else {
            return Vec::new();
        };
        self.outline()
            .unwrap_or_default()
            .into_iter()
            .filter(|o| re.is_match(&o.title))
            .filter_map(|o| Some((o.page?, PdfRect::new(0.0, 0.0, 1.0, 1.0))))
            .collect()
    }
}
