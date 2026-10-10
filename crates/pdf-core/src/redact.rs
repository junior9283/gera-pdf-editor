//! Redigir (Acrobat): localizar o que redigir (frase ou padrões), aplicar as marcas de redação
//! de forma permanente e limpar as informações ocultas do documento.
//!
//! As marcas são anotações `/Redact` (criadas pelo caminho das anotações, com desfazer). Aplicar
//! usa a redação do MuPDF — o texto sob a marca sai, os pixels das imagens são apagados e os
//! desenhos cobertos saem — e pinta cada caixa com a cor e o texto sobreposto escolhidos.
use crate::doc::PdfDoc;
use crate::edit_text::{glyph_run, width};
use crate::error::Result;
use crate::page_content::{invert, matrix_ops, num, rgb_ops};
use crate::types::PdfRect;
use mupdf::pdf::{
    PdfAnnotationType, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::{Font, Rect, TextPageFlags};
use regex::Regex;

/// Dado sensível que o Localizar sabe achar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactPattern {
    Cpf,
    Cnpj,
    Phone,
    Email,
    CreditCard,
    Cep,
    Date,
}

impl RedactPattern {
    pub const ALL: [RedactPattern; 7] = [
        RedactPattern::Cpf,
        RedactPattern::Cnpj,
        RedactPattern::Phone,
        RedactPattern::Email,
        RedactPattern::CreditCard,
        RedactPattern::Cep,
        RedactPattern::Date,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RedactPattern::Cpf => "CPF",
            RedactPattern::Cnpj => "CNPJ",
            RedactPattern::Phone => "Telefone",
            RedactPattern::Email => "E-mail",
            RedactPattern::CreditCard => "Cartão de crédito",
            RedactPattern::Cep => "CEP",
            RedactPattern::Date => "Data",
        }
    }

    fn regex(self) -> &'static str {
        match self {
            RedactPattern::Cpf => r"\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b",
            RedactPattern::Cnpj => r"\b\d{2}\.?\d{3}\.?\d{3}/?\d{4}-?\d{2}\b",
            RedactPattern::Phone => r"(?:\(\d{2}\)\s?|\b\d{2}\s)?\b9?\d{4}-\d{4}\b",
            RedactPattern::Email => r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+",
            RedactPattern::CreditCard => r"\b(?:\d{4}[ -]?){3}\d{4}\b",
            RedactPattern::Cep => r"\b\d{5}-\d{3}\b",
            RedactPattern::Date => r"\b\d{1,2}/\d{1,2}/(?:\d{4}|\d{2})\b",
        }
    }

    /// Confere o achado além do formato (cartão: dígito verificador de Luhn).
    fn accepts(self, found: &str) -> bool {
        if self != RedactPattern::CreditCard {
            return true;
        }
        let digits: Vec<u32> = found.chars().filter_map(|c| c.to_digit(10)).collect();
        let sum: u32 = digits
            .iter()
            .rev()
            .enumerate()
            .map(|(i, &d)| match (i % 2 == 1, d * 2) {
                (true, dd) if dd > 9 => dd - 9,
                (true, dd) => dd,
                (false, _) => d,
            })
            .sum();
        sum.is_multiple_of(10)
    }
}

/// O que procurar.
#[derive(Debug, Clone, PartialEq)]
pub enum RedactQuery {
    /// Frase, sem diferenciar maiúsculas.
    Phrase(String),
    Patterns(Vec<RedactPattern>),
}

/// Um trecho achado: a página, as caixas (na tela) e o texto.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactHit {
    pub page: i32,
    pub rects: Vec<PdfRect>,
    pub text: String,
}

/// Aparência das caixas ao aplicar: cor, texto sobreposto e a cor dele.
#[derive(Debug, Clone, PartialEq)]
pub struct RedactLook {
    pub fill: [u8; 3],
    pub overlay: String,
    pub overlay_color: [u8; 3],
}

/// O que o Limpar documento tira.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SanitizeOptions {
    /// Propriedades (/Info) e metadados XMP.
    pub metadata: bool,
    /// Arquivos anexados.
    pub attachments: bool,
    /// JavaScript e ações automáticas.
    pub scripts: bool,
    /// Comentários (anotações, menos campos de formulário e links).
    pub comments: bool,
}

