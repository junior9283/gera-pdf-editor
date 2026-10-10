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
}

impl Thread {
    /// Status para filtrar: sem status definido conta como Nenhum.
    pub fn status(&self) -> ReviewState {
        self.review.as_ref().map_or(ReviewState::None, |r| r.0)
    }
}

/// Ordem cronológica de anotações: data de modificação e, no empate, o id (a mais nova vence).
fn chrono_key(a: &AnnotInfo) -> (&str, i32) {
    (a.modified.as_str(), a.id)
}

/// Monta as conversas: só os comentários (sem `reply_to`) viram linhas; respostas e status ficam
/// dentro do pai; os de pai inexistente (PDF de outro programa) são ignorados.
pub fn threads(annots: &[AnnotInfo]) -> Vec<Thread> {
    let mut out: Vec<Thread> = annots
        .iter()
        .filter(|a| !a.is_reply())
        .map(|a| Thread {
            root: a.clone(),
            replies: Vec::new(),
            review: None,
            checked: false,
        })
        .collect();
    let index: HashMap<i32, usize> = out
        .iter()
        .enumerate()
        .map(|(n, t)| (t.root.id, n))
        .collect();
    let mut children: Vec<&AnnotInfo> = annots.iter().filter(|a| a.is_reply()).collect();
    children.sort_by(|x, y| chrono_key(x).cmp(&chrono_key(y)));
    for child in children {
        let Some(&n) = child.reply_to.and_then(|p| index.get(&p)) else {
            continue;
        };
        let t = &mut out[n];
        match &child.state {
            None => t.replies.push(child.clone()),
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
            && (self.authors.is_empty() || self.authors.contains(&t.root.author))
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
        SortBy::Date => threads.sort_by(|x, y| chrono_key(&x.root).cmp(&chrono_key(&y.root))),
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
}
