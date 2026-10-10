//! Janela "Propriedades da anotação": cor, opacidade e espessura, aplicadas na hora.
use pdf_core::{AnnotInfo, AnnotKind, AnnotOp};

/// As cores da paleta de comentários do Acrobat.
pub const PALETTE: [(&str, [f32; 3]); 8] = [
    ("Amarelo", [1.0, 0.9, 0.0]),
    ("Vermelho", [1.0, 0.0, 0.0]),
    ("Laranja", [1.0, 0.5, 0.0]),
    ("Verde", [0.2, 0.7, 0.2]),
    ("Azul", [0.1, 0.45, 0.95]),
    ("Roxo", [0.55, 0.25, 0.75]),
    ("Preto", [0.0, 0.0, 0.0]),
    ("Cinza", [0.5, 0.5, 0.5]),
];

/// Cor RGB (0–1) para o egui.
pub fn color32(c: [f32; 3]) -> egui::Color32 {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    egui::Color32::from_rgb(r, g, b)
}

/// Opacidade (0–1) em porcentagem inteira.
pub fn percent(opacity: f32) -> f32 {
    (opacity.clamp(0.0, 1.0) * 100.0).round()
}

/// Porcentagem de volta para opacidade (0–1).
pub fn from_percent(p: f32) -> f32 {
    (p / 100.0).clamp(0.0, 1.0)
}

/// Espessura do traço ou da borda, em pontos.
const WIDTHS: std::ops::RangeInclusive<f32> = 1.0..=12.0;
/// Tamanho do texto da caixa de texto, em pontos.
const FONT_SIZES: std::ops::RangeInclusive<f32> = 8.0..=72.0;

/// Quais controles a janela mostra para cada tipo de anotação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sections {
    /// Cor da anotação (contorno das formas e linhas).
    pub color: bool,
    /// Preenchimento (formas) ou fundo (caixa de texto).
    pub fill: bool,
    /// Espessura do traço ou da borda.
    pub width: bool,
    /// Cor e tamanho do texto (caixa de texto).
    pub font: bool,
}

pub fn sections(kind: AnnotKind) -> Sections {
    use AnnotKind::*;
    Sections {
        color: !matches!(kind, TextBox | Stamp),
        fill: kind.has_fill(),
        width: kind.has_border(),
        font: kind == TextBox,
    }
}

/// Janela aberta para uma anotação, com os valores dos sliders enquanto são arrastados.
pub struct PropsDraft {
    pub page: i32,
    pub id: i32,
    /// Valores da anotação vistos por último: quando mudam (desfazer, por exemplo), os sliders
    /// os acompanham.
    seen: (f32, Option<f32>, Option<f32>),
    opacity: f32,
    width: f32,
    pub(crate) font_size: f32,
}

impl PropsDraft {
    pub fn new(info: &AnnotInfo) -> Self {
        Self {
            page: info.page,
            id: info.id,
            seen: (f32::NAN, None, None),
            opacity: 100.0,
            width: 1.0,
            font_size: 12.0,
        }
    }

    fn sync(&mut self, info: &AnnotInfo) {
        let now = (info.opacity, info.width, info.font.map(|f| f.0));
        if self.seen.0 != now.0 || self.seen.1 != now.1 || self.seen.2 != now.2 {
            self.seen = now;
            self.opacity = percent(info.opacity);
            self.width = info
                .width
                .unwrap_or(1.0)
                .clamp(*WIDTHS.start(), *WIDTHS.end());
            self.font_size = info
                .font
                .map_or(12.0, |f| f.0)
                .clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
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

    /// Texto da caixa: tamanho do slider e a cor pedida (ou a atual).
    fn font(&self, info: &AnnotInfo, color: Option<[f32; 3]>) -> AnnotOp {
        AnnotOp::Font {
            page: info.page,
            id: info.id,
            size: self.font_size,
            color: color.or(info.font.map(|f| f.1)).unwrap_or([0.0; 3]),
        }
    }
}

fn fill(info: &AnnotInfo, color: Option<[f32; 3]>) -> AnnotOp {
    AnnotOp::Fill {
        page: info.page,
        id: info.id,
        color,
    }
}

/// Slider soltou (ou mudou pelo teclado)?
fn committed(r: &egui::Response) -> bool {
    r.drag_stopped() || (r.changed() && !r.dragged())
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02)
}

/// Linha de quadradinhos da paleta (com "Nenhum" se `none`). Devolve a escolha:
/// `Some(Some(cor))`, `Some(None)` para Nenhum, `None` sem clique.
fn swatches(ui: &mut egui::Ui, current: Option<[f32; 3]>, none: bool) -> Option<Option<[f32; 3]>> {
    let mut picked = None;
    ui.horizontal(|ui| {
        if none
            && ui
                .add(egui::Button::selectable(current.is_none(), "Nenhum"))
                .clicked()
        {
            picked = Some(None);
        }
        for (name, rgb) in PALETTE {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
            ui.painter().rect_filled(rect, 3.0, color32(rgb));
            if current.is_some_and(|c| same(c, rgb)) {
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    3.0,
                    egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                    egui::StrokeKind::Outside,
                );
            }
            if resp.on_hover_text(name).clicked() {
                picked = Some(Some(rgb));
            }
        }
    });
    picked
}

