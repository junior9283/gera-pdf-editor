//! Editar texto (modo Editar PDF do Acrobat): os blocos de texto da página, trocar o texto de um
//! bloco e acrescentar texto novo.
//!
//! Trocar = redação só de texto nas linhas do bloco (sem tarja, sem tocar em imagens nem traços) e
//! o texto novo escrito por cima, no mesmo lugar, tamanho e cor, com quebra automática na largura
//! do bloco. A fonte original é reaproveitada quando tem todas as letras do texto novo; senão, a
//! padrão equivalente (Helvetica, Times ou Courier, com negrito e itálico). Se a redação não tirar
//! o texto antigo (formulário, fonte Type3), a edição é recusada e o documento volta como estava.
use crate::doc::PdfDoc;
use crate::error::{refused, Result};
use crate::page_content::{invert, matrix_ops, num, own_resource, rgb_ops};
use crate::text_fit::LINE_HEIGHT;
use crate::types::PdfRect;
use mupdf::pdf::{
    PdfAnnotationType, PdfObject, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::text_page::TextBlockType;
use mupdf::text_page::TextCharFlags;
use mupdf::{Font, Matrix, Point, Quad, Rect, StrokeState, TextPageFlags};
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
    /// Pintado (preenchido ou contornado); o invisível é o texto de OCR (`3 Tr`).
    visible: bool,
}

/// Trecho de uma linha (as lacunas grandes separam colunas), com as medidas usadas para juntar
/// os trechos em parágrafos.
struct Segment {
    glyphs: Vec<Glyph>,
    dir: Dir,
    /// Linha de base (perpendicular à direção), maior tamanho de letra, começo e fim ao longo da
    /// linha.
    baseline: f32,
    size: f32,
    span: (f32, f32),
}

