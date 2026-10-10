//! Comparar arquivos (como no Acrobat): as diferenças de texto por palavra entre um PDF antigo e um
//! novo, com as páginas alinhadas aos pares.
use crate::doc::open_authenticated;
use crate::edit_text::strip_subset;
use crate::error::refused;
use crate::{PdfError, PdfRect, Result};
use md5::{Digest, Md5};
use mupdf::text_page::TextBlockType;
use mupdf::{Rect, TextPageFlags};
use similar::{capture_diff_slices_deadline, Algorithm, DiffOp};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Máximo de páginas de cada lado (memória).
const MAX_PAGES: usize = 20_000;
/// Prazo do diff: depois dele o resultado fica mais grosso, mas sai.
const DIFF_DEADLINE: Duration = Duration::from_secs(10);
/// Alinhamento ótimo das páginas só até este tamanho de tabela; acima, em sequência.
const MAX_ALIGN_CELLS: usize = 4_000_000;
/// Imagens maiores que isto (bytes dos pixels) não são decodificadas: são identificadas só pelas
/// dimensões (um PDF com uma imagem enorme e bem comprimida esgotaria a memória).
const MAX_DECODED_BYTES: u64 = 16_000_000;
/// Uma palavra que aparece em mais páginas que isto (de cada lado) é cabeçalho, rodapé ou palavra
/// comum: não ajuda a dizer que páginas formam par.
const MAX_RARE_PAGES: usize = 8;
/// Máximo de caracteres de cada trecho citado na descrição de uma alteração.
const QUOTE_CHARS: usize = 140;

/// Um dos dois arquivos.
#[derive(Clone, Debug, Default)]
pub struct CompareInput {
    pub path: PathBuf,
    pub password: Option<String>,
    /// Páginas escolhidas (base 0, em ordem); `None`: todas.
    pub pages: Option<Vec<i32>>,
}

#[derive(Clone, Debug, Default)]
pub struct CompareOptions {
    pub old: CompareInput,
    pub new: CompareInput,
    /// Só o texto: sem imagens.
    pub text_only: bool,
    /// Também as mudanças de fonte, tamanho e cor (desligado no Acrobat).
    pub formatting: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Replaced,
    Inserted,
    Deleted,
    Formatting,
    Image,
}

impl ChangeKind {
    pub const ALL: [ChangeKind; 5] = [
        ChangeKind::Replaced,
        ChangeKind::Inserted,
        ChangeKind::Deleted,
        ChangeKind::Formatting,
        ChangeKind::Image,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Replaced => "Substituído",
            ChangeKind::Inserted => "Inserido",
            ChangeKind::Deleted => "Excluído",
            ChangeKind::Formatting => "Formatação",
            ChangeKind::Image => "Imagem",
        }
    }

    /// Cor das marcas e da legenda.
    pub fn color(self) -> [f32; 3] {
        let hex = match self {
            ChangeKind::Replaced => 0x4F7BF0,
            ChangeKind::Inserted => 0x2EAD4B,
            ChangeKind::Deleted => 0xE5484D,
            ChangeKind::Formatting => 0xF2994A,
            ChangeKind::Image => 0x9B59D0,
        };
        [16, 8, 0].map(|s| ((hex >> s) & 0xFF) as f32 / 255.0)
    }

    /// Tom claro da cor (metade branco), para o destaque não escurecer o texto embaixo.
    pub fn tint(self) -> [f32; 3] {
        self.color().map(|c| c + (1.0 - c) * 0.5)
    }
}

