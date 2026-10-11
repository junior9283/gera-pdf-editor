//! Painel Comentários: conversa (comentário, respostas, status e visto), filtros, ordenação e busca.
use crate::review::{ReviewState, MARKED};
use pdf_core::{AnnotInfo, AnnotKind, StateModel};
use std::collections::HashMap;

/// Um comentário com a conversa: respostas (por data), status de revisão e visto vigentes.
#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub root: AnnotInfo,
    pub replies: Vec<AnnotInfo>,
    /// Status vigente e quem o definiu.
    pub review: Option<(ReviewState, String)>,
    pub checked: bool,
    /// Tem anotações agrupadas (Substituir texto: o cursor com o riscado).
    pub grouped: bool,
}

impl Thread {
    /// Status para filtrar: sem status definido conta como Nenhum.
    pub fn status(&self) -> ReviewState {
        self.review.as_ref().map_or(ReviewState::None, |r| r.0)
    }
}

/// Data do PDF (`D:AAAAMMDDhhmmss` com fuso `Z`, `+hh'mm'` ou `-hh'mm'`) em segundos UTC, para
/// comparar datas de programas e fusos diferentes. Inválida: o menor valor.
pub fn utc_key(raw: &str) -> i64 {
    let s = raw.trim().strip_prefix("D:").unwrap_or(raw.trim());
    let num = |a: usize, b: usize, default: i64| -> Option<i64> {
        match s.get(a..b) {
            Some(t) if t.bytes().all(|c| c.is_ascii_digit()) && !t.is_empty() => t.parse().ok(),
            Some(_) => None,
            None => Some(default),
        }
    };
    let parsed = (|| {
        let y = num(0, 4, -1).filter(|y| *y >= 0)?;
        let (mo, d) = (num(4, 6, 1)?, num(6, 8, 1)?);
        let (h, mi, se) = (num(8, 10, 0)?, num(10, 12, 0)?, num(12, 14, 0)?);
        let tz = s.get(14..).unwrap_or("");
        let offset = match tz.chars().next() {
            Some(sign @ ('+' | '-')) => {
                let digits: String = tz[1..].chars().filter(char::is_ascii_digit).collect();
                let oh: i64 = digits.get(0..2).and_then(|t| t.parse().ok()).unwrap_or(0);
                let om: i64 = digits.get(2..4).and_then(|t| t.parse().ok()).unwrap_or(0);
                let m = oh * 60 + om;
                if sign == '-' {
                    -m
                } else {
                    m
                }
            }
            _ => 0,
        };
        // dias desde 1970-01-01 (algoritmo civil de Howard Hinnant)
        let (yy, mm) = if mo <= 2 {
            (y - 1, mo + 9)
        } else {
            (y, mo - 3)
        };
        let era = yy.div_euclid(400);
        let yoe = yy - era * 400;
        let doy = (153 * mm + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(days * 86_400 + h * 3600 + mi * 60 + se - offset * 60)
    })();
    parsed.unwrap_or(i64::MIN)
}

/// Ordem cronológica de anotações: data de modificação em UTC e, no empate, o id (a mais nova
/// vence).
fn chrono_key(a: &AnnotInfo) -> (i64, i32) {
    (utc_key(&a.modified), a.id)
}

/// Monta as conversas: só os comentários (sem `reply_to`) viram linhas; respostas e status ficam
/// dentro do pai; os de pai inexistente (PDF de outro programa) são ignorados.
pub fn threads(annots: &[AnnotInfo]) -> Vec<Thread> {
    // `group` já vem conferido do pdf-core (só aponta para um principal da mesma página)
    let group_roots: std::collections::HashSet<i32> =
        annots.iter().filter_map(|a| a.group).collect();
    let mut out: Vec<Thread> = annots
        .iter()
        // o membro de um grupo (o riscado do Substituir texto) aparece no item da principal
        .filter(|a| !a.is_reply() && a.group.is_none())
        .map(|a| Thread {
            root: a.clone(),
            replies: Vec::new(),
            review: None,
            checked: false,
            grouped: group_roots.contains(&a.id),
        })
        .collect();
    let index: HashMap<i32, usize> = out
        .iter()
        .enumerate()
        .map(|(n, t)| (t.root.id, n))
        .collect();
    let mut children: Vec<&AnnotInfo> = annots.iter().filter(|a| a.is_reply()).collect();
    children.sort_by_cached_key(|a| chrono_key(a));
    let parent_of: HashMap<i32, i32> = annots
        .iter()
        .filter_map(|a| Some((a.id, a.reply_to?)))
        .collect();
    // a raiz de uma resposta (respostas de respostas sobem até o comentário)
    let root_of = |mut id: i32| -> Option<usize> {
        for _ in 0..64 {
            match index.get(&id) {
                Some(&n) => return Some(n),
                None => id = *parent_of.get(&id)?,
            }
        }
        None
    };
    for child in children {
        let Some(n) = child.reply_to.and_then(root_of) else {
            continue;
        };
        let t = &mut out[n];
        let on_root = child.reply_to == Some(t.root.id);
        match &child.state {
            None => t.replies.push(child.clone()),
            // status de uma resposta não é o do comentário
            Some(_) if !on_root => {}
            Some((StateModel::Review, s)) => {
                t.review = ReviewState::from_pdf(s).map(|r| (r, child.author.clone()));
            }
            Some((StateModel::Marked, s)) => t.checked = s == MARKED,
        }
    }
    out
}

/// Minúsculas sem acento, para a busca achar "Revisão" com "revisao".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Filtros do painel (vazios = sem filtro); todos se combinam.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentFilter {
    /// Busca no texto, no autor e nas respostas.
    pub text: String,
    pub kinds: Vec<AnnotKind>,
    pub authors: Vec<String>,
    pub statuses: Vec<ReviewState>,
    pub checked: Option<bool>,
}

