//! Comparar arquivos (como no Acrobat): as diferenças de texto por palavra entre um PDF antigo e um
//! novo, com as páginas alinhadas aos pares.
use crate::doc::open_authenticated;
use crate::error::refused;
use crate::{PdfError, PdfRect, Result};
use mupdf::{Rect, TextPageFlags};
use similar::{capture_diff_slices_deadline, Algorithm, DiffOp};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Máximo de páginas de cada lado (memória).
const MAX_PAGES: usize = 20_000;
/// Prazo do diff: depois dele o resultado fica mais grosso, mas sai.
const DIFF_DEADLINE: Duration = Duration::from_secs(10);
/// Alinhamento ótimo das páginas só até este tamanho de tabela; acima, em sequência.
const MAX_ALIGN_CELLS: usize = 4_000_000;
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
}

/// Em que ponto está (para o progresso); páginas contadas a partir de 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareStage {
    ReadingOld(i32, i32),
    ReadingNew(i32, i32),
    Diffing,
    Writing(usize, usize),
}

/// Fonte (sem o prefixo de subconjunto), tamanho em meios pontos e cor de uma palavra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Style {
    pub font: String,
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

/// O que foi lido de um dos arquivos.
pub(crate) struct Side {
    pub pages: Vec<i32>,
    pub words: Vec<WordAt>,
}

pub(crate) fn cancelled() -> PdfError {
    refused("comparação cancelada")
}

/// Nome da fonte sem o prefixo de subconjunto ("ABCDEF+Helvetica" → "Helvetica").
fn base_font(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

fn union(a: PdfRect, b: PdfRect) -> PdfRect {
    PdfRect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// Lê as palavras das páginas escolhidas de um arquivo.
pub(crate) fn read_side(
    input: &CompareInput,
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
    let mut words = Vec::new();
    for (k, &p) in pages.iter().enumerate() {
        if !progress(stage(k as i32 + 1, total)) {
            return Err(cancelled());
        }
        let tp = doc.load_page(p)?.to_text_page(TextPageFlags::empty())?;
        for block in tp.blocks() {
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
                            w.rect = union(w.rect, r);
                        }
                        None => {
                            cur = Some(WordAt {
                                text: c.to_string(),
                                rect: r,
                                style: Style {
                                    font: ch
                                        .font()
                                        .map(|f| base_font(f.name()).to_owned())
                                        .unwrap_or_default(),
                                    half_points: (ch.size() * 2.0).round() as i32,
                                    argb: ch.argb(),
                                },
                                page: k,
                            })
                        }
                    }
                }
                words.extend(cur);
            }
        }
    }
    Ok(Side { pages, words })
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
    let w = |i: usize, j: usize| u64::from(*weights.get(&(i, j)).unwrap_or(&0)) * 2 + 1;
    let cols = m + 1;
    let mut score = vec![0u64; (n + 1) * cols];
    for i in 1..=n {
        for j in 1..=m {
            let pair = score[(i - 1) * cols + j - 1] + w(i - 1, j - 1);
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
                *last = union(*last, r)
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

/// Marca de largura zero onde o texto entrou ou saiu: depois da palavra anterior, ou antes da
/// seguinte, se ela estiver no mesmo par.
fn anchor(words: &[WordAt], at: usize, pair_of: &[usize], pair: usize) -> Option<PdfRect> {
    let caret = |x: f32, r: &PdfRect| PdfRect {
        x0: x,
        y0: r.y0,
        x1: x,
        y1: r.y1,
    };
    let before = at
        .checked_sub(1)
        .map(|i| &words[i])
        .filter(|w| pair_of[w.page] == pair);
    let after = words.get(at).filter(|w| pair_of[w.page] == pair);
    before
        .map(|w| caret(w.rect.x1, &w.rect))
        .or_else(|| after.map(|w| caret(w.rect.x0, &w.rect)))
}

/// Compara os dois arquivos. `progress` devolve `false` para cancelar.
pub fn compare(
    opts: &CompareOptions,
    progress: &mut dyn FnMut(CompareStage) -> bool,
) -> Result<Comparison> {
    let old = read_side(&opts.old, progress, CompareStage::ReadingOld)?;
    let new = read_side(&opts.new, progress, CompareStage::ReadingNew)?;
    if !progress(CompareStage::Diffing) {
        return Err(cancelled());
    }
    let a: Vec<&str> = old.words.iter().map(|w| w.text.as_str()).collect();
    let b: Vec<&str> = new.words.iter().map(|w| w.text.as_str()).collect();
    let ops = capture_diff_slices_deadline(
        Algorithm::Patience,
        &a,
        &b,
        Some(Instant::now() + DIFF_DEADLINE),
    );
    let mut weights: HashMap<(usize, usize), u32> = HashMap::new();
    for op in &ops {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = *op
        {
            for k in 0..len {
                *weights
                    .entry((old.words[old_index + k].page, new.words[new_index + k].page))
                    .or_default() += 1;
            }
        }
    }
    let aligned = align_pages(old.pages.len(), new.pages.len(), &weights);
    let mut old_pair = vec![0; old.pages.len()];
    let mut new_pair = vec![0; new.pages.len()];
    for (k, (o, n)) in aligned.iter().enumerate() {
        if let Some(o) = o {
            old_pair[*o] = k;
        }
        if let Some(n) = n {
            new_pair[*n] = k;
        }
    }
    let mut changes = Vec::new();
    for op in &ops {
        let (o, n) = (op.old_range(), op.new_range());
        if matches!(op, DiffOp::Equal { .. }) {
            continue;
        }
        // por par: as palavras antigas e as novas
        let mut groups: BTreeMap<usize, (Vec<&WordAt>, Vec<&WordAt>)> = BTreeMap::new();
        for w in &old.words[o.clone()] {
            groups.entry(old_pair[w.page]).or_default().0.push(w);
        }
        for w in &new.words[n.clone()] {
            groups.entry(new_pair[w.page]).or_default().1.push(w);
        }
        for (pair, (ow, nw)) in groups {
            let (kind, description, anchor) = match (ow.is_empty(), nw.is_empty()) {
                (false, false) => (
                    ChangeKind::Replaced,
                    format!("Substituído: {} → {}", quote(&ow), quote(&nw)),
                    None,
                ),
                (true, _) => (
                    ChangeKind::Inserted,
                    format!("Inserido: {}", quote(&nw)),
                    anchor(&old.words, o.start, &old_pair, pair),
                ),
                (_, true) => (
                    ChangeKind::Deleted,
                    format!("Excluído: {}", quote(&ow)),
                    anchor(&new.words, n.start, &new_pair, pair),
                ),
            };
            changes.push(Change {
                kind,
                pair,
                old_rects: merge_lines(ow.iter().map(|w| w.rect)),
                new_rects: merge_lines(nw.iter().map(|w| w.rect)),
                anchor,
                description,
            });
        }
    }
    changes.sort_by_key(|c| c.pair);
    let pairs = aligned
        .iter()
        .map(|(o, n)| (o.map(|i| old.pages[i]), n.map(|j| new.pages[j])))
        .collect();
    Ok(Comparison {
        pairs,
        changes,
        old_pages: old.pages.len() as i32,
        new_pages: new.pages.len() as i32,
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

    #[test]
    fn subset_prefix_is_dropped() {
        assert_eq!(base_font("ABCDEF+Helvetica"), "Helvetica");
        assert_eq!(base_font("Abc+X"), "Abc+X");
    }

    #[test]
    fn colors_come_from_the_legend() {
        assert_eq!(
            ChangeKind::Deleted.color(),
            [0xE5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4D as f32 / 255.0]
        );
    }
}