/// Uma alteração, no par de páginas `pair`. Os retângulos estão nas coordenadas da página como
/// aparece na tela (origem no alto, com a rotação).
#[derive(Clone, Debug)]
pub struct Change {
    pub kind: ChangeKind,
    pub pair: usize,
    pub old_rects: Vec<PdfRect>,
    pub new_rects: Vec<PdfRect>,
    /// Do lado sem retângulos (inserção ou exclusão): onde o texto entrou ou saiu (largura zero).
    pub anchor: Option<PdfRect>,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct Comparison {
    /// Pares de páginas (número da página antiga, da nova); `None`: página sem par.
    pub pairs: Vec<(Option<i32>, Option<i32>)>,
    pub changes: Vec<Change>,
    /// Páginas comparadas de cada lado.
    pub old_pages: i32,
    pub new_pages: i32,
    /// Palavras lidas de cada lado (0: sem texto, talvez digitalizado).
    pub old_words: usize,
    pub new_words: usize,
}

/// Em que ponto está (para o progresso); páginas contadas a partir de 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareStage {
    ReadingOld(i32, i32),
    ReadingNew(i32, i32),
    Diffing,
    Writing(usize, usize),
}

/// Tetos contra arquivos maliciosos ou absurdos: um PDF pequeno pode repetir o mesmo conteúdo em
/// milhares de páginas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// Palavras de cada lado.
    pub max_words: usize,
    /// Imagens de cada lado (as seguintes ficam de fora da comparação).
    pub max_images: usize,
    /// Bytes de pixels decodificados de cada lado; depois, as imagens valem só pelas dimensões.
    pub decode_bytes: u64,
    /// Bytes de conteúdo das páginas copiadas para o resultado.
    pub content_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_words: 3_000_000,
            max_images: 50_000,
            decode_bytes: 512_000_000,
            content_bytes: 1_000_000_000,
        }
    }
}

/// Fonte (sem o prefixo de subconjunto, compartilhada entre as palavras), tamanho em meios pontos e
/// cor de uma palavra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Style {
    pub font: Arc<str>,
    pub half_points: i32,
    pub argb: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct WordAt {
    pub text: String,
    pub rect: PdfRect,
    pub style: Style,
    /// Índice da página entre as escolhidas.
    pub page: usize,
}

/// Uma imagem da página: o MD5 dos pixels e onde está.
#[derive(Clone, Debug)]
pub(crate) struct ImageAt {
    pub hash: [u8; 16],
    pub rect: PdfRect,
    pub page: usize,
}

/// O que foi lido de um dos arquivos.
pub(crate) struct Side {
    pub pages: Vec<i32>,
    /// Caixa de cada página escolhida, na tela.
    pub bounds: Vec<PdfRect>,
    pub words: Vec<WordAt>,
    pub images: Vec<ImageAt>,
}

impl Side {
    /// As palavras (e as imagens) de cada página, como faixas dos vetores (estão em ordem de página).
    fn by_page<T>(&self, items: &[T], page: impl Fn(&T) -> usize) -> Vec<Range<usize>> {
        let mut out = vec![0..0; self.pages.len()];
        let mut start = 0;
        while start < items.len() {
            let p = page(&items[start]);
            let end = start + items[start..].iter().take_while(|x| page(x) == p).count();
            out[p] = start..end;
            start = end;
        }
        out
    }
}

/// Se a imagem (largura × altura × componentes) cabe no limite para ser decodificada.
fn decodable(width: u32, height: u32, components: u8) -> bool {
    image_bytes(width, height, components) <= MAX_DECODED_BYTES
}

fn image_bytes(width: u32, height: u32, components: u8) -> u64 {
    u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(u64::from(components.max(1)))
}

