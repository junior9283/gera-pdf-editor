//! Preparar formulário: nomes automáticos e tamanhos padrão dos campos novos (como no Acrobat).
use pdf_core::{NewFieldKind, PdfRect};

/// Prefixo do nome automático de cada tipo ("Texto1", "Caixa1"…).
pub fn prefix(kind: NewFieldKind) -> &'static str {
    kind.prefix()
}

/// Rótulo da ferramenta na linha Preparar formulário.
pub fn label(kind: NewFieldKind) -> &'static str {
    match kind {
        NewFieldKind::Text => "Campo de texto",
        NewFieldKind::Checkbox => "Caixa de seleção",
        NewFieldKind::Radio => "Botão de opção",
        NewFieldKind::Combo => "Lista suspensa",
        NewFieldKind::List => "Caixa de listagem",
        NewFieldKind::Signature => "Assinatura",
        NewFieldKind::Button => "Botão",
        NewFieldKind::Date => "Data",
        NewFieldKind::Image => "Imagem",
    }
}

/// Na ordem da barra do Acrobat (Botão, Data e Imagem antes da Assinatura).
pub const KINDS: [NewFieldKind; 9] = [
    NewFieldKind::Text,
    NewFieldKind::Checkbox,
    NewFieldKind::Radio,
    NewFieldKind::Combo,
    NewFieldKind::List,
    NewFieldKind::Button,
    NewFieldKind::Date,
    NewFieldKind::Image,
    NewFieldKind::Signature,
];

/// Primeiro nome livre do tipo: "Texto1", "Texto2"…
pub fn next_name<'a>(
    kind: NewFieldKind,
    taken: impl IntoIterator<Item = &'a str> + Clone,
) -> String {
    let p = prefix(kind);
    (1..)
        .map(|n| format!("{p}{n}"))
        .find(|name| !taken.clone().into_iter().any(|t| t == name))
        .expect("sequência infinita")
}

/// Tamanho padrão (pt) do campo colocado com um clique.
pub fn default_size(kind: NewFieldKind) -> (f32, f32) {
    match kind {
        NewFieldKind::Text | NewFieldKind::Combo => (150.0, 22.0),
        NewFieldKind::Date => (100.0, 22.0),
        NewFieldKind::Button => (90.0, 24.0),
        NewFieldKind::Checkbox | NewFieldKind::Radio => (14.0, 14.0),
        NewFieldKind::List => (150.0, 60.0),
        NewFieldKind::Signature => (180.0, 40.0),
        NewFieldKind::Image => (120.0, 90.0),
    }
}

/// Retângulo do campo novo: o arrasto (se tiver tamanho) ou o tamanho padrão a partir do clique.
pub fn placed_rect(kind: NewFieldKind, from: (f32, f32), to: (f32, f32)) -> PdfRect {
    let drag = PdfRect::from_corners(from, to);
    if drag.width() >= 6.0 && drag.height() >= 6.0 {
        drag
    } else {
        let (w, h) = default_size(kind);
        PdfRect::new(from.0, from.1, from.0 + w, from.1 + h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_names_skip_the_taken_ones() {
        let taken = ["Texto1", "Texto2", "Caixa1"];
        assert_eq!(next_name(NewFieldKind::Text, taken), "Texto3");
        assert_eq!(next_name(NewFieldKind::Checkbox, taken), "Caixa2");
        assert_eq!(next_name(NewFieldKind::Radio, taken), "Grupo1");
    }

    #[test]
    fn a_click_uses_the_default_size_and_a_drag_its_own() {
        let r = placed_rect(NewFieldKind::Text, (10.0, 20.0), (11.0, 21.0));
        assert_eq!(
            (r.x0, r.y0, r.width(), r.height()),
            (10.0, 20.0, 150.0, 22.0)
        );
        let r = placed_rect(NewFieldKind::Text, (100.0, 100.0), (40.0, 60.0));
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (40.0, 60.0, 100.0, 100.0));
        assert_eq!(default_size(NewFieldKind::Radio), (14.0, 14.0));
    }
}
