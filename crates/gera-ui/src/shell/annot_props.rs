//! Janela "Propriedades da anotação": cor, opacidade e espessura, aplicadas na hora.
use pdf_core::{AnnotInfo, AnnotOp};

/// As cores da paleta de comentários do Acrobat.
pub const PALETTE: [(&str, [f32; 3]); 8] = [
    ("Amarelo", [1.0, 0.9, 0.0]),
    ("Vermelho", [0.9, 0.1, 0.1]),
    ("Laranja", [1.0, 0.5, 0.0]),
    ("Verde", [0.2, 0.7, 0.2]),
    ("Azul", [0.1, 0.45, 0.95]),
    ("Roxo", [0.55, 0.25, 0.75]),
    ("Preto", [0.0, 0.0, 0.0]),
    ("Cinza", [0.5, 0.5, 0.5]),
];

/// Opacidade (0–1) em porcentagem inteira.
pub fn percent(opacity: f32) -> f32 {
    (opacity.clamp(0.0, 1.0) * 100.0).round()
}

/// Porcentagem de volta para opacidade (0–1).
pub fn from_percent(p: f32) -> f32 {
    (p / 100.0).clamp(0.0, 1.0)
}

/// Espessura do traço (desenho), em pontos.
const WIDTHS: std::ops::RangeInclusive<f32> = 1.0..=12.0;

/// Janela aberta para uma anotação, com os valores dos sliders enquanto são arrastados.
pub struct PropsDraft {
    pub page: i32,
    pub id: i32,
    /// Valores da anotação vistos por último: quando mudam (desfazer, por exemplo), os sliders
    /// os acompanham.
    seen: (f32, Option<f32>),
    opacity: f32,
    width: f32,
}

impl PropsDraft {
    pub fn new(info: &AnnotInfo) -> Self {
        Self {
            page: info.page,
            id: info.id,
            seen: (f32::NAN, None),
            opacity: 100.0,
            width: 1.0,
        }
    }

    fn sync(&mut self, info: &AnnotInfo) {
        let now = (info.opacity, info.width);
        if self.seen.0 != now.0 || self.seen.1 != now.1 {
            self.seen = now;
            self.opacity = percent(info.opacity);
            self.width = info
                .width
                .unwrap_or(1.0)
                .clamp(*WIDTHS.start(), *WIDTHS.end());
        }
    }

    fn style(&self, info: &AnnotInfo, color: Option<[f32; 3]>) -> AnnotOp {
        AnnotOp::Style {
            page: info.page,
            id: info.id,
            color,
            opacity: from_percent(self.opacity),
            width: info.width.map(|_| self.width),
        }
    }
}

/// Slider soltou (ou mudou pelo teclado)?
fn committed(r: &egui::Response) -> bool {
    r.drag_stopped() || (r.changed() && !r.dragged())
}

/// Desenha a janela; devolve a mudança de estilo pedida neste quadro. `open` vira falso ao fechar.
pub fn window(
    ui: &mut egui::Ui,
    info: &AnnotInfo,
    draft: &mut PropsDraft,
    open: &mut bool,
) -> Option<AnnotOp> {
    draft.sync(info);
    let mut op = None;
    egui::Window::new("Propriedades da anotação")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .show(ui, |ui| {
            ui.label("Cor");
            ui.horizontal(|ui| {
                for (name, rgb) in PALETTE {
                    let [r, g, b] = rgb.map(|c| (c * 255.0).round() as u8);
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    let chosen = info
                        .color
                        .is_some_and(|c| c.iter().zip(rgb).all(|(a, b)| (a - b).abs() < 0.02));
                    ui.painter()
                        .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                    if chosen {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            3.0,
                            egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if resp.on_hover_text(name).clicked() {
                        op = Some(draft.style(info, Some(rgb)));
                    }
                }
            });
            ui.add_space(6.0);
            let r = ui.add(
                egui::Slider::new(&mut draft.opacity, 0.0..=100.0)
                    .suffix(" %")
                    .fixed_decimals(0)
                    .text("Opacidade"),
            );
            if committed(&r) {
                op = Some(draft.style(info, info.color));
            }
            if info.width.is_some() {
                let r = ui.add(
                    egui::Slider::new(&mut draft.width, WIDTHS)
                        .suffix(" pt")
                        .fixed_decimals(1)
                        .text("Espessura"),
                );
                if committed(&r) {
                    op = Some(draft.style(info, info.color));
                }
            }
        });
    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::{AnnotKind, PdfRect};

    #[test]
    fn palette_has_eight_distinct_colors() {
        for (n, (_, a)) in PALETTE.iter().enumerate() {
            assert!(PALETTE[n + 1..].iter().all(|(_, b)| b != a));
        }
    }

    #[test]
    fn opacity_round_trips_through_percent() {
        assert_eq!(percent(0.5), 50.0);
        assert_eq!(percent(1.7), 100.0);
        assert_eq!(from_percent(percent(0.25)), 0.25);
        assert_eq!(from_percent(-5.0), 0.0);
    }

    #[test]
    fn style_carries_width_only_when_the_annotation_has_one() {
        let mut info = AnnotInfo {
            id: 7,
            kind: AnnotKind::Highlight,
            page: 1,
            rect: PdfRect::new(0.0, 0.0, 1.0, 1.0),
            contents: String::new(),
            color: Some([1.0, 0.9, 0.0]),
            opacity: 0.4,
            width: None,
        };
        let mut d = PropsDraft::new(&info);
        d.sync(&info);
        assert!(matches!(
            d.style(&info, None),
            AnnotOp::Style { id: 7, page: 1, width: None, opacity, .. } if opacity == 0.4
        ));
        info.width = Some(3.0);
        d.sync(&info);
        assert!(matches!(
            d.style(&info, None),
            AnnotOp::Style { width: Some(w), .. } if w == 3.0
        ));
    }
}