/// Lê as palavras e as imagens das páginas escolhidas de um arquivo.
pub(crate) fn read_side(
    input: &CompareInput,
    images: bool,
    limits: &Limits,
    progress: &mut dyn FnMut(CompareStage) -> bool,
    stage: fn(i32, i32) -> CompareStage,
) -> Result<Side> {
    let doc = open_authenticated(&input.path, input.password.as_deref())?;
    // o resultado sai sem senha, com as páginas dentro: como no Combinar
    if !crate::security::pages_can_be_copied(&doc, input.password.as_deref())? {
        return Err(crate::security::restricted_source(&input.path));
    }
    let count = doc.page_count()?;
    let pages = match &input.pages {
        Some(p) => p.clone(),
        None => (0..count).collect(),
    };
    if pages.is_empty() {
        return Err(PdfError::InvalidPages("nenhuma página escolhida".into()));
    }
    if pages.len() > MAX_PAGES {
        return Err(refused(format!(
            "o arquivo tem mais de {MAX_PAGES} páginas para comparar"
        )));
    }
    if let Some(&p) = pages.iter().find(|&&p| p < 0 || p >= count) {
        return Err(PdfError::PageOutOfRange(p));
    }
    let total = pages.len() as i32;
    let mut words: Vec<WordAt> = Vec::new();
    let mut found = Vec::new();
    let mut bounds = Vec::with_capacity(pages.len());
    let mut fonts: HashMap<String, Arc<str>> = HashMap::new();
    let mut decode_left = limits.decode_bytes;
    let flags = if images {
        TextPageFlags::PRESERVE_IMAGES
    } else {
        TextPageFlags::empty()
    };
    for (k, &p) in pages.iter().enumerate() {
        if !progress(stage(k as i32 + 1, total)) {
            return Err(PdfError::Cancelled);
        }
        let page = doc.load_page(p)?;
        bounds.push(page.bounds()?.into());
        let tp = page.to_text_page(flags)?;
        for (b, block) in tp.blocks().enumerate() {
            // páginas com milhares de blocos: Cancelar responde sem esperar a página toda
            if b % 512 == 511 && !progress(stage(k as i32 + 1, total)) {
                return Err(PdfError::Cancelled);
            }
            if block.r#type() == TextBlockType::Image {
                if found.len() >= limits.max_images {
                    continue;
                }
                if let Some(img) = block.image() {
                    let (w, h, n) = (img.width(), img.height(), img.n());
                    let mut hash = Md5::new();
                    hash.update(w.to_le_bytes());
                    hash.update(h.to_le_bytes());
                    hash.update([n]);
                    let bytes = image_bytes(w, h, n);
                    if decodable(w, h, n) && bytes <= decode_left {
                        decode_left -= bytes;
                        hash.update(img.to_pixmap()?.samples());
                    }
                    found.push(ImageAt {
                        hash: hash.finalize().into(),
                        rect: block.bounds().into(),
                        page: k,
                    });
                }
                continue;
            }
            for line in block.lines() {
                let mut cur: Option<WordAt> = None;
                for ch in line.chars() {
                    let c = ch.char().unwrap_or('\u{FFFD}');
                    if c.is_whitespace() {
                        words.extend(cur.take());
                        continue;
                    }
                    let r: PdfRect = Rect::from(ch.quad()).into();
                    match &mut cur {
                        Some(w) => {
                            w.text.push(c);
                            w.rect = w.rect.union(&r);
                        }
                        None => {
                            let name = ch.font().map(|f| strip_subset(f.name()).to_owned());
                            let font = match name {
                                Some(n) => fonts
                                    .entry(n)
                                    .or_insert_with_key(|n| Arc::from(n.as_str()))
                                    .clone(),
                                None => Arc::from(""),
                            };
                            cur = Some(WordAt {
                                text: c.to_string(),
                                rect: r,
                                style: Style {
                                    font,
                                    half_points: (ch.size() * 2.0).round() as i32,
                                    argb: ch.argb(),
                                },
                                page: k,
                            })
                        }
                    }
                }
                words.extend(cur);
                if words.len() > limits.max_words {
                    return Err(refused(format!(
                        "o arquivo tem texto demais para comparar (mais de {} palavras)",
                        limits.max_words
                    )));
                }
            }
        }
    }
    Ok(Side {
        pages,
        bounds,
        words,
        images: found,
    })
}

/// O que distingue uma página: palavra ou imagem.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Token<'a> {
    Word(&'a str),
    Image([u8; 16]),
}