impl PdfDoc {
    /// Os trechos de todas as páginas que combinam com a consulta, na ordem do documento.
    pub fn find_redactions(&self, query: &RedactQuery) -> Result<Vec<RedactHit>> {
        let mut hits = Vec::new();
        for page in 0..self.page_count()? {
            match query {
                RedactQuery::Phrase(phrase) if !phrase.trim().is_empty() => {
                    hits.extend(self.search(page, phrase)?.into_iter().map(|r| RedactHit {
                        page,
                        rects: vec![r],
                        text: phrase.clone(),
                    }));
                }
                RedactQuery::Phrase(_) => {}
                RedactQuery::Patterns(patterns) => {
                    hits.extend(self.find_patterns(page, patterns)?);
                }
            }
        }
        Ok(hits)
    }

    /// Padrões numa página: cada linha do texto estruturado, com a caixa de cada letra.
    fn find_patterns(&self, page: i32, patterns: &[RedactPattern]) -> Result<Vec<RedactHit>> {
        let regexes: Vec<(RedactPattern, Regex)> = patterns
            .iter()
            .map(|&p| (p, Regex::new(p.regex()).expect("padrão fixo válido")))
            .collect();
        let tp = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        let mut hits = Vec::new();
        for block in tp.blocks() {
            for line in block.lines() {
                // o texto da linha e, para cada letra, onde ela começa no texto e a caixa dela
                let mut text = String::new();
                let mut boxes: Vec<(usize, Rect)> = Vec::new();
                for ch in line.chars() {
                    let Some(c) = ch.char() else { continue };
                    boxes.push((text.len(), Rect::from(ch.quad())));
                    text.push(c);
                }
                for (pattern, re) in &regexes {
                    for m in re.find_iter(&text) {
                        if !pattern.accepts(m.as_str()) {
                            continue;
                        }
                        let rect = boxes
                            .iter()
                            .filter(|(at, _)| *at >= m.start() && *at < m.end())
                            .map(|(_, r)| *r)
                            .reduce(|a, b| a.union(&b));
                        if let Some(r) = rect {
                            hits.push(RedactHit {
                                page,
                                rects: vec![r.into()],
                                text: m.as_str().to_owned(),
                            });
                        }
                    }
                }
            }
        }
        Ok(hits)
    }

