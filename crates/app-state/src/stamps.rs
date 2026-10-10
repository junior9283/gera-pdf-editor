//! Catálogo de carimbos, como no Acrobat: dinâmicos (com nome, data e hora) e padrão comercial.

/// Largura e altura padrão de um carimbo na página (as da imagem desenhada), em pontos.
pub use pdf_core::{STAMP_HEIGHT, STAMP_WIDTH};

const GREEN: [f32; 3] = [0.13, 0.55, 0.13];
const RED: [f32; 3] = [0.8, 0.1, 0.1];
const BLUE: [f32; 3] = [0.1, 0.3, 0.75];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StampKind {
    #[default]
    DynApproved,
    DynReviewed,
    DynReceived,
    Approved,
    NotApproved,
    Confidential,
    Draft,
    Final,
    Expired,
    ForComment,
    TopSecret,
    AsIs,
    Sold,
    Departmental,
    Experimental,
    ForPublicRelease,
    NotForPublicRelease,
}

impl StampKind {
    pub const DYNAMIC: [StampKind; 3] = [Self::DynApproved, Self::DynReviewed, Self::DynReceived];
    pub const STANDARD: [StampKind; 14] = [
        Self::Approved,
        Self::NotApproved,
        Self::Confidential,
        Self::Draft,
        Self::Final,
        Self::Expired,
        Self::ForComment,
        Self::TopSecret,
        Self::AsIs,
        Self::Sold,
        Self::Departmental,
        Self::Experimental,
        Self::ForPublicRelease,
        Self::NotForPublicRelease,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::DynApproved | Self::Approved => "APROVADO",
            Self::DynReviewed => "REVISADO",
            Self::DynReceived => "RECEBIDO",
            Self::NotApproved => "NÃO APROVADO",
            Self::Confidential => "CONFIDENCIAL",
            Self::Draft => "RASCUNHO",
            Self::Final => "FINAL",
            Self::Expired => "VENCIDO",
            Self::ForComment => "PARA COMENTÁRIO",
            Self::TopSecret => "ULTRASSECRETO",
            Self::AsIs => "CONFORME",
            Self::Sold => "VENDIDO",
            Self::Departmental => "DEPARTAMENTAL",
            Self::Experimental => "EXPERIMENTAL",
            Self::ForPublicRelease => "DIVULGAÇÃO PÚBLICA",
            Self::NotForPublicRelease => "NÃO DIVULGAR",
        }
    }

    pub fn is_dynamic(self) -> bool {
        Self::DYNAMIC.contains(&self)
    }

    pub fn color(self) -> [f32; 3] {
        match self {
            Self::DynApproved
            | Self::DynReviewed
            | Self::DynReceived
            | Self::Approved
            | Self::AsIs
            | Self::Final => GREEN,
            Self::NotApproved
            | Self::Confidential
            | Self::Expired
            | Self::TopSecret
            | Self::NotForPublicRelease => RED,
            _ => BLUE,
        }
    }
}

/// Linha de baixo do carimbo dinâmico: "Maria Silva, 09/10/2026 14:05".
/// `now` = (ano, mês, dia, hora, minuto), na hora local.
pub fn subtitle(name: &str, (y, m, d, h, min): (i32, u32, u32, u32, u32)) -> String {
    format!("{}, {d:02}/{m:02}/{y} {h:02}:{min:02}", name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_titles_are_unique_within_each_group_and_colored() {
        for group in [&StampKind::DYNAMIC[..], &StampKind::STANDARD[..]] {
            for (n, a) in group.iter().enumerate() {
                assert!(
                    group[n + 1..].iter().all(|b| b.title() != a.title()),
                    "{a:?}"
                );
            }
        }
        assert_eq!(StampKind::DYNAMIC.len() + StampKind::STANDARD.len(), 17);
        assert_eq!(StampKind::NotApproved.color(), RED);
        assert_eq!(StampKind::Approved.color(), GREEN);
        assert_eq!(StampKind::Draft.color(), BLUE);
        assert!(StampKind::DYNAMIC.iter().all(|s| s.is_dynamic()));
        assert!(!StampKind::STANDARD.iter().any(|s| s.is_dynamic()));
    }

    #[test]
    fn dynamic_subtitle_has_name_date_and_time() {
        assert_eq!(
            subtitle(" Maria Silva ", (2026, 10, 9, 14, 5)),
            "Maria Silva, 09/10/2026 14:05"
        );
    }
}
