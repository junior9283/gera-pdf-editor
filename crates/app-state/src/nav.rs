/// Teclas de navegação do documento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    PageUp,
    PageDown,
    Home,
    End,
}

/// Nova posição de rolagem (em pontos) para uma tecla de navegação, sempre dentro do documento.
/// Page Up/Down andam quase uma janela inteira (deixam 10% de contexto da tela anterior).
pub fn nav_target(key: NavKey, scroll_y: f32, viewport_h: f32, total_h: f32) -> f32 {
    let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let (scroll_y, viewport_h, total_h) = (clean(scroll_y), clean(viewport_h), clean(total_h));
    let max_scroll = (total_h - viewport_h).max(0.0);
    let target = match key {
        NavKey::PageUp => scroll_y - viewport_h * 0.9,
        NavKey::PageDown => scroll_y + viewport_h * 0.9,
        NavKey::Home => 0.0,
        NavKey::End => max_scroll,
    };
    target.clamp(0.0, max_scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_keys_move_by_almost_one_viewport() {
        // conteúdo de 5000 pt, janela de 1000 pt, rolagem em 2000
        assert_eq!(nav_target(NavKey::PageDown, 2000.0, 1000.0, 5000.0), 2900.0);
        assert_eq!(nav_target(NavKey::PageUp, 2000.0, 1000.0, 5000.0), 1100.0);
    }

    #[test]
    fn home_and_end_go_to_the_extremes() {
        assert_eq!(nav_target(NavKey::Home, 2000.0, 1000.0, 5000.0), 0.0);
        assert_eq!(nav_target(NavKey::End, 2000.0, 1000.0, 5000.0), 4000.0);
    }

    #[test]
    fn the_target_is_clamped_inside_the_document() {
        assert_eq!(nav_target(NavKey::PageUp, 100.0, 1000.0, 5000.0), 0.0);
        assert_eq!(nav_target(NavKey::PageDown, 3900.0, 1000.0, 5000.0), 4000.0);
        // conteúdo menor que a janela: nunca rola
        assert_eq!(nav_target(NavKey::End, 0.0, 1000.0, 300.0), 0.0);
        assert_eq!(nav_target(NavKey::PageDown, 0.0, 1000.0, 300.0), 0.0);
    }

    #[test]
    fn invalid_numbers_do_not_produce_invalid_targets() {
        for key in [NavKey::PageUp, NavKey::PageDown, NavKey::Home, NavKey::End] {
            let t = nav_target(key, f32::NAN, f32::NAN, f32::NAN);
            assert!(t.is_finite() && t >= 0.0, "{key:?} -> {t}");
        }
    }
}
