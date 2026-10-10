//! Editar texto (modo Editar PDF do Acrobat): os blocos de texto da página, trocar o texto de um
//! bloco e acrescentar texto novo.
//!
//! Trocar = redação só de texto nas linhas do bloco (sem tarja, sem tocar em imagens nem traços) e
//! o texto novo escrito por cima, no mesmo lugar, tamanho e cor, com quebra automática na largura
//! do bloco. A fonte original é reaproveitada quando tem todas as letras do texto novo; senão, a
//! padrão equivalente (Helvetica, Times ou Courier, com negrito e itálico). Se a redação não tirar
//! o texto antigo (formulário, fonte Type3), a edição é recusada e o documento volta como estava.
use crate::doc::PdfDoc;
use crate::edit_images::{invert, num, own_resource};
use crate::error::{refused, Result};
use crate::text_fit::text_width;
use crate::types::PdfRect;
use mupdf::pdf::{
    PdfAnnotationType, PdfObject, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::text_page::TextBlockType;
use mupdf::{Font, Quad, Rect, TextPageFlags};
use std::collections::HashMap;

/// Bloco de texto da página.
#[derive(Debug, Clone, PartialEq)]
pub struct PageTextBlock {
    /// Caixa na tela (y para baixo, com a rotação da página).
    pub rect: PdfRect,
    /// O texto, uma linha por `\n`.
    pub text: String,
    /// Tamanho da fonte, em pontos.
    pub size: f32,
    pub color: [u8; 3],
    /// Nome da fonte (sem o prefixo de subconjunto).
    pub font: String,
    /// Só blocos de linhas horizontais (na tela) são editáveis.
    pub editable: bool,
}

/// Direção do texto na tela (vetor unitário).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dir(f32, f32);

impl Dir {
    /// Posição de `p` ao longo da linha.
    fn along(self, p: (f32, f32)) -> f32 {
        p.0 * self.0 + p.1 * self.1
    }

    /// Posição de `p` de cima para baixo (perpendicular à linha).
    fn across(self, p: (f32, f32)) -> f32 {
        -p.0 * self.1 + p.1 * self.0
    }

    fn same(self, o: Dir) -> bool {
        self.0 * o.0 + self.1 * o.1 > 0.999
    }
}

/// Um caractere do texto extraído.
struct Glyph {
    c: char,
    quad: Quad,
    origin: (f32, f32),
    size: f32,
    argb: u32,
    font: Option<Font>,
}

/// Trecho de uma linha (as lacunas grandes separam colunas).
struct Segment {
    glyphs: Vec<Glyph>,
    dir: Dir,
}

impl Segment {
    fn first(&self) -> &Glyph {
        &self.glyphs[0]
    }

    fn baseline(&self) -> f32 {
        self.dir.across(self.first().origin)
    }

    fn size(&self) -> f32 {
        self.glyphs.iter().map(|g| g.size).fold(0.0, f32::max)
    }

    /// Começo e fim ao longo da linha.
    fn span(&self) -> (f32, f32) {
        let d = self.dir;
        self.glyphs.iter().fold((f32::MAX, f32::MIN), |(a, b), g| {
            let (p, q) = (
                d.along((g.quad.ul.x, g.quad.ul.y)),
                d.along((g.quad.ur.x, g.quad.ur.y)),
            );
            (a.min(p.min(q)), b.max(p.max(q)))
        })
    }

    fn bounds(&self) -> Rect {
        self.glyphs
            .iter()
            .map(|g| Rect::from(g.quad.clone()))
            .reduce(|a, b| a.union(&b))
            .expect("trecho sem letras não existe")
    }
}

/// Direção do caractere pela caixa dele.
fn dir_of(q: &Quad) -> Dir {
    let (dx, dy) = (q.ur.x - q.ul.x, q.ur.y - q.ul.y);
    let n = (dx * dx + dy * dy).sqrt();
    if n < 1e-6 {
        Dir(1.0, 0.0)
    } else {
        Dir(dx / n, dy / n)
    }
}

/// Divide a linha onde há lacuna maior que dois emes (colunas) ou a direção muda.
fn split_line(glyphs: Vec<Glyph>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for g in glyphs {
        let dir = dir_of(&g.quad);
        let joins = out.last().is_some_and(|s| {
            let prev = s.glyphs.last().expect("trecho sem letras não existe");
            let end = s.dir.along((prev.quad.ur.x, prev.quad.ur.y));
            let start = s.dir.along((g.quad.ul.x, g.quad.ul.y));
            (g.c == ' ' || s.dir.same(dir)) && start - end <= 2.0 * g.size.max(prev.size)
        });
        match out.last_mut() {
            Some(s) if joins => s.glyphs.push(g),
            _ if g.c.is_whitespace() => {}
            _ => out.push(Segment {
                glyphs: vec![g],
                dir,
            }),
        }
    }
    out
}

/// Junta os trechos em parágrafos: mesma direção, tamanho parecido, logo abaixo do anterior e
/// sobrepostos ao longo da linha.
fn group(segments: Vec<Segment>) -> Vec<Vec<Segment>> {
    let mut blocks: Vec<Vec<Segment>> = Vec::new();
    for s in segments {
        let size = s.size();
        let (a, b) = s.span();
        let target = blocks.iter_mut().rev().find(|block| {
            let last = block.last().expect("bloco sem linhas não existe");
            let step = s.baseline() - last.baseline();
            let (la, lb) = last.span();
            let regular = block.len() < 2 || {
                let prev = block[block.len() - 2].baseline();
                (step - (last.baseline() - prev)).abs() <= 0.3 * size
            };
            last.dir.same(s.dir)
                && (last.size() - size).abs() <= 0.15 * size.max(last.size())
                && step > 0.3 * size
                && step <= 1.7 * size
                && a < lb
                && b > la
                && regular
        });
        match target {
            Some(block) => block.push(s),
            None => blocks.push(vec![s]),
        }
    }
    blocks
}

/// O bloco com o que é preciso para trocá-lo.
struct Block {
    info: PageTextBlock,
    /// Caixa de cada linha (a redação tira só o que está nelas).
    lines: Vec<Rect>,
    font: Option<Font>,
    /// Origem do primeiro caractere (na linha de base), na tela.
    origin: (f32, f32),
    dir: Dir,
    /// Fim das linhas ao longo da direção do texto.
    end: f32,
    /// Distância entre as linhas de base (só com mais de uma linha).
    gap: Option<f32>,
}

impl Block {
    fn from_lines(mut lines: Vec<Segment>) -> Block {
        // estilo mais frequente: (fonte, tamanho×10, cor) → letras e a fonte
        let mut styles: HashMap<(String, i32, u32), (usize, Option<Font>)> = HashMap::new();
        for g in lines.iter_mut().flat_map(|l| &mut l.glyphs) {
            if g.c.is_whitespace() {
                continue;
            }
            let key = (
                g.font
                    .as_ref()
                    .map(|f| f.name().to_owned())
                    .unwrap_or_default(),
                (g.size * 10.0).round() as i32,
                g.argb,
            );
            let e = styles.entry(key).or_insert((0, None));
            e.0 += 1;
            if e.1.is_none() {
                e.1 = g.font.take();
            }
        }
        let ((name, size, argb), (_, font)) = styles
            .into_iter()
            .max_by_key(|(_, (n, _))| *n)
            .expect("trecho sempre tem letra");
        let dir = lines[0].dir;
        let n = lines.len();
        let gap = (n > 1).then(|| (lines[n - 1].baseline() - lines[0].baseline()) / (n - 1) as f32);
        let rects: Vec<Rect> = lines.iter().map(Segment::bounds).collect();
        let text = lines
            .iter()
            .map(|l| {
                l.glyphs
                    .iter()
                    .map(|g| g.c)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        Block {
            info: PageTextBlock {
                rect: rects
                    .iter()
                    .copied()
                    .reduce(|a, b| a.union(&b))
                    .expect("bloco tem linhas")
                    .into(),
                text,
                size: size as f32 / 10.0,
                color: [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8],
                font: strip_subset(&name).to_owned(),
                editable: true,
            },
            lines: rects,
            font,
            origin: lines[0].first().origin,
            dir,
            end: lines.iter().map(|l| l.span().1).fold(f32::MIN, f32::max),
            gap,
        }
    }
}

/// Onde e como escrever: origem da primeira linha, direção, largura e entrelinha.
struct Place {
    origin: (f32, f32),
    dir: Dir,
    maxw: f32,
    gap: f32,
}

/// Estilo do texto a escrever.
struct Style {
    font: Font,
    size: f32,
    color: [u8; 3],
}

/// Tamanho de fonte padrão do Adicionar texto, como no Acrobat.
const NEW_TEXT_SIZE: f32 = 12.0;
/// Chave própria no dicionário da fonte acrescentada: a impressão digital da fonte, para a próxima
/// edição reaproveitar a mesma (o nome muda: a Helvetica não embutida, lida de volta, vira
/// NimbusSans-Regular).
const SOURCE_KEY: &str = "GeraFonte";
/// Entrelinha quando o bloco tem uma linha só.
const LINE_HEIGHT: f32 = 1.2;

fn strip_subset(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// A fonte padrão do PDF parecida com `font` (família, negrito e itálico).
fn standard_font(font: Option<&Font>) -> Result<Font> {
    let name = font.map_or("", |f| f.name()).to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let bold = font.is_some_and(Font::is_bold) || has(&["bold", "black", "heavy", "semibold"]);
    let italic = font.is_some_and(Font::is_italic) || has(&["italic", "oblique"]);
    let mono = font.is_some_and(Font::is_monospaced) || has(&["courier", "mono"]);
    let serif = font.is_some_and(Font::is_serif)
        || has(&["times", "serif", "roman", "georgia", "garamond", "book"]) && !has(&["sans"]);
    let family = if mono {
        "Courier"
    } else if serif {
        "Times"
    } else {
        "Helvetica"
    };
    let style = match (bold, italic, family) {
        (false, false, "Times") => "-Roman",
        (false, false, _) => "",
        (true, false, _) => "-Bold",
        (false, true, "Times") => "-Italic",
        (true, true, "Times") => "-BoldItalic",
        (false, true, _) => "-Oblique",
        (true, true, _) => "-BoldOblique",
    };
    Ok(Font::new(&format!("{family}{style}"))?)
}

/// A fonte tem todas as letras do texto (os espaços podem faltar: viram deslocamento)?
fn covers(font: &Font, text: &str) -> bool {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .all(|c| font.encode_character(c as i32).is_ok_and(|g| g > 0))
}

/// Largura do texto, com espaço sem glifo valendo um quarto de eme.
fn width(font: &Font, s: &str, size: f32) -> Result<f32> {
    let mut w = 0.0;
    for c in s.chars() {
        let g = font.encode_character(c as i32)?;
        w += if g == 0 && c == ' ' {
            0.25 * size
        } else {
            text_width(font, &c.to_string(), size)?
        };
    }
    Ok(w)
}

/// Quebra o texto em linhas de até `maxw`: parágrafos nos `\n`, depois entre palavras; uma palavra
/// maior que a linha quebra no meio.
fn wrap(font: &Font, text: &str, size: f32, maxw: f32) -> Result<Vec<String>> {
    let maxw = maxw.max(size);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if line.is_empty() || width(font, &candidate, size)? <= maxw {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            }
            // palavra maior que a linha: corta onde couber
            while width(font, &line, size)? > maxw && line.chars().count() > 1 {
                let mut cut = String::new();
                for c in line.chars() {
                    cut.push(c);
                    if width(font, &cut, size)? > maxw {
                        cut.pop();
                        break;
                    }
                }
                if cut.is_empty() {
                    cut = line.chars().next().map(String::from).unwrap_or_default();
                }
                line = line[cut.len()..].to_owned();
                lines.push(cut);
            }
        }
        lines.push(line);
    }
    Ok(lines)
}

/// O `TJ` de uma linha: os glifos em hexadecimal (fonte Identity-H) e os espaços sem glifo como
/// deslocamento.
fn glyph_run(font: &Font, line: &str) -> Result<String> {
    let mut out = String::from("[<");
    for c in line.chars() {
        let g = font.encode_character(c as i32)?;
        if g == 0 && c == ' ' {
            out.push_str("> -250 <");
        } else {
            out.push_str(&format!("{g:04X}"));
        }
    }
    out.push_str(">] TJ");
    Ok(out)
}

/// Impressão digital da fonte: glifo e avanço de algumas letras, ascendente e descendente (iguais
/// para a mesma fonte, qualquer que seja o nome com que foi carregada).
fn fingerprint(font: &Font) -> String {
    let mut out = format!("{:.4} {:.4}", font.ascender(), font.descender());
    for c in ['a', 'W', '0', 'g', 'M'] {
        let g = font.encode_character(c as i32).unwrap_or(0);
        let w = font.advance_glyph(g).unwrap_or(0.0);
        out.push_str(&format!(" {g}:{w:.4}"));
    }
    out
}

/// A caixa de `rect` reduzida para conferir o que sobrou depois da redação.
fn inner(rect: &Rect) -> Rect {
    let (dx, dy) = ((rect.x1 - rect.x0) * 0.1, (rect.y1 - rect.y0) * 0.25);
    Rect::new(rect.x0 + dx, rect.y0 + dy, rect.x1 - dx, rect.y1 - dy)
}

fn contains(r: &Rect, x: f32, y: f32) -> bool {
    x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1
}

impl PdfDoc {
    /// Blocos de texto da página, na ordem de leitura.
    pub fn page_text_blocks(&self, page: i32) -> Result<Vec<PageTextBlock>> {
        Ok(self
            .read_blocks(page)?
            .into_iter()
            .map(|b| b.info)
            .collect())
    }

    fn read_blocks(&self, page: i32) -> Result<Vec<Block>> {
        let tp = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        let mut segments = Vec::new();
        for block in tp.blocks() {
            if block.r#type() != TextBlockType::Text {
                continue;
            }
            for line in block.lines() {
                let glyphs: Vec<Glyph> = line
                    .chars()
                    .filter_map(|ch| {
                        Some(Glyph {
                            c: ch.char()?,
                            quad: ch.quad(),
                            origin: (ch.origin().x, ch.origin().y),
                            size: ch.size(),
                            argb: ch.argb() & 0xff_ffff,
                            font: ch.font(),
                        })
                    })
                    .collect();
                segments.extend(split_line(glyphs));
            }
        }
        Ok(group(segments).into_iter().map(Block::from_lines).collect())
    }

    /// Troca o texto do bloco de caixa `rect` (o mais próximo) por `text`; texto vazio apaga o
    /// bloco. Desfazer volta o documento todo de antes.
    pub fn edit_text_block(&mut self, page: i32, rect: PdfRect, text: &str) -> Result<()> {
        self.guarded_page_edit(page, "editar o texto", |doc| {
            let distance = |b: &Block| {
                let r = b.info.rect;
                (r.x0 - rect.x0).abs()
                    + (r.y0 - rect.y0).abs()
                    + (r.x1 - rect.x1).abs()
                    + (r.y1 - rect.y1).abs()
            };
            let block = doc
                .read_blocks(page)?
                .into_iter()
                .filter(|b| distance(b) <= 8.0)
                .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                .ok_or_else(|| refused("o bloco de texto não está mais na página"))?;
            if !block.info.editable {
                return Err(refused("este texto não pode ser editado"));
            }
            doc.redact_lines(page, &block.lines)?;
            let text = text.trim_end();
            if text.trim().is_empty() {
                return Ok(());
            }
            let font = match block.font {
                Some(f) if covers(&f, text) => f,
                f => standard_font(f.as_ref())?,
            };
            let style = Style {
                font,
                size: block.info.size,
                color: block.info.color,
            };
            // parágrafo mantém a largura; linha solta (título) cresce até a margem, como no Acrobat
            let maxw = if block.lines.len() > 1 {
                block.end - block.dir.along(block.origin)
            } else {
                doc.room(page, block.origin, block.dir)?
            };
            let place = Place {
                origin: block.origin,
                dir: block.dir,
                maxw,
                gap: block.gap.unwrap_or(LINE_HEIGHT * style.size),
            };
            doc.write_text(page, &style, text, &place)
        })
    }

    /// Acrescenta `text` com o canto superior esquerdo em `at` (tela), em Helvetica 12 preta,
    /// quebrando na margem direita da página.
    pub fn add_text(&mut self, page: i32, at: (f32, f32), text: &str) -> Result<()> {
        self.guarded_page_edit(page, "acrescentar texto", |doc| {
            let text = text.trim_end();
            if text.trim().is_empty() {
                return Ok(());
            }
            let font = Font::new("Helvetica")?;
            let origin = (at.0, at.1 + font.ascender() * NEW_TEXT_SIZE);
            let place = Place {
                origin,
                dir: Dir(1.0, 0.0),
                maxw: doc.room(page, origin, Dir(1.0, 0.0))?,
                gap: LINE_HEIGHT * NEW_TEXT_SIZE,
            };
            let style = Style {
                font,
                size: NEW_TEXT_SIZE,
                color: [0, 0, 0],
            };
            doc.write_text(page, &style, text, &place)
        })
    }

    /// Espaço de `origin` até a margem da página (36 pt) na direção `dir`.
    fn room(&self, page: i32, origin: (f32, f32), dir: Dir) -> Result<f32> {
        let b = self.load(page)?.bounds()?;
        let m = 36.0;
        let limit = |p: f32, d: f32, lo: f32, hi: f32| {
            if d > 1e-3 {
                (hi - m - p) / d
            } else if d < -1e-3 {
                (lo + m - p) / d
            } else {
                f32::INFINITY
            }
        };
        let t = limit(origin.0, dir.0, b.x0, b.x1).min(limit(origin.1, dir.1, b.y0, b.y1));
        Ok(t.max(72.0))
    }

    /// Redação só de texto nas caixas das linhas; recusa se o texto antigo continuar lá.
    fn redact_lines(&mut self, page: i32, lines: &[Rect]) -> Result<()> {
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        if pdf_page
            .annotations()
            .any(|a| a.r#type().is_ok_and(|t| t == PdfAnnotationType::Redact))
        {
            return Err(refused(
                "a página tem marcas de redação pendentes; aplique ou apague-as antes",
            ));
        }
        let quads: Vec<Quad> = lines.iter().map(|r| Quad::from(*r)).collect();
        pdf_page.add_redact_annotation(quads)?;
        pdf_page.apply_redactions_with_options(PdfRedactOptions {
            black_boxes: false,
            image_method: PdfRedactImageMethod::None,
            line_art: PdfRedactLineArtMethod::None,
            text: PdfRedactTextMethod::Remove,
        })?;
        let tp = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        let boxes: Vec<Rect> = lines.iter().map(inner).collect();
        for block in tp.blocks() {
            for line in block.lines() {
                for ch in line.chars() {
                    if ch.char().is_none_or(char::is_whitespace) {
                        continue;
                    }
                    let q = ch.quad();
                    let (x, y) = ((q.ul.x + q.lr.x) / 2.0, (q.ul.y + q.lr.y) / 2.0);
                    if boxes.iter().any(|b| contains(b, x, y)) {
                        return Err(refused(
                            "este texto não pode ser editado (faz parte de um formulário ou usa \
                             uma fonte especial)",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Escreve `text` por cima do conteúdo da página, na posição e direção de `place`.
    fn write_text(&mut self, page: i32, style: &Style, text: &str, place: &Place) -> Result<()> {
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        let mut page_obj = pdf_page.object();
        let to_screen = page_obj.page_ctm()?;
        let name = self.font_resource(&mut page_obj, &style.font)?;
        let [r, g, b] = style.color.map(|c| num(c as f32 / 255.0));
        let m = invert(&to_screen)?;
        let mut ops = format!(
            " q {} cm BT {r} {g} {b} rg /{name} {} Tf ",
            [m.a, m.b, m.c, m.d, m.e, m.f].map(num).join(" "),
            num(style.size)
        );
        // na tela (y para baixo): o x do glifo segue `dir`, o "para cima" dele é `dir` girada
        let Dir(dx, dy) = place.dir;
        let (ox, oy) = place.origin;
        for (n, line) in wrap(&style.font, text, style.size, place.maxw)?
            .iter()
            .enumerate()
        {
            let t = n as f32 * place.gap;
            let (x, y) = (ox - dy * t, oy + dx * t);
            ops.push_str(&format!(
                "{} {} {} {} {} {} Tm {} ",
                num(dx),
                num(dy),
                num(dy),
                num(-dx),
                num(x),
                num(y),
                glyph_run(&style.font, line)?
            ));
        }
        ops.push_str("ET Q ");
        // o conteúdo antigo entre q/Q: o texto novo não herda matriz, cor nem recorte dele
        pdf_page.wrap_contents(&mut self.inner)?;
        pdf_page.insert_contents(&mut self.inner, ops.as_bytes(), true)?;
        Ok(())
    }

    /// A fonte nos recursos da página (como `GeraFnN`), reaproveitando a já posta por outra edição.
    fn font_resource(&mut self, page_obj: &mut PdfObject, font: &Font) -> Result<String> {
        let mut fonts = own_resource(&self.inner, page_obj, "Font")?;
        let mut n = 0;
        loop {
            let name = format!("GeraFn{n}");
            match fonts.get_dict(name.as_str())? {
                Some(f) => {
                    let source = f.get_dict(SOURCE_KEY)?.and_then(|b| b.as_string().ok());
                    if source.as_deref() == Some(fingerprint(font).as_str()) {
                        return Ok(name);
                    }
                }
                None => {
                    let mut obj = self.inner.add_font(font)?;
                    obj.dict_put(SOURCE_KEY, PdfObject::new_string(&fingerprint(font))?)?;
                    fonts.dict_put(name.as_str(), obj)?;
                    return Ok(name);
                }
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_paragraphs_and_breaks_long_words() {
        let f = Font::new("Helvetica").unwrap();
        let lines = wrap(&f, "um dois três\nquatro", 10.0, 40.0).unwrap();
        assert_eq!(lines.last().unwrap(), "quatro");
        assert!(lines.len() >= 3, "{lines:?}");
        let long = wrap(&f, "aaaaaaaaaaaaaaaaaaaaaaaa", 10.0, 30.0).unwrap();
        assert!(long.len() > 1 && long.concat() == "aaaaaaaaaaaaaaaaaaaaaaaa");
    }

    #[test]
    fn subset_prefix_is_stripped() {
        assert_eq!(strip_subset("ABCDEF+Calibri-Bold"), "Calibri-Bold");
        assert_eq!(strip_subset("Arial+X"), "Arial+X");
    }

    #[test]
    fn standard_font_follows_the_original_style() {
        let f = |name: &str| standard_font(Some(&Font::new(name).unwrap())).unwrap();
        assert!(f("Times-BoldItalic").name().contains("Times"));
        assert!(f("Courier-Bold").name().contains("Courier"));
    }
}
