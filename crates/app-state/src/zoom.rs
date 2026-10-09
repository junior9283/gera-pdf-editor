pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 8.0;
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

pub fn zoom_in(z: f32) -> f32 {
    clamp_zoom(z * 1.25)
}

pub fn zoom_out(z: f32) -> f32 {
    clamp_zoom(z / 1.25)
}

pub fn fit_width(page_w: f32, avail_w: f32) -> f32 {
    match (finite_positive(page_w), finite_positive(avail_w)) {
        (Some(page), Some(avail)) => clamp_zoom(avail / page),
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
}