impl Segment {
    fn new(glyphs: Vec<Glyph>, dir: Dir) -> Self {
        let span = glyphs.iter().fold((f32::MAX, f32::MIN), |(a, b), g| {
            let (p, q) = (
                dir.along((g.quad.ul.x, g.quad.ul.y)),
                dir.along((g.quad.ur.x, g.quad.ur.y)),
            );
            (a.min(p.min(q)), b.max(p.max(q)))
        });
        Self {
            baseline: dir.across(glyphs[0].origin),
            size: glyphs.iter().map(|g| g.size).fold(0.0, f32::max),
            span,
            glyphs,
            dir,
        }
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
    let mut out: Vec<(Vec<Glyph>, Dir)> = Vec::new();
    for g in glyphs {
        let dir = dir_of(&g.quad);
        let joins = out.last().is_some_and(|(s, d)| {
            let prev = s.last().expect("trecho sem letras não existe");
            let end = d.along((prev.quad.ur.x, prev.quad.ur.y));
            let start = d.along((g.quad.ul.x, g.quad.ul.y));
            (g.c == ' ' || d.same(dir)) && start - end <= 2.0 * g.size.max(prev.size)
        });
        match out.last_mut() {
            Some((s, _)) if joins => s.push(g),
            _ if g.c.is_whitespace() => {}
            _ => out.push((vec![g], dir)),
        }
    }
    out.into_iter().map(|(g, d)| Segment::new(g, d)).collect()
}

/// Junta os trechos em parágrafos: mesma direção, tamanho parecido, logo abaixo do anterior e
/// sobrepostos ao longo da linha.
fn group(segments: Vec<Segment>) -> Vec<Vec<Segment>> {
    let mut blocks: Vec<Vec<Segment>> = Vec::new();
    for s in segments {
        let size = s.size;
        let (a, b) = s.span;
        let target = blocks.iter_mut().rev().find(|block| {
            let last = block.last().expect("bloco sem linhas não existe");
            let step = s.baseline - last.baseline;
            let (la, lb) = last.span;
            let regular = block.len() < 2 || {
                let prev = block[block.len() - 2].baseline;
                (step - (last.baseline - prev)).abs() <= 0.3 * size
            };
            last.dir.same(s.dir)
                && (last.size - size).abs() <= 0.15 * size.max(last.size)
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
    /// Caixa de cada linha (para conferir o que sobrou depois da redação).
    lines: Vec<Rect>,
    /// Faixa do meio de cada letra: a redação tira só o que a toca, não as linhas vizinhas
    /// (as caixas das letras, do ascendente ao descendente, se sobrepõem em linhas apertadas).
    marks: Vec<Quad>,
    font: Option<Font>,
    /// Início da margem esquerda na linha de base da primeira linha, na tela, e o recuo da
    /// primeira linha a partir dela.
    origin: (f32, f32),
    indent: f32,
    dir: Dir,
    /// Largura do bloco ao longo da direção do texto (da margem ao fim da linha mais longa).
    width: f32,
    /// Distância entre as linhas de base (só com mais de uma linha).
    gap: Option<f32>,
    /// Texto invisível (camada de OCR).
    invisible: bool,
}

/// A faixa do meio (35% a 65% da altura) da caixa da letra.
fn middle_band(q: &Quad) -> Quad {
    let at = |lo: &Point, hi: &Point, t: f32| Point {
        x: lo.x + (hi.x - lo.x) * t,
        y: lo.y + (hi.y - lo.y) * t,
    };
    Quad::new(
        at(&q.ll, &q.ul, 0.65),
        at(&q.lr, &q.ur, 0.65),
        at(&q.ll, &q.ul, 0.35),
        at(&q.lr, &q.ur, 0.35),
    )
}

impl Block {
    fn from_lines(mut lines: Vec<Segment>) -> Block {
        // estilo mais frequente: (fonte, tamanho×10, cor) → letras e a fonte
        let mut styles: HashMap<(String, i32, u32), (usize, Option<Font>)> = HashMap::new();
        let (mut shown, mut letters) = (0, 0);
        for g in lines.iter_mut().flat_map(|l| &mut l.glyphs) {
            if g.c.is_whitespace() {
                continue;
            }
            letters += 1;
            shown += usize::from(g.visible);
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
        let gap = (n > 1).then(|| (lines[n - 1].baseline - lines[0].baseline) / (n - 1) as f32);
        let rects: Vec<Rect> = lines.iter().map(Segment::bounds).collect();
        let left = lines.iter().map(|l| l.span.0).fold(f32::MAX, f32::min);
        let end = lines.iter().map(|l| l.span.1).fold(f32::MIN, f32::max);
        let em = size as f32 / 10.0;
        // a linha continua na seguinte (o parágrafo reflui, como no Acrobat) se a primeira palavra
        // da seguinte não caberia no fim dela; senão, a quebra era do autor
        let mut text = String::new();
        for (i, l) in lines.iter().enumerate() {
            let s: String = l.glyphs.iter().map(|g| g.c).collect();
            text.push_str(s.trim_end());
            if let Some(next) = lines.get(i + 1) {
                let word_end = next
                    .glyphs
                    .iter()
                    .take_while(|g| !g.c.is_whitespace())
                    .map(|g| dir.along((g.quad.ur.x, g.quad.ur.y)))
                    .fold(next.span.0, f32::max);
                let word = word_end - next.span.0;
                text.push(if l.span.1 + 0.3 * em + word > end {
                    ' '
                } else {
                    '\n'
                });
            }
        }
        let first = lines[0].glyphs[0].origin;
        let indent = (dir.along(first) - left).max(0.0);
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
            },
            marks: lines
                .iter()
                .flat_map(|l| &l.glyphs)
                .filter(|g| !g.c.is_whitespace())
                .map(|g| middle_band(&g.quad))
                .collect(),
            lines: rects,
            font,
            origin: (first.0 - dir.0 * indent, first.1 - dir.1 * indent),
            indent,
            dir,
            width: end - left,
            gap,
            invisible: shown * 2 < letters,
        }
    }
}

/// Onde e como escrever: origem da primeira linha, direção, largura e entrelinha.
struct Place {
    origin: (f32, f32),
    dir: Dir,
    maxw: f32,
    gap: f32,
    /// Recuo da primeira linha.
    indent: f32,
}

/// Estilo do texto a escrever.
struct Style {
    font: Font,
    size: f32,
    color: [u8; 3],
    invisible: bool,
}

/// Tamanho de fonte do Adicionar texto, como no Acrobat.
pub const NEW_TEXT_SIZE: f32 = 12.0;
/// Margem da página até onde a linha solta e o texto novo crescem, e a largura mínima deles.
pub const TEXT_MARGIN: f32 = 36.0;
pub const MIN_TEXT_WIDTH: f32 = 72.0;

pub(crate) fn strip_subset(name: &str) -> &str {
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

/// A fonte tem o desenho de todas as letras do texto (os espaços podem faltar: viram
/// deslocamento)? Subconjuntos podem manter o mapa de caracteres inteiro com os glifos não usados
/// vazios, e fontes Type3 não têm contorno: os dois caem para a padrão.
fn covers(font: &Font, text: &str) -> bool {
    text.chars().filter(|c| !c.is_whitespace()).all(|c| {
        font.encode_character(c as i32)
            .is_ok_and(|g| g > 0 && has_outline(font, g))
    })
}

/// O glifo tem desenho (contorno com área)?
fn has_outline(font: &Font, glyph: i32) -> bool {
    let Ok(Some(path)) = font.outline_glyph(glyph) else {
        return false;
    };
    path.bounds(&StrokeState::default(), &Matrix::IDENTITY)
        .is_ok_and(|b| b.x1 > b.x0 && b.y1 > b.y0)
}

/// Avanço de um caractere; o espaço sem glifo vale um quarto de eme.
pub(crate) fn advance(font: &Font, c: char, size: f32) -> Result<f32> {
    let g = font.encode_character(c as i32)?;
    Ok(if g == 0 && c == ' ' {
        0.25 * size
    } else {
        font.advance_glyph(g)? * size
    })
}

pub(crate) fn width(font: &Font, s: &str, size: f32) -> Result<f32> {
    s.chars().map(|c| advance(font, c, size)).sum()
}

/// Quebra o texto em linhas de até `maxw`: parágrafos nos `\n`, depois entre palavras; uma palavra
/// maior que a linha quebra no meio. Cada palavra é medida uma vez.
fn wrap(font: &Font, text: &str, size: f32, maxw: f32, indent: f32) -> Result<Vec<String>> {
    let maxw = maxw.max(size);
    let space = advance(font, ' ', size)?;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        // a primeira linha começa depois do recuo
        let mut used = if lines.is_empty() {
            indent.min(maxw - size).max(0.0)
        } else {
            0.0
        };
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let w = width(font, word, size)?;
            if !line.is_empty() && used + space + w > maxw {
                lines.push(std::mem::take(&mut line));
                used = 0.0;
            }
            if line.is_empty() && used + w > maxw {
                // palavra maior que a linha: corta onde couber
                for c in word.chars() {
                    let a = advance(font, c, size)?;
                    if !line.is_empty() && used + a > maxw {
                        lines.push(std::mem::take(&mut line));
                        used = 0.0;
                    }
                    line.push(c);
                    used += a;
                }
                continue;
            }
            if !line.is_empty() {
                line.push(' ');
                used += space;
            }
            line.push_str(word);
            used += w;
        }
        lines.push(line);
    }
    Ok(lines)
}

/// O `TJ` de uma linha: os glifos em hexadecimal (fonte Identity-H) e os espaços sem glifo como
/// deslocamento.
pub(crate) fn glyph_run(font: &Font, line: &str) -> Result<String> {
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

/// A caixa de `rect` reduzida para conferir o que sobrou depois da redação.
fn inner(rect: &Rect) -> PdfRect {
    let (dx, dy) = ((rect.x1 - rect.x0) * 0.1, (rect.y1 - rect.y0) * 0.25);
    PdfRect::new(rect.x0 + dx, rect.y0 + dy, rect.x1 - dx, rect.y1 - dy)
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
                            visible: ch
                                .flags()
                                .intersects(TextCharFlags::FILLED | TextCharFlags::STROKED),
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
            doc.redact_lines(page, &block.lines, &block.marks)?;
            let text = text.trim_end();
            if text.trim().is_empty() {
                return Ok(());
            }
            // parágrafo mantém a largura; linha solta (título) cresce até a margem, como no Acrobat
            let maxw = if block.lines.len() > 1 {
                block.width
            } else {
                doc.room(page, block.origin, block.dir)?
            };
            let place = Place {
                origin: block.origin,
                dir: block.dir,
                maxw,
                gap: block.gap.unwrap_or(LINE_HEIGHT * block.info.size),
                indent: block.indent,
            };
            let style = |font| Style {
                font,
                size: block.info.size,
                color: block.info.color,
                invisible: block.invisible,
            };
            let fallback = standard_font(block.font.as_ref())?;
            match block.font {
                // a original pode não poder ser embutida de novo (Type1 antiga, Type3): a padrão
                Some(f) if covers(&f, text) => doc
                    .write_text(page, &style(f), text, &place)
                    .or_else(|_| doc.write_text(page, &style(fallback), text, &place)),
                _ => doc.write_text(page, &style(fallback), text, &place),
            }
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
                indent: 0.0,
            };
            let style = Style {
                font,
                size: NEW_TEXT_SIZE,
                color: [0, 0, 0],
                invisible: false,
            };
            doc.write_text(page, &style, text, &place)
        })
    }

    /// Espaço de `origin` até a margem da página (36 pt) na direção `dir`.
    fn room(&self, page: i32, origin: (f32, f32), dir: Dir) -> Result<f32> {
        let b = self.load(page)?.bounds()?;
        let m = TEXT_MARGIN;
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
        Ok(t.max(MIN_TEXT_WIDTH))
    }

    /// Redação só de texto na faixa do meio das letras do bloco (`marks`); recusa se o texto
    /// antigo continuar nas linhas ou se alguma letra de fora sumir. Links e comentários sobre o
    /// texto ficam (a redação do MuPDF os apagaria).
    fn redact_lines(&mut self, page: i32, lines: &[Rect], marks: &[Quad]) -> Result<()> {
        let before = self.letters(page)?;
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        if pdf_page
            .annotations()
            .any(|a| a.r#type().is_ok_and(|t| t == PdfAnnotationType::Redact))
        {
            return Err(refused(
                "a página tem marcas de redação pendentes; aplique ou apague-as antes",
            ));
        }
        let mut page_obj = pdf_page.object();
        let annots = page_obj
            .get_dict("Annots")?
            .map(|a| a.copy_array())
            .transpose()?;
        pdf_page.add_redact_annotation(marks.to_vec())?;
        pdf_page.apply_redactions_with_options(PdfRedactOptions {
            black_boxes: false,
            image_method: PdfRedactImageMethod::None,
            line_art: PdfRedactLineArtMethod::None,
            text: PdfRedactTextMethod::Remove,
        })?;
        match annots {
            Some(a) => page_obj.dict_put("Annots", a)?,
            None => page_obj.dict_delete("Annots")?,
        }
        drop(pdf_page);
        let after = self.letters(page)?;
        let boxes: Vec<PdfRect> = lines.iter().map(inner).collect();
        let inside = |p: &(f32, f32)| boxes.iter().any(|b| b.contains(p.0, p.1));
        if after.iter().any(inside) {
            return Err(refused(
                "este texto não pode ser editado (faz parte de um formulário ou usa uma fonte \
                 especial)",
            ));
        }
        // só as letras do bloco podem ter saído: as outras continuam todas
        let center = |q: &Quad| ((q.ul.x + q.lr.x) / 2.0, (q.ul.y + q.lr.y) / 2.0);
        let ours: Vec<(f32, f32)> = marks.iter().map(center).collect();
        let others = before
            .iter()
            .filter(|p| {
                !ours
                    .iter()
                    .any(|o| (o.0 - p.0).abs() < 0.05 && (o.1 - p.1).abs() < 0.05)
            })
            .count();
        if after.len() < others {
            return Err(refused(
                "editar este texto apagaria letras vizinhas (linhas sobrepostas)",
            ));
        }
        Ok(())
    }

    /// Centro de cada letra (sem os espaços) da página.
    fn letters(&self, page: i32) -> Result<Vec<(f32, f32)>> {
        let tp = self.load(page)?.to_text_page(TextPageFlags::empty())?;
        let mut out = Vec::new();
        for block in tp.blocks() {
            for line in block.lines() {
                for ch in line.chars() {
                    if ch.char().is_none_or(char::is_whitespace) {
                        continue;
                    }
                    let q = ch.quad();
                    out.push(((q.ul.x + q.lr.x) / 2.0, (q.ul.y + q.lr.y) / 2.0));
                    // (o mesmo centro que o da faixa do meio em `marks`)
                }
            }
        }
        Ok(out)
    }

    /// Escreve `text` por cima do conteúdo da página, na posição e direção de `place`.
    fn write_text(&mut self, page: i32, style: &Style, text: &str, place: &Place) -> Result<()> {
        let mut pdf_page = self.inner.load_pdf_page(page)?;
        let mut page_obj = pdf_page.object();
        let to_screen = page_obj.page_ctm()?;
        let name = self.font_resource(&mut page_obj, &style.font)?;
        let mut ops = format!(
            " q {} cm BT {} /{name} {} Tf {}",
            matrix_ops(&invert(&to_screen)?),
            rgb_ops(style.color),
            num(style.size),
            if style.invisible { "3 Tr " } else { "" }
        );
        // na tela (y para baixo): o x do glifo segue `dir`, o "para cima" dele é `dir` girada
        let Dir(dx, dy) = place.dir;
        let (ox, oy) = place.origin;
        for (n, line) in wrap(&style.font, text, style.size, place.maxw, place.indent)?
            .iter()
            .enumerate()
        {
            let t = n as f32 * place.gap;
            let along = if n == 0 { place.indent } else { 0.0 };
            let (x, y) = (ox - dy * t + dx * along, oy + dx * t + dy * along);
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

    /// A fonte nos recursos da página (como `GeraFnN`). O MuPDF já devolve o mesmo objeto para o
    /// mesmo programa de fonte acrescentado nesta sessão (pelo MD5); o nome que já aponta para ele
    /// é reaproveitado. Fontes do arquivo nunca são tomadas por "do app".
    pub(crate) fn font_resource(
        &mut self,
        page_obj: &mut PdfObject,
        font: &Font,
    ) -> Result<String> {
        let obj = self.inner.add_font(font)?;
        let num = obj.as_indirect()?;
        let ours = self.added_fonts.contains(&num);
        if !ours {
            self.added_fonts.push(num);
        }
        let mut fonts = own_resource(&self.inner, page_obj, "Font")?;
        let mut free = None;
        for n in 0.. {
            let name = format!("GeraFn{n}");
            match fonts.get_dict(name.as_str()) {
                Ok(None) => {
                    free = Some(name);
                    break;
                }
                Ok(Some(f)) if ours && f.is_indirect()? && f.as_indirect()? == num => {
                    return Ok(name);
                }
                _ => {} // ocupado (ou ilegível): o próximo
            }
        }
        let name = free.expect("o laço só termina num nome livre");
        fonts.dict_put(name.as_str(), obj)?;
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_paragraphs_and_breaks_long_words() {
        let f = Font::new("Helvetica").unwrap();
        let lines = wrap(&f, "um dois três\nquatro", 10.0, 40.0, 0.0).unwrap();
        assert_eq!(lines.last().unwrap(), "quatro");
        assert!(lines.len() >= 3, "{lines:?}");
        let long = wrap(&f, "aaaaaaaaaaaaaaaaaaaaaaaa", 10.0, 30.0, 0.0).unwrap();
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
