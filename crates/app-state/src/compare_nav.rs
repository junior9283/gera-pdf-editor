//! Próxima e anterior alteração no resultado da comparação (Ctrl+. e Ctrl+, como no Acrobat).

/// Posição de uma alteração: página e canto superior esquerdo (y, x) da marca.
pub type Spot = (i32, f32, f32);

/// Ordem de leitura: página, depois de cima para baixo, depois da esquerda para a direita.
pub fn order(spots: &[Spot]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..spots.len()).collect();
    idx.sort_by(|&a, &b| {
        let (pa, ya, xa) = spots[a];
        let (pb, yb, xb) = spots[b];
        pa.cmp(&pb).then(ya.total_cmp(&yb)).then(xa.total_cmp(&xb))
    });
    idx
}

/// A posição (em `order`) depois de `current` (`forward`) ou antes. Sem posição atual, começa na
/// primeira (ou na última, para trás); nas pontas fica onde está, sem dar a volta.
pub fn step(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(i), true) => (i + 1).min(count - 1),
        (Some(i), false) => i.saturating_sub(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_order() {
        let spots = [
            (1, 50.0, 10.0),
            (0, 300.0, 10.0),
            (1, 50.0, 5.0),
            (0, 20.0, 400.0),
        ];
        assert_eq!(order(&spots), vec![3, 1, 2, 0]);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(step(0, None, true), None);
        assert_eq!(step(3, None, true), Some(0));
        assert_eq!(step(3, None, false), Some(2));
        assert_eq!(step(3, Some(1), true), Some(2));
        assert_eq!(step(3, Some(2), true), Some(2));
        assert_eq!(step(3, Some(0), false), Some(0));
        assert_eq!(step(3, Some(2), false), Some(1));
    }
}