    /// Aplica todas as marcas de redação (as do app e as de outros programas): o que está sob
    /// elas sai do arquivo e a caixa é pintada. Devolve quantas marcas foram aplicadas.
    pub fn apply_redactions(&mut self, look: &RedactLook) -> Result<usize> {
        self.require_modifiable("aplicar a redação")?;
        self.with_snapshot(|d| {
            let mut applied = 0;
            for page in 0..d.page_count()? {
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                let mut boxes = Vec::new();
                for annot in pdf_page.annotations() {
                    if annot.r#type()? != PdfAnnotationType::Redact {
                        continue;
                    }
                    applied += 1;
                    let quads = annot.quad_points()?;
                    if quads.is_empty() {
                        boxes.push(annot.rect()?);
                    } else {
                        boxes.extend(quads.into_iter().map(Rect::from));
                    }
                }
                if boxes.is_empty() {
                    continue;
                }
                pdf_page.apply_redactions_with_options(PdfRedactOptions {
                    black_boxes: false,
                    image_method: PdfRedactImageMethod::Pixels,
                    line_art: PdfRedactLineArtMethod::RemoveIfCovered,
                    text: PdfRedactTextMethod::Remove,
                })?;
                drop(pdf_page);
                d.paint_boxes(page, &boxes, look)?;
            }
            Ok(applied)
        })
    }

    /// Pinta as caixas aplicadas (tela) com a cor e o texto sobreposto, por cima do conteúdo.
    fn paint_boxes(&mut self, page: i32, boxes: &[Rect], look: &RedactLook) -> Result<()> {
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        let mut page_obj = pdf_page.object();
        let to_screen = page_obj.page_ctm()?;
        let mut ops = format!(
            " q {} cm {} ",
            matrix_ops(&invert(&to_screen)?),
            rgb_ops(look.fill)
        );
        for b in boxes {
            ops.push_str(&format!(
                "{} {} {} {} re ",
                num(b.x0),
                num(b.y0),
                num(b.x1 - b.x0),
                num(b.y1 - b.y0)
            ));
        }
        ops.push_str("f ");
        let overlay = look.overlay.trim();
        if !overlay.is_empty() {
            let font = Font::new("Helvetica-Bold")?;
            let name = self.font_resource(&mut page_obj, &font)?;
            let unit = width(&font, overlay, 1.0)?.max(0.01);
            ops.push_str(&format!("BT {} ", rgb_ops(look.overlay_color)));
            for b in boxes {
                let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
                // cabe na caixa: até 12 pt, no máximo 75% da altura e 95% da largura
                let size = 12f32.min(0.75 * h).min(0.95 * w / unit);
                if size < 3.0 {
                    continue;
                }
                let x = b.x0 + (w - unit * size) / 2.0;
                let y = b.y0 + h / 2.0 + 0.35 * size;
                ops.push_str(&format!(
                    "/{name} {} Tf 1 0 0 -1 {} {} Tm {} ",
                    num(size),
                    num(x),
                    num(y),
                    glyph_run(&font, overlay)?
                ));
            }
            ops.push_str("ET ");
        }
        ops.push_str("Q ");
        pdf_page.wrap_contents(&mut self.inner)?;
        pdf_page.insert_contents(&mut self.inner, ops.as_bytes(), true)?;
        Ok(())
    }

    /// Limpar documento (Remover informações ocultas do Acrobat).
    pub fn sanitize(&mut self, opts: &SanitizeOptions) -> Result<()> {
        self.require_modifiable("limpar o documento")?;
        self.with_snapshot(|d| {
            let mut trailer = d.inner.trailer()?;
            let mut catalog = d.inner.catalog()?;
            if opts.metadata {
                trailer.dict_delete("Info")?;
                catalog.dict_delete("Metadata")?;
            }
            let mut names = catalog.get_dict("Names")?;
            if opts.attachments {
                catalog.dict_delete("AF")?;
                if let Some(n) = names.as_mut() {
                    n.dict_delete("EmbeddedFiles")?;
                }
            }
            if opts.scripts {
                catalog.dict_delete("OpenAction")?;
                catalog.dict_delete("AA")?;
                if let Some(n) = names.as_mut() {
                    n.dict_delete("JavaScript")?;
                }
            }
            for page in 0..d.page_count()? {
                let mut pdf_page = d.inner.load_pdf_page(page)?;
                if opts.scripts {
                    pdf_page.object().dict_delete("AA")?;
                }
                let mut doomed = Vec::new();
                for annot in pdf_page.annotations() {
                    let kind = annot.r#type()?;
                    let comment = !matches!(
                        kind,
                        PdfAnnotationType::Widget
                            | PdfAnnotationType::Link
                            | PdfAnnotationType::Popup
                    );
                    if (opts.comments && comment)
                        || (opts.attachments && kind == PdfAnnotationType::FileAttachment)
                    {
                        doomed.push(annot);
                        continue;
                    }
                    if opts.scripts {
                        let mut obj = annot.object();
                        obj.dict_delete("AA")?;
                        let js = obj
                            .get_dict("A")?
                            .and_then(|a| a.get_dict("S").ok().flatten())
                            .and_then(|s| s.as_name().ok())
                            .is_some_and(|s| s == b"JavaScript");
                        if js {
                            obj.dict_delete("A")?;
                        }
                    }
                }
                for annot in doomed {
                    pdf_page.delete_annotation(annot)?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_cards_must_pass_luhn() {
        assert!(RedactPattern::CreditCard.accepts("4111 1111 1111 1111"));
        assert!(!RedactPattern::CreditCard.accepts("1234 5678 9012 3456"));
    }

    #[test]
    fn patterns_are_valid_regexes() {
        for p in RedactPattern::ALL {
            Regex::new(p.regex()).unwrap();
        }
    }
}