/// Desenha a janela; devolve a mudança pedida neste quadro. `open` vira falso ao fechar.
pub fn window(
    ui: &mut egui::Ui,
    info: &AnnotInfo,
    draft: &mut PropsDraft,
    open: &mut bool,
) -> Option<AnnotOp> {
    draft.sync(info);
    let sec = sections(info.kind);
    let mut op = None;
    egui::Window::new("Propriedades da anotação")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .show(ui, |ui| {
            if sec.color {
                let label = if sec.fill { "Cor da linha" } else { "Cor" };
                ui.label(label);
                if let Some(Some(rgb)) = swatches(ui, info.color, false) {
                    op = Some(draft.style(info, Some(rgb)));
                }
            }
            if sec.font {
                ui.label("Cor do texto");
                if let Some(Some(rgb)) = swatches(ui, info.font.map(|f| f.1), false) {
                    op = Some(draft.font(info, Some(rgb)));
                }
                let r = ui.add(
                    egui::Slider::new(&mut draft.font_size, FONT_SIZES)
                        .suffix(" pt")
                        .fixed_decimals(0)
                        .text("Tamanho do texto"),
                );
                if committed(&r) {
                    op = Some(draft.font(info, None));
                }
            }
            if sec.fill {
                ui.label(if sec.font { "Fundo" } else { "Preenchimento" });
                if let Some(choice) = swatches(ui, info.fill, true) {
                    op = Some(fill(info, choice));
                }
            }
            ui.add_space(6.0);
            let r = ui.add(
                egui::Slider::new(&mut draft.opacity, 0.0..=100.0)
                    .suffix(" %")
                    .fixed_decimals(0)
                    .text("Opacidade"),
            );
            if committed(&r) {
                op = Some(draft.style(info, None));
            }
            if sec.width && info.width.is_some() {
                let label = if sec.font { "Borda" } else { "Espessura" };
                let r = ui.add(
                    egui::Slider::new(&mut draft.width, WIDTHS)
                        .suffix(" pt")
                        .fixed_decimals(1)
                        .text(label),
                );
                if committed(&r) {
                    op = Some(draft.style(info, None));
                }
            }
        });
    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

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
            ..Default::default()
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

    #[test]
    fn sections_follow_the_annotation_kind() {
        use AnnotKind::*;
        let s = |color, fill, width, font| Sections {
            color,
            fill,
            width,
            font,
        };
        assert_eq!(sections(Rect), s(true, true, true, false));
        assert_eq!(sections(Ellipse), s(true, true, true, false));
        assert_eq!(sections(Line), s(true, false, true, false));
        assert_eq!(sections(TextBox), s(false, true, true, true));
        assert_eq!(sections(Stamp), s(false, false, false, false));
        assert_eq!(sections(Highlight), s(true, false, false, false));
        assert_eq!(sections(Ink), s(true, false, true, false));
    }

    #[test]
    fn text_box_changes_go_to_font_and_fill() {
        let info = AnnotInfo {
            id: 4,
            kind: AnnotKind::TextBox,
            page: 0,
            font: Some((12.0, [1.0, 0.0, 0.0])),
            opacity: 1.0,
            ..Default::default()
        };
        let mut d = PropsDraft::new(&info);
        d.sync(&info);
        d.font_size = 18.0;
        assert!(matches!(
            d.font(&info, None),
            AnnotOp::Font { id: 4, size, color, .. } if size == 18.0 && color == [1.0, 0.0, 0.0]
        ));
        assert!(matches!(
            fill(&info, None),
            AnnotOp::Fill {
                id: 4,
                color: None,
                ..
            }
        ));
    }
}
