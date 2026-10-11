//! Caixa de página com rótulos (números lógicos, como no Acrobat): o campo mostra o rótulo
//! ("iii") e, ao lado, a posição ("(3 de 10)"); digitar um rótulo ou um número vai até a página.

/// Página (a partir de 0) para o que foi digitado: um rótulo (o primeiro, se repetido) ou, se
/// não for rótulo, o número da página.
pub fn page_for_input(input: &str, labels: &[String], total: usize) -> Option<usize> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(p) = labels.iter().position(|l| l == input) {
        return Some(p);
    }
    let n: usize = input.parse().ok()?;
    (1..=total).contains(&n).then(|| n - 1)
}

/// Texto do campo e o que vem ao lado: com rótulos e números lógicos, "iii" e "(3 de 10)";
/// senão, "3" e "/ 10".
pub fn page_box_text(
    labels: &[String],
    page: usize,
    total: usize,
    logical: bool,
) -> (String, String) {
    match labels.get(page).filter(|_| logical) {
        Some(label) => (label.clone(), format!("({} de {total})", page + 1)),
        None => ((page + 1).to_string(), format!("/ {total}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Vec<String> {
        ["i", "ii", "iii", "1", "2", "1"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_label_goes_to_its_page() {
        assert_eq!(page_for_input("iii", &labels(), 6), Some(2));
        assert_eq!(page_for_input(" ii ", &labels(), 6), Some(1));
    }

    #[test]
    fn a_repeated_label_goes_to_the_first() {
        assert_eq!(page_for_input("1", &labels(), 6), Some(3));
    }

    #[test]
    fn a_number_that_is_not_a_label_is_the_page_position() {
        assert_eq!(page_for_input("5", &labels(), 6), Some(4));
        assert_eq!(page_for_input("5", &[], 6), Some(4));
        assert_eq!(page_for_input("99", &labels(), 6), None);
        assert_eq!(page_for_input("0", &labels(), 6), None);
        assert_eq!(page_for_input("x", &labels(), 6), None);
        assert_eq!(page_for_input("", &labels(), 6), None);
    }

    #[test]
    fn box_text_with_and_without_labels() {
        assert_eq!(
            page_box_text(&labels(), 2, 6, true),
            ("iii".to_string(), "(3 de 6)".to_string())
        );
        assert_eq!(
            page_box_text(&labels(), 2, 6, false),
            ("3".to_string(), "/ 6".to_string())
        );
        assert_eq!(
            page_box_text(&[], 2, 6, true),
            ("3".to_string(), "/ 6".to_string())
        );
    }
}