/// Quanto cada par de páginas tem em comum, contando só o que é raro (aparece em poucas páginas de
/// cada lado): cabeçalho, rodapé e palavras comuns iguais em toda página embaralhariam os pares.
fn rare_weights(old: &Side, new: &Side) -> HashMap<(usize, usize), u32> {
    // por token: em que páginas aparece de cada lado, e quantas vezes
    let mut index: HashMap<Token, [Vec<(usize, u32)>; 2]> = HashMap::new();
    for (side, s) in [old, new].into_iter().enumerate() {
        let tokens = s
            .words
            .iter()
            .map(|w| (Token::Word(&w.text), w.page))
            .chain(s.images.iter().map(|i| (Token::Image(i.hash), i.page)));
        for (t, page) in tokens {
            let list = &mut index.entry(t).or_default()[side];
            match list.iter_mut().find(|(p, _)| *p == page) {
                Some((_, n)) => *n += 1,
                None => list.push((page, 1)),
            }
        }
    }
    let limit = |n: usize| (n / 2).clamp(1, MAX_RARE_PAGES);
    let (lo, ln) = (limit(old.pages.len()), limit(new.pages.len()));
    let mut weights: HashMap<(usize, usize), u32> = HashMap::new();
    for [a, b] in index.values() {
        if a.is_empty() || b.is_empty() || a.len() > lo || b.len() > ln {
            continue;
        }
        for &(i, ci) in a {
            for &(j, cj) in b {
                *weights.entry((i, j)).or_default() += ci.min(cj);
            }
        }
    }
    weights
}

/// Alinha as `n` páginas antigas com as `m` novas, em ordem, maximizando as palavras iguais de cada
/// par (`weights`, por índice). Emparelhar vale 2·iguais + 1: páginas reescritas por inteiro ainda
/// formam par; uma fica sozinha só quando isso deixa pares melhores.
pub(crate) fn align_pages(
    n: usize,
    m: usize,
    weights: &HashMap<(usize, usize), u32>,
) -> Vec<(Option<usize>, Option<usize>)> {
    if n.saturating_mul(m) > MAX_ALIGN_CELLS {
        return (0..n.max(m))
            .map(|i| ((i < n).then_some(i), (i < m).then_some(i)))
            .collect();
    }
    // tabela densa: o laço abaixo consulta todas as células
    let mut dense = vec![0u32; n * m];
    for (&(i, j), &w) in weights {
        if i < n && j < m {
            dense[i * m + j] = w;
        }
    }
    let cols = m + 1;
    let mut score = vec![0u64; (n + 1) * cols];
    for i in 1..=n {
        for j in 1..=m {
            let w = u64::from(dense[(i - 1) * m + j - 1]) * 2 + 1;
            let pair = score[(i - 1) * cols + j - 1] + w;
            let up = score[(i - 1) * cols + j];
            let left = score[i * cols + j - 1];
            score[i * cols + j] = pair.max(up).max(left);
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let here = score[i * cols + j];
        // nos empates, a página sem par fica por último (os pares vêm antes)
        if j > 0 && (i == 0 || score[i * cols + j - 1] == here) {
            out.push((None, Some(j - 1)));
            j -= 1;
        } else if i > 0 && (j == 0 || score[(i - 1) * cols + j] == here) {
            out.push((Some(i - 1), None));
            i -= 1;
        } else {
            out.push((Some(i - 1), Some(j - 1)));
            i -= 1;
            j -= 1;
        }
    }
    out.reverse();
    out
}

/// Junta os retângulos de palavras seguidas da mesma linha.
fn merge_lines(rects: impl IntoIterator<Item = PdfRect>) -> Vec<PdfRect> {
    let mut out: Vec<PdfRect> = Vec::new();
    for r in rects {
        match out.last_mut() {
            Some(last)
                if (last.y0 - r.y0).abs() < 0.5 * (r.y1 - r.y0).max(1.0)
                    && r.x0 >= last.x0 - 1.0 =>
            {
                *last = last.union(&r)
            }
            _ => out.push(r),
        }
    }
    out
}

/// Trecho citado: as palavras, cortado em `QUOTE_CHARS` caracteres.
fn quote(words: &[&WordAt]) -> String {
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    match text.char_indices().nth(QUOTE_CHARS) {
        Some((cut, _)) => format!("«{}…»", &text[..cut]),
        None => format!("«{text}»"),
    }
}

/// Marca de largura zero onde o texto entrou ou saiu: depois da palavra anterior da página, ou antes
/// da seguinte.
fn anchor(words: &[WordAt], at: usize) -> Option<PdfRect> {
    let caret = |x: f32, r: &PdfRect| PdfRect::new(x, r.y0, x, r.y1);
    at.checked_sub(1)
        .and_then(|i| words.get(i))
        .map(|w| caret(w.rect.x1, &w.rect))
        .or_else(|| words.get(at).map(|w| caret(w.rect.x0, &w.rect)))
}

/// "Helvetica 12" ou "Helvetica 10,5", com a cor quando ela difere da outra.
fn style_text(s: &Style, other: &Style) -> String {
    let size = if s.half_points % 2 == 0 {
        (s.half_points / 2).to_string()
    } else {
        format!("{},5", s.half_points / 2)
    };
    let color = if s.argb & 0xFF_FFFF != other.argb & 0xFF_FFFF {
        format!(" #{:06X}", s.argb & 0xFF_FFFF)
    } else {
        String::new()
    };
    format!("{} {size}{color}", s.font)
}

/// Diff com prazo (o resultado fica mais grosso depois dele, mas sai).
fn diff<T: Eq + std::hash::Hash>(a: &[T], b: &[T], deadline: Instant) -> Vec<DiffOp> {
    capture_diff_slices_deadline(Algorithm::Patience, a, b, Some(deadline))
}

/// Os pares iguais de cada `Equal` (índice antigo, índice novo).
fn equal_pairs(ops: &[DiffOp]) -> impl Iterator<Item = (usize, usize)> + '_ {
    ops.iter()
        .filter_map(|op| match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => Some((0..len).map(move |k| (old_index + k, new_index + k))),
            _ => None,
        })
        .flatten()
}