impl CommentFilter {
    pub fn is_active(&self) -> bool {
        *self != Self::default()
    }

    pub fn matches(&self, t: &Thread) -> bool {
        let query = fold(self.text.trim());
        let found = query.is_empty()
            || std::iter::once(&t.root)
                .chain(&t.replies)
                .any(|a| fold(&a.contents).contains(&query) || fold(&a.author).contains(&query));
        found
            && (self.kinds.is_empty() || self.kinds.contains(&t.root.kind))
            && (self.authors.is_empty()
                || std::iter::once(&t.root)
                    .chain(&t.replies)
                    .any(|a| self.authors.contains(&a.author)))
            && (self.statuses.is_empty() || self.statuses.contains(&t.status()))
            && self.checked.is_none_or(|c| c == t.checked)
    }
}

/// Ordenar por (menu do painel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Page,
    Author,
    Date,
    Kind,
}

impl SortBy {
    pub const ALL: [SortBy; 4] = [Self::Page, Self::Author, Self::Date, Self::Kind];

    pub fn label(self) -> &'static str {
        match self {
            Self::Page => "Página",
            Self::Author => "Autor",
            Self::Date => "Data",
            Self::Kind => "Tipo",
        }
    }
}

/// Posição na página (de cima para baixo, da esquerda para a direita).
fn place(a: &AnnotInfo) -> (i32, i64, i64) {
    (a.page, a.rect.y0 as i64, a.rect.x0 as i64)
}

