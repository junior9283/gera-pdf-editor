pub const MIN_ZOOM: f32 = 0.0833;
pub const MAX_ZOOM: f32 = 64.0;

/// Níveis de ampliação do Acrobat (lista do campo de zoom e passos de Ampliar/Reduzir).
pub const LEVELS: [f32; 20] = [
    MIN_ZOOM, 0.125, 0.25, 0.3333, 0.5, 0.6667, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0,
    12.0, 16.0, 24.0, 32.0, MAX_ZOOM,
];
pub const DEFAULT_ZOOM: f32 = 1.0;

/// `Some(v)` se `v` é finito e maior que zero.
pub(crate) fn finite_positive(v: f32) -> Option<f32> {
    (v.is_finite() && v > 0.0).then_some(v)
}

pub fn clamp_zoom(z: f32) -> f32 {
    if z.is_finite() {
        z.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        DEFAULT_ZOOM
    }
}

/// Próximo nível da lista acima de `z`.
pub fn zoom_in(z: f32) -> f32 {
    let z = clamp_zoom(z);
    LEVELS
        .into_iter()
        .find(|l| *l > z * 1.001)
        .unwrap_or(MAX_ZOOM)
}

/// Nível da lista logo abaixo de `z`.
pub fn zoom_out(z: f32) -> f32 {
    let z = clamp_zoom(z);
    LEVELS
        .into_iter()
        .rev()
        .find(|l| *l < z / 1.001)
        .unwrap_or(MIN_ZOOM)
}

/// Porcentagem digitada no campo de zoom ("150", "150%", "66,67"), como fator. Fora dos
/// limites, o limite; texto que não é número positivo, `None`.
pub fn parse_zoom(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim().replace(',', ".");
    let pct: f32 = t.parse().ok()?;
    (pct.is_finite() && pct > 0.0).then(|| clamp_zoom(pct / 100.0))
}

/// Zoom escrito como no campo do Acrobat em português: "100%", "8,33%", "12,5%".
pub fn percent_text(z: f32) -> String {
    let text = format!("{:.2}", z * 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}%", text.replace('.', ","))
}

/// Zoom em que a altura da página cabe em `avail_h`.
pub fn fit_height(page_h: f32, avail_h: f32) -> f32 {
    fit_width(page_h, avail_h)
}

pub fn fit_width(page_w: f32, avail_w: f32) -> f32 {
    match (finite_positive(page_w), finite_positive(avail_w)) {
        (Some(page), Some(avail)) => clamp_zoom(avail / page),
        _ => DEFAULT_ZOOM,
    }
}

/// Ajuste automático do zoom à área visível (refeito quando a janela muda de tamanho).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Zoom escolhido pelo usuário.
    None,
    #[default]
    Width,
    Page,
    Height,
    /// Largura do conteúdo (sem as margens brancas) da página atual.
    Visible,
}

/// Zoom em que `page` (largura, altura em pontos) cabe inteira em `avail`.
pub fn fit_page(page: (f32, f32), avail: (f32, f32)) -> f32 {
    match (
        finite_positive(page.0),
        finite_positive(page.1),
        finite_positive(avail.0),
        finite_positive(avail.1),
    ) {
        (Some(w), Some(h), Some(aw), Some(ah)) => clamp_zoom((aw / w).min(ah / h)),
        _ => DEFAULT_ZOOM,
    }
}

/// Chave de cache estável para um zoom (milésimos).
pub fn quantize(z: f32) -> u32 {
    (z * 1000.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_steps() {
        assert_eq!(clamp_zoom(100.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(0.0), MIN_ZOOM);
        assert_eq!(clamp_zoom(-3.0), MIN_ZOOM);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        assert_eq!(clamp_zoom(f32::INFINITY), 1.0);
        assert!((zoom_in(1.0) - 1.25).abs() < 1e-6);
        assert!((zoom_out(1.25) - 1.0).abs() < 1e-6);
        assert_eq!(zoom_in(MAX_ZOOM), MAX_ZOOM);
        assert_eq!(zoom_out(MIN_ZOOM), MIN_ZOOM);
    }

    #[test]
    fn zoom_steps_follow_acrobat_levels() {
        assert_eq!(zoom_in(1.0), 1.25);
        assert_eq!(zoom_in(1.1), 1.25);
        assert_eq!(zoom_in(8.0), 12.0);
        assert_eq!(zoom_in(32.0), 64.0);
        assert_eq!(zoom_out(1.0), 0.75);
        assert_eq!(zoom_out(0.7), 0.6667);
        assert_eq!(zoom_out(0.125), MIN_ZOOM);
        assert_eq!(zoom_out(f32::NAN), 0.75, "NaN vira 100 %");
        assert!(LEVELS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn parse_zoom_accepts_percent_and_comma() {
        assert_eq!(parse_zoom("150"), Some(1.5));
        assert_eq!(parse_zoom(" 150 % "), Some(1.5));
        assert!((parse_zoom("66,67").unwrap() - 0.6667).abs() < 1e-4);
        assert_eq!(parse_zoom("99999"), Some(MAX_ZOOM));
        assert_eq!(parse_zoom("1"), Some(MIN_ZOOM));
        for bad in ["", "abc", "0", "-5", "NaN", "inf"] {
            assert_eq!(parse_zoom(bad), None, "{bad}");
        }
    }

    #[test]
    fn percent_text_is_brazilian() {
        assert_eq!(percent_text(1.0), "100%");
        assert_eq!(percent_text(MIN_ZOOM), "8,33%");
        assert_eq!(percent_text(0.125), "12,5%");
        assert_eq!(percent_text(0.6667), "66,67%");
        assert_eq!(percent_text(64.0), "6400%");
        assert_eq!(parse_zoom(&percent_text(0.6667)), Some(0.6667));
    }

    #[test]
    fn fit_height_uses_the_page_height() {
        assert!((fit_height(800.0, 400.0) - 0.5).abs() < 1e-6);
        assert_eq!(fit_height(0.0, 400.0), DEFAULT_ZOOM);
    }

    #[test]
    fn fit_width_handles_bad_input() {
        assert!((fit_width(500.0, 1000.0) - 2.0).abs() < 1e-6);
        assert_eq!(fit_width(0.0, 1000.0), 1.0);
        assert_eq!(fit_width(500.0, -5.0), 1.0);
        assert_eq!(fit_width(f32::NAN, 100.0), 1.0);
    }

    #[test]
    fn quantize_is_stable() {
        assert_eq!(quantize(1.5), 1500);
        assert_eq!(quantize(1.5004), 1500);
    }

    #[test]
    fn fit_page_uses_the_tighter_side() {
        // retrato 600x800 numa área 1200x800: a altura manda (1.0)
        assert!((fit_page((600.0, 800.0), (1200.0, 800.0)) - 1.0).abs() < 1e-6);
        // paisagem 800x600 numa área 400x900: a largura manda (0.5)
        assert!((fit_page((800.0, 600.0), (400.0, 900.0)) - 0.5).abs() < 1e-6);
        assert_eq!(fit_page((0.0, 800.0), (100.0, 100.0)), 1.0);
        assert_eq!(fit_page((600.0, 800.0), (f32::NAN, 100.0)), 1.0);
        assert_eq!(Fit::default(), Fit::Width);
    }
}
