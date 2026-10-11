//! Localizar com as opções do Acrobat: palavras inteiras, maiúsculas, marcadores e comentários.
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::types::{PdfRect, SearchOptions};
use regex::Regex;

/// Regex literal do texto procurado, com as opções. `None` para texto vazio.
pub(crate) fn regex_for(needle: &str, opts: &SearchOptions) -> Option<Regex> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let mut pattern = regex::escape(needle);
    if opts.whole_words {
        // a fronteira de palavra só nas pontas que são letra ou algarismo: "(x)" e "C++" são
        // palavras inteiras, e "\b(" exigiria uma letra antes do parêntese
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        let (start, end) = (word(needle.chars().next()), word(needle.chars().last()));
        pattern = format!(
            "{}{pattern}{}",
            if start { r"\b" } else { "" },
            if end { r"\b" } else { "" }
        );
    }
    if !opts.match_case {
        pattern = format!("(?i){pattern}");
    }
    Regex::new(&pattern).ok()
}

/// Uma busca: o texto, as opções e a regex compilada uma vez para o documento todo.
pub struct SearchQuery {
    needle: String,
    opts: SearchOptions,
    re: Option<Regex>,
}

impl SearchQuery {
    pub fn new(needle: &str, opts: SearchOptions) -> Self {
        Self {
            needle: needle.to_owned(),
            opts,
            re: regex_for(needle, &opts),
        }
    }
}

impl PdfDoc {
    /// Resultados da página: o texto (e, com a opção, os comentários cujo conteúdo casa).
    pub fn search_with(
        &self,
        page: i32,
        needle: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<PdfRect>> {
        self.search_page(page, &SearchQuery::new(needle, *opts))
    }

    pub(crate) fn search_page(&self, page: i32, q: &SearchQuery) -> Result<Vec<PdfRect>> {
        let plain = !q.opts.whole_words && !q.opts.match_case;
        let mut hits = match (&q.re, plain) {
            // o padrão continua sendo a busca do MuPDF (acha frases entre linhas)
            (_, true) => self.search(page, &q.needle)?,
            (Some(re), false) => self
                .find_patterns(page, &[(None, re.clone())])?
                .into_iter()
                .flat_map(|h| h.rects)
                .collect(),
            (None, false) => Vec::new(),
        };
        if let Some(re) = q.re.as_ref().filter(|_| q.opts.comments) {
            hits.extend(
                self.annotations(page)?
                    .into_iter()
                    .filter(|a| re.is_match(&a.contents))
                    .map(|a| a.rect),
            );
        }
        Ok(hits)
    }

    /// Marcadores cujo título casa (com a opção ligada): a página de destino e o topo dela.
    pub fn search_bookmarks(&self, needle: &str, opts: &SearchOptions) -> Vec<(i32, PdfRect)> {
        if !opts.bookmarks {
            return Vec::new();
        }
        let Some(re) = regex_for(needle, opts) else {
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
