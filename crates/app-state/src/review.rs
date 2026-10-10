//! Status de revisão de um comentário, como no Acrobat (Definir status ▸ …).

/// Estados do visto (/StateModel /Marked), como o Acrobat grava.
pub const MARKED: &str = "Marked";
pub const UNMARKED: &str = "Unmarked";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewState {
    None,
    Accepted,
    Cancelled,
    Completed,
    Rejected,
}

impl ReviewState {
    pub const ALL: [ReviewState; 5] = [
        Self::None,
        Self::Accepted,
        Self::Cancelled,
        Self::Completed,
        Self::Rejected,
    ];

    /// Nome gravado no PDF (/State).
    pub fn pdf_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Accepted => "Accepted",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
            Self::Rejected => "Rejected",
        }
    }

    pub fn from_pdf(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.pdf_name() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Nenhum",
            Self::Accepted => "Aceito",
            Self::Cancelled => "Cancelado",
            Self::Completed => "Concluído",
            Self::Rejected => "Rejeitado",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_names_round_trip_and_labels_are_portuguese() {
        for s in ReviewState::ALL {
            assert_eq!(ReviewState::from_pdf(s.pdf_name()), Some(s));
        }
        assert_eq!(ReviewState::from_pdf("Marked"), None);
        assert_eq!(ReviewState::Completed.label(), "Concluído");
    }
}