fn texts(words: &[WordAt]) -> Vec<&str> {
    words.iter().map(|w| w.text.as_str()).collect()
}

/// As alterações de texto de um par de páginas.
fn text_changes(pair: usize, a: &[WordAt], b: &[WordAt], ops: &[DiffOp], out: &mut Vec<Change>) {
    for op in ops.iter().filter(|op| !matches!(op, DiffOp::Equal { .. })) {
        let (o, n) = (op.old_range(), op.new_range());
        let (ow, nw): (Vec<&WordAt>, Vec<&WordAt>) =
            (a[o.clone()].iter().collect(), b[n.clone()].iter().collect());
        let (kind, description, anchor) = match (ow.is_empty(), nw.is_empty()) {
            (false, false) => (
                ChangeKind::Replaced,
                format!("Substituído: {} → {}", quote(&ow), quote(&nw)),
                None,
            ),
            (true, _) => (
                ChangeKind::Inserted,
                format!("Inserido: {}", quote(&nw)),
                anchor(a, o.start),
            ),
            (_, true) => (
                ChangeKind::Deleted,
                format!("Excluído: {}", quote(&ow)),
                anchor(b, n.start),
            ),
        };
        out.push(Change {
            kind,
            pair,
            old_rects: merge_lines(ow.iter().map(|w| w.rect)),
            new_rects: merge_lines(nw.iter().map(|w| w.rect)),
            anchor,
            description,
        });
    }
}

/// Palavras iguais seguidas com a mesma mudança de fonte, tamanho ou cor: uma alteração só.
fn format_changes(pair: usize, a: &[WordAt], b: &[WordAt], ops: &[DiffOp], out: &mut Vec<Change>) {
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut flush = |run: &mut Vec<(usize, usize)>| {
        let Some(&(i, j)) = run.first() else { return };
        let (os, ns) = (&a[i].style, &b[j].style);
        out.push(Change {
            kind: ChangeKind::Formatting,
            pair,
            old_rects: merge_lines(run.iter().map(|&(i, _)| a[i].rect)),
            new_rects: merge_lines(run.iter().map(|&(_, j)| b[j].rect)),
            anchor: None,
            description: format!(
                "Formatação: {} → {}",
                style_text(os, ns),
                style_text(ns, os)
            ),
        });
        run.clear();
    };
    for (i, j) in equal_pairs(ops) {
        let continues = run.last().is_some_and(|&(pi, pj)| {
            pi + 1 == i && pj + 1 == j && a[pi].style == a[i].style && b[pj].style == b[j].style
        });
        if !continues {
            flush(&mut run);
        }
        if a[i].style != b[j].style {
            run.push((i, j));
        }
    }
    flush(&mut run);
}