pub fn sorted(mut threads: Vec<Thread>, by: SortBy) -> Vec<Thread> {
    match by {
        SortBy::Page => threads.sort_by_key(|t| place(&t.root)),
        SortBy::Author => threads.sort_by_cached_key(|t| (fold(&t.root.author), place(&t.root))),
        SortBy::Date => threads.sort_by_cached_key(|t| chrono_key(&t.root)),
        SortBy::Kind => threads.sort_by_key(|t| (t.root.kind as u8, place(&t.root))),
    }
    threads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: i32, page: i32, author: &str, modified: &str, text: &str) -> AnnotInfo {
        AnnotInfo {
            id,
            page,
            kind: AnnotKind::Note,
            author: author.into(),
            modified: modified.into(),
            created: modified.into(),
            contents: text.into(),
            opacity: 1.0,
            ..Default::default()
        }
    }

    fn reply(id: i32, parent: i32, author: &str, modified: &str, text: &str) -> AnnotInfo {
        AnnotInfo {
            reply_to: Some(parent),
            ..a(id, 0, author, modified, text)
        }
    }

    fn state(id: i32, parent: i32, model: StateModel, s: &str, author: &str, m: &str) -> AnnotInfo {
        AnnotInfo {
            state: Some((model, s.into())),
            ..reply(id, parent, author, m, "")
        }
    }

    #[test]
    fn replies_go_under_their_parent_sorted_by_date() {
        let all = vec![
            a(1, 0, "Ana", "D:20261009100000", "pai"),
            reply(3, 1, "Bia", "D:20261009120000", "segunda"),
            reply(2, 1, "Caio", "D:20261009110000", "primeira"),
        ];
        let t = threads(&all);
        assert_eq!(t.len(), 1);
        let texts: Vec<_> = t[0].replies.iter().map(|r| r.contents.as_str()).collect();
        assert_eq!(texts, ["primeira", "segunda"]);
    }

    #[test]
    fn a_replace_text_pair_is_one_thread() {
        let caret = AnnotInfo {
            kind: AnnotKind::Caret,
            ..a(1, 0, "Ana", "D:20261009100000", "novo texto")
        };
        let strike = AnnotInfo {
            kind: AnnotKind::StrikeOut,
            group: Some(1),
            ..a(2, 0, "Ana", "D:20261009100000", "")
        };
        let t = threads(&[caret, strike]);
        assert_eq!(t.len(), 1, "o riscado agrupado não vira linha");
        assert!(t[0].grouped, "o cursor tem par: é Substituir texto");
        assert_eq!(t[0].root.id, 1);
    }

    #[test]
    fn orphan_replies_are_ignored() {
        let all = vec![
            a(1, 0, "Ana", "D:20261009100000", "pai"),
            reply(2, 99, "Bia", "D:20261009110000", "órfã"),
        ];
        let t = threads(&all);
        assert_eq!(t.len(), 1);
        assert!(t[0].replies.is_empty());
    }

    #[test]
    fn current_state_is_the_latest() {
        let all = vec![
            a(1, 0, "Ana", "D:20261009100000", "pai"),
            state(
                2,
                1,
                StateModel::Review,
                "Accepted",
                "Bia",
                "D:20261009120000",
            ),
            state(
                3,
                1,
                StateModel::Review,
                "Rejected",
                "Caio",
                "D:20261009110000",
            ),
            state(
                4,
                1,
                StateModel::Review,
                "Completed",
                "Davi",
                "D:20261009120000",
            ),
        ];
        let t = threads(&all);
        assert_eq!(
            t[0].review,
            Some((ReviewState::Completed, "Davi".into())),
            "empate: maior id"
        );
        assert!(t[0].replies.is_empty(), "status não é resposta");
    }

    #[test]
    fn checked_follows_the_latest_marked_state() {
        let mut all = vec![
            a(1, 0, "Ana", "D:20261009100000", "pai"),
            state(
                2,
                1,
                StateModel::Marked,
                "Marked",
                "Ana",
                "D:20261009110000",
            ),
        ];
        assert!(threads(&all)[0].checked);
        all.push(state(
            3,
            1,
            StateModel::Marked,
            "Unmarked",
            "Ana",
            "D:20261009120000",
        ));
        assert!(!threads(&all)[0].checked);
    }

    #[test]
    fn filters_combine() {
        let mut ink = a(5, 1, "Bia", "D:20261009100000", "");
        ink.kind = AnnotKind::Ink;
        let all = vec![
            a(1, 0, "Ana", "D:20261009100000", "nota"),
            state(
                2,
                1,
                StateModel::Review,
                "Accepted",
                "Ana",
                "D:20261009110000",
            ),
            state(
                3,
                1,
                StateModel::Marked,
                "Marked",
                "Ana",
                "D:20261009110000",
            ),
            ink,
        ];
        let t = threads(&all);
        let f = CommentFilter {
            kinds: vec![AnnotKind::Note],
            statuses: vec![ReviewState::Accepted],
            checked: Some(true),
            ..Default::default()
        };
        assert!(f.is_active());
        let kept: Vec<_> = t
            .iter()
            .filter(|t| f.matches(t))
            .map(|t| t.root.id)
            .collect();
        assert_eq!(kept, [1]);
        let none = CommentFilter {
            statuses: vec![ReviewState::None],
            ..Default::default()
        };
        let kept: Vec<_> = t
            .iter()
            .filter(|t| none.matches(t))
            .map(|t| t.root.id)
            .collect();
        assert_eq!(kept, [5], "sem status conta como Nenhum");
        assert!(!CommentFilter::default().is_active());
    }

    #[test]
    fn search_ignores_accents_and_case() {
        let all = vec![
            a(1, 0, "Ana Lúcia", "D:20261009100000", "Revisão do contrato"),
            a(2, 0, "Bia", "D:20261009100000", "ok"),
            reply(3, 2, "Caio", "D:20261009110000", "Ver cláusula"),
        ];
        let t = threads(&all);
        let find = |q: &str| -> Vec<i32> {
            let f = CommentFilter {
                text: q.into(),
                ..Default::default()
            };
            t.iter()
                .filter(|t| f.matches(t))
                .map(|t| t.root.id)
                .collect()
        };
        assert_eq!(find("revisao"), [1]);
        assert_eq!(find("LUCIA"), [1], "autor também");
        assert_eq!(find("clausula"), [2], "texto das respostas também");
        assert_eq!(fold("ÁÉÍÓÚ Çã õ"), "aeiou ca o");
    }

    #[test]
    fn sort_by_author_and_date() {
        let all = vec![
            a(1, 2, "Caio", "D:20261009100000", ""),
            a(2, 0, "ana", "D:20261009120000", ""),
            a(3, 1, "Bia", "D:20261009110000", ""),
        ];
        let ids = |by| -> Vec<i32> {
            sorted(threads(&all), by)
                .iter()
                .map(|t| t.root.id)
                .collect()
        };
        assert_eq!(ids(SortBy::Page), [2, 3, 1]);
        assert_eq!(ids(SortBy::Author), [2, 3, 1], "sem diferenciar maiúsculas");
        assert_eq!(ids(SortBy::Date), [1, 3, 2]);
    }

    #[test]
    fn replies_of_replies_join_the_root_thread() {
        let all = vec![
            a(1, 0, "Ana", "D:20261009100000", "pai"),
            reply(2, 1, "Bia", "D:20261009110000", "filha"),
            reply(3, 2, "Caio", "D:20261009120000", "neta"),
        ];
        let t = threads(&all);
        assert_eq!(t.len(), 1);
        let texts: Vec<_> = t[0].replies.iter().map(|r| r.contents.as_str()).collect();
        assert_eq!(texts, ["filha", "neta"]);
    }

    #[test]
    fn dates_compare_in_utc() {
        // 10:00 em São Paulo (-03) é depois de 12:00 em UTC
        assert!(utc_key("D:20261009100000-03'00'") > utc_key("D:20261009120000Z"));
        assert!(utc_key("D:20261009100000") < utc_key("D:20261009100001"));
        assert_eq!(
            utc_key(""),
            utc_key("lixo"),
            "datas inválidas ficam juntas no começo"
        );
    }
}