fn image_change(pair: usize, old: Option<PdfRect>, new: Option<PdfRect>) -> Change {
    let what = match (old, new) {
        (Some(_), Some(_)) => "substituída",
        (Some(_), None) => "excluída",
        _ => "inserida",
    };
    Change {
        kind: ChangeKind::Image,
        pair,
        old_rects: old.into_iter().collect(),
        new_rects: new.into_iter().collect(),
        anchor: None,
        description: format!("Imagem {what}"),
    }
}

/// As imagens de um par: uma de cada lado, na ordem, é substituída; o resto entra ou sai.
fn image_changes(
    pair: usize,
    a: &[ImageAt],
    b: &[ImageAt],
    deadline: Instant,
    out: &mut Vec<Change>,
) {
    let hashes = |x: &[ImageAt]| x.iter().map(|i| i.hash).collect::<Vec<_>>();
    for op in diff(&hashes(a), &hashes(b), deadline) {
        let (o, n) = (&a[op.old_range()], &b[op.new_range()]);
        for k in 0..o.len().max(n.len()) {
            if !matches!(op, DiffOp::Equal { .. }) {
                out.push(image_change(
                    pair,
                    o.get(k).map(|i| i.rect),
                    n.get(k).map(|i| i.rect),
                ));
            }
        }
    }
}

/// Página sem par: tudo nela entrou (`inserted`) ou saiu. Página vazia: a página inteira.
fn lone_page(
    pair: usize,
    side: &Side,
    page: usize,
    words: &[WordAt],
    images: &[ImageAt],
    inserted: bool,
    out: &mut Vec<Change>,
) {
    let (kind, label) = if inserted {
        (ChangeKind::Inserted, "Página inserida")
    } else {
        (ChangeKind::Deleted, "Página excluída")
    };
    let place = |rects: Vec<PdfRect>| {
        if inserted {
            (Vec::new(), rects)
        } else {
            (rects, Vec::new())
        }
    };
    if !words.is_empty() || images.is_empty() {
        let refs: Vec<&WordAt> = words.iter().collect();
        let (description, rects) = if words.is_empty() {
            (label.to_string(), vec![side.bounds[page]])
        } else {
            (
                format!("{label}: {}", quote(&refs)),
                merge_lines(words.iter().map(|w| w.rect)),
            )
        };
        let (old_rects, new_rects) = place(rects);
        out.push(Change {
            kind,
            pair,
            old_rects,
            new_rects,
            anchor: None,
            description,
        });
    }
    for i in images {
        let (o, n) = place(vec![i.rect]);
        out.push(image_change(pair, o.first().copied(), n.first().copied()));
    }
}

/// Compara os dois arquivos. `progress` devolve `false` para cancelar.
pub fn compare(
    opts: &CompareOptions,
    progress: &mut dyn FnMut(CompareStage) -> bool,
) -> Result<Comparison> {
    compare_with(opts, &Limits::default(), progress)
}

pub(crate) fn compare_with(
    opts: &CompareOptions,
    limits: &Limits,
    progress: &mut dyn FnMut(CompareStage) -> bool,
) -> Result<Comparison> {
    let images = !opts.text_only;
    let old = read_side(
        &opts.old,
        images,
        limits,
        progress,
        CompareStage::ReadingOld,
    )?;
    let new = read_side(
        &opts.new,
        images,
        limits,
        progress,
        CompareStage::ReadingNew,
    )?;
    if !progress(CompareStage::Diffing) {
        return Err(PdfError::Cancelled);
    }
    // primeiro os pares de páginas (pelo que é raro); depois o diff dentro de cada par
    let aligned = align_pages(old.pages.len(), new.pages.len(), &rare_weights(&old, &new));
    let (ow, nw) = (
        old.by_page(&old.words, |w| w.page),
        new.by_page(&new.words, |w| w.page),
    );
    let (oi, ni) = (
        old.by_page(&old.images, |i| i.page),
        new.by_page(&new.images, |i| i.page),
    );
    let deadline = Instant::now() + DIFF_DEADLINE;
    let mut changes = Vec::new();
    for (pair, &(o, n)) in aligned.iter().enumerate() {
        if !progress(CompareStage::Diffing) {
            return Err(PdfError::Cancelled);
        }
        match (o, n) {
            (Some(i), Some(j)) => {
                let (a, b) = (&old.words[ow[i].clone()], &new.words[nw[j].clone()]);
                let ops = diff(&texts(a), &texts(b), deadline);
                text_changes(pair, a, b, &ops, &mut changes);
                if opts.formatting {
                    format_changes(pair, a, b, &ops, &mut changes);
                }
                image_changes(
                    pair,
                    &old.images[oi[i].clone()],
                    &new.images[ni[j].clone()],
                    deadline,
                    &mut changes,
                );
            }
            (None, Some(j)) => lone_page(
                pair,
                &new,
                j,
                &new.words[nw[j].clone()],
                &new.images[ni[j].clone()],
                true,
                &mut changes,
            ),
            (Some(i), None) => lone_page(
                pair,
                &old,
                i,
                &old.words[ow[i].clone()],
                &old.images[oi[i].clone()],
                false,
                &mut changes,
            ),
            (None, None) => {}
        }
    }
    let pairs = aligned
        .iter()
        .map(|(o, n)| (o.map(|i| old.pages[i]), n.map(|j| new.pages[j])))
        .collect();
    Ok(Comparison {
        pairs,
        changes,
        old_pages: old.pages.len() as i32,
        new_pages: new.pages.len() as i32,
        old_words: old.words.len(),
        new_words: new.words.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(cells: &[(usize, usize, u32)]) -> HashMap<(usize, usize), u32> {
        cells.iter().map(|&(i, j, w)| ((i, j), w)).collect()
    }

    #[test]
    fn same_pages_pair_in_order() {
        let w = weights(&[(0, 0, 10), (1, 1, 10)]);
        assert_eq!(
            align_pages(2, 2, &w),
            vec![(Some(0), Some(0)), (Some(1), Some(1))]
        );
    }

    #[test]
    fn inserted_page_has_no_pair() {
        let w = weights(&[(0, 0, 10), (1, 2, 10)]);
        assert_eq!(
            align_pages(2, 3, &w),
            vec![(Some(0), Some(0)), (None, Some(1)), (Some(1), Some(2))]
        );
    }

    #[test]
    fn deleted_page_has_no_pair() {
        let w = weights(&[(0, 0, 10), (2, 1, 10)]);
        assert_eq!(
            align_pages(3, 2, &w),
            vec![(Some(0), Some(0)), (Some(1), None), (Some(2), Some(1))]
        );
    }

    #[test]
    fn rewritten_pages_still_pair_in_order() {
        assert_eq!(
            align_pages(2, 3, &HashMap::new()),
            vec![(Some(0), Some(0)), (Some(1), Some(1)), (None, Some(2))]
        );
    }

    #[test]
    fn huge_documents_pair_in_sequence() {
        let n = 2001;
        let pairs = align_pages(n, n + 2000, &HashMap::new());
        assert_eq!(pairs.len(), n + 2000);
        assert_eq!(pairs[0], (Some(0), Some(0)));
        assert_eq!(pairs[n], (None, Some(n)));
    }

    fn sample_input(dir: &std::path::Path, pages: usize) -> CompareInput {
        let path = dir.join("amostra.pdf");
        std::fs::write(&path, crate::sample::sample_pdf(pages, 0)).unwrap();
        CompareInput {
            path,
            password: None,
            pages: None,
        }
    }

    #[test]
    fn too_many_words_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let input = sample_input(dir.path(), 3);
        let limits = Limits {
            max_words: 5,
            ..Limits::default()
        };
        let r = read_side(
            &input,
            true,
            &limits,
            &mut |_| true,
            CompareStage::ReadingOld,
        );
        assert!(matches!(r, Err(PdfError::Refused(m)) if m.contains("texto demais")));
        let ok = read_side(
            &input,
            true,
            &Limits::default(),
            &mut |_| true,
            CompareStage::ReadingOld,
        );
        assert_eq!(ok.unwrap().words.len(), 12);
    }

    /// PDF de 2 páginas, cada uma com uma imagem cinza 2×2 de tons diferentes.
    fn two_images(dir: &std::path::Path) -> CompareInput {
        let mut objs = vec![
            "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
            "2 0 obj<</Type/Pages/Kids[3 0 R 5 0 R]/Count 2>>endobj\n".into(),
        ];
        for (page, img, gray) in [(3, 4, 'A'), (5, 6, 'z')] {
            objs.push(format!(
                "{page} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Resources<</XObject<</I {img} 0 R>>>>/Contents 7 0 R>>endobj\n"
            ));
            objs.push(format!(
                "{img} 0 obj<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceGray/BitsPerComponent 8/Length 4>>stream\n{g}{g}{g}{g}\nendstream endobj\n",
                g = gray
            ));
        }
        let ops = "q 100 0 0 100 50 50 cm /I Do Q";
        objs.push(format!(
            "7 0 obj<</Length {}>>stream\n{ops}\nendstream endobj\n",
            ops.len() + 1
        ));
        let path = dir.join("imagens.pdf");
        std::fs::write(
            &path,
            format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
        )
        .unwrap();
        CompareInput {
            path,
            password: None,
            pages: None,
        }
    }

    #[test]
    fn decoding_stops_at_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let input = two_images(dir.path());
        let read = |decode_bytes| {
            let limits = Limits {
                decode_bytes,
                ..Limits::default()
            };
            read_side(
                &input,
                true,
                &limits,
                &mut |_| true,
                CompareStage::ReadingOld,
            )
            .unwrap()
        };
        let all = read(1000);
        assert_ne!(all.images[0].hash, all.images[1].hash);
        // só a primeira (4 bytes) cabe: a segunda vale só pelas dimensões
        let budget = read(4);
        assert_eq!(budget.images[0].hash, all.images[0].hash);
        assert_ne!(budget.images[1].hash, all.images[1].hash);
        let few = read_side(
            &input,
            true,
            &Limits {
                max_images: 1,
                ..Limits::default()
            },
            &mut |_| true,
            CompareStage::ReadingOld,
        )
        .unwrap();
        assert_eq!(few.images.len(), 1);
    }

    #[test]
    fn fonts_are_shared_between_words() {
        let dir = tempfile::tempdir().unwrap();
        let input = sample_input(dir.path(), 2);
        let side = read_side(
            &input,
            false,
            &Limits::default(),
            &mut |_| true,
            CompareStage::ReadingOld,
        )
        .unwrap();
        assert!(Arc::ptr_eq(
            &side.words[0].style.font,
            &side.words[5].style.font
        ));
    }

    #[test]
    fn pages_split_into_ranges() {
        let side = Side {
            pages: vec![0, 1, 2],
            bounds: vec![PdfRect::default(); 3],
            words: Vec::new(),
            images: Vec::new(),
        };
        let pages = [0usize, 0, 2, 2, 2];
        assert_eq!(side.by_page(&pages, |p| *p), vec![0..2, 0..0, 2..5]);
    }

    #[test]
    fn huge_images_are_not_decoded() {
        assert!(decodable(2000, 2000, 3));
        assert!(!decodable(5000, 5000, 1));
        assert!(!decodable(u32::MAX, u32::MAX, 4));
    }

    #[test]
    fn colors_come_from_the_legend() {
        assert_eq!(
            ChangeKind::Deleted.color(),
            [
                0xE5 as f32 / 255.0,
                0x48 as f32 / 255.0,
                0x4D as f32 / 255.0
            ]
        );
    }
}
