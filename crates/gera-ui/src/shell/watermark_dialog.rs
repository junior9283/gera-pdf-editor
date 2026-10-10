//! Janela "Adicionar marca d'água" (como no Acrobat): origem (texto ou arquivo), aparência
//! (rotação, opacidade, escala, atrás/na frente), posição e páginas, com prévia. Também as peças
//! comuns às janelas de marca d'água e de cabeçalho e rodapé.
use crate::shell::dialog::ok_cancel_window;
use pdf_core::{Align, PageSet, StdFont, TextLook, Watermark, WatermarkSource};
use std::path::PathBuf;

/// Rotações do Acrobat; `None` é "outra".
const ROTATIONS: [(&str, Option<f32>); 4] = [
    ("45°", Some(45.0)),
    ("0°", Some(0.0)),
    ("−45°", Some(-45.0)),
    ("Outra", None),
];
const FONTS: [(&str, StdFont); 3] = [
    ("Helvetica", StdFont::Helvetica),
    ("Times", StdFont::Times),
    ("Courier", StdFont::Courier),
];
const VERTICAL: [(&str, Align); 3] = [
    ("Topo", Align::Start),
    ("Centro", Align::Center),
    ("Base", Align::End),
];
const HORIZONTAL: [(&str, Align); 3] = [
    ("Esquerda", Align::Start),
    ("Centro", Align::Center),
    ("Direita", Align::End),
];
const PAGES: [&str; 4] = ["Todas", "Intervalo", "Só pares", "Só ímpares"];
/// Arquivos aceitos como origem.
pub const FILE_EXTS: &[&str] = &["pdf", "png", "jpg", "jpeg"];

pub struct WatermarkDialog {
    from_file: bool,
    pub text: String,
    look: TextLook,
    pub path: Option<PathBuf>,
    /// Página do PDF de origem (a partir de 1).
    pdf_page: u32,
    /// Índice em `ROTATIONS`.
    rotation: usize,
    custom_rotation: f32,
    /// 0 a 100.
    opacity: f32,
    relative: bool,
    /// % da página.
    scale: f32,
    behind: bool,
    vertical: (Align, f32),
    horizontal: (Align, f32),
    pub pages: Pages,
    /// A janela pediu para escolher o arquivo de origem.
    pub wants_file: bool,
}

impl Default for WatermarkDialog {
    /// Como o Acrobat abre: texto, 45°, 50% de opacidade, centrada, na frente, todas as páginas.
    fn default() -> Self {
        Self {
            from_file: false,
            text: String::new(),
            look: TextLook {
                font: StdFont::Helvetica,
                bold: true,
                size: 48.0,
                color: [200, 0, 0],
            },
            path: None,
            pdf_page: 1,
            rotation: 0,
            custom_rotation: 30.0,
            opacity: 50.0,
            relative: false,
            scale: 50.0,
            behind: false,
            vertical: (Align::Center, 0.0),
            horizontal: (Align::Center, 0.0),
            pages: Pages::default(),
            wants_file: false,
        }
    }
}

/// Escolha de páginas das janelas: todas, intervalo, pares, ímpares.
#[derive(Default)]
pub struct Pages {
    choice: usize,
    range: String,
}

impl Pages {
    pub fn from_set(p: &PageSet) -> Self {
        let (choice, range) = match p {
            PageSet::All => (0, String::new()),
            PageSet::Range(r) => (1, r.clone()),
            PageSet::Even => (2, String::new()),
            PageSet::Odd => (3, String::new()),
        };
        Self { choice, range }
    }

    /// As páginas escolhidas, ou o problema do intervalo (o mesmo texto do pdf-core).
    pub fn set(&self, total: usize) -> Result<PageSet, String> {
        Ok(match self.choice {
            1 => {
                pdf_core::parse_ranges(&self.range, total)?;
                PageSet::Range(self.range.clone())
            }
            2 => PageSet::Even,
            3 => PageSet::Odd,
            _ => PageSet::All,
        })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, id: &str) {
        ui.horizontal(|ui| {
            ui.label("Páginas");
            egui::ComboBox::from_id_salt(id)
                .selected_text(PAGES[self.choice])
                .show_index(ui, &mut self.choice, PAGES.len(), |i| PAGES[i]);
            if self.choice == 1 {
                ui.add(
                    egui::TextEdit::singleline(&mut self.range)
                        .desired_width(90.0)
                        .hint_text("1-3, 5"),
                );
            }
        });
    }
}

/// Fonte, negrito, tamanho e cor do texto (as duas janelas).
pub fn look_editor(ui: &mut egui::Ui, id: &str, look: &mut TextLook, max_size: f32) {
    ui.horizontal(|ui| {
        let name = FONTS.iter().find(|f| f.1 == look.font).map_or("", |f| f.0);
        egui::ComboBox::from_id_salt(id)
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (label, font) in FONTS {
                    ui.selectable_value(&mut look.font, font, label);
                }
            });
        ui.checkbox(&mut look.bold, "Negrito");
        ui.add(
            egui::DragValue::new(&mut look.size)
                .range(4.0..=max_size)
                .suffix(" pt"),
        );
        ui.color_edit_button_srgb(&mut look.color);
    });
}

/// A página em miniatura para a prévia (altura `h`): o pintor, o retângulo e a escala pt → px.
pub fn preview_page(
    ui: &mut egui::Ui,
    page: (f32, f32),
    h: f32,
) -> (egui::Painter, egui::Rect, f32) {
    let w = h * page.0 / page.1.max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::GRAY),
        egui::StrokeKind::Inside,
    );
    (painter, rect, w / page.0.max(1.0))
}

impl WatermarkDialog {
    /// A janela do Atualizar: as escolhas da marca existente.
    pub fn from_watermark(w: &Watermark) -> Self {
        let mut d = Self::default();
        match &w.source {
            WatermarkSource::Text { text, look } => {
                d.text = text.clone();
                d.look = look.clone();
            }
            // o caminho não vem do documento (o pdf-core o apaga): escolher o arquivo de novo
            WatermarkSource::Image(p) => {
                d.from_file = true;
                d.path = (!p.as_os_str().is_empty()).then(|| p.clone());
            }
            WatermarkSource::PdfPage { path, page } => {
                d.from_file = true;
                d.path = (!path.as_os_str().is_empty()).then(|| path.clone());
                d.pdf_page = (*page + 1).max(1) as u32;
            }
        }
        d.rotation = ROTATIONS
            .iter()
            .position(|r| r.1 == Some(w.rotation))
            .unwrap_or(ROTATIONS.len() - 1);
        d.custom_rotation = w.rotation;
        d.opacity = w.opacity * 100.0;
        d.relative = w.scale.is_some();
        d.scale = w.scale.map_or(50.0, |s| s * 100.0);
        d.behind = w.behind;
        d.vertical = w.vertical;
        d.horizontal = w.horizontal;
        d.pages = Pages::from_set(&w.pages);
        d
    }

    fn rotation(&self) -> f32 {
        ROTATIONS[self.rotation].1.unwrap_or(self.custom_rotation)
    }

    fn source_is_pdf(&self) -> bool {
        self.path
            .as_ref()
            .and_then(|p| p.extension())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    }

    /// A marca das escolhas, ou o que falta (para o OK ficar desligado).
    pub fn watermark(&self, total: usize) -> Result<Watermark, String> {
        let source = if self.from_file {
            let path = self
                .path
                .clone()
                .ok_or("Escolha o arquivo da marca d'água.")?;
            if self.source_is_pdf() {
                WatermarkSource::PdfPage {
                    path,
                    page: self.pdf_page.max(1) as i32 - 1,
                }
            } else {
                WatermarkSource::Image(path)
            }
        } else {
            if self.text.trim().is_empty() {
                return Err("Escreva o texto da marca d'água.".into());
            }
            WatermarkSource::Text {
                text: self.text.clone(),
                look: self.look.clone(),
            }
        };
        Ok(Watermark {
            source,
            rotation: self.rotation(),
            opacity: (self.opacity / 100.0).clamp(0.0, 1.0),
            scale: self.relative.then_some(self.scale / 100.0),
            behind: self.behind,
            vertical: self.vertical,
            horizontal: self.horizontal,
            pages: self.pages.set(total)?,
        })
    }

    /// Desenha a janela. `page` é o tamanho da página atual (para a prévia) e `total`, o número de
    /// páginas.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        page: (f32, f32),
        total: usize,
    ) -> Option<Option<Watermark>> {
        ok_cancel_window(ctx, title, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| self.choices(ui));
                // (um separador vertical aqui esticaria a janela até a borda da tela)
                ui.add_space(12.0);
                self.preview(ui, page);
            });
            self.watermark(total)
        })
    }

    fn choices(&mut self, ui: &mut egui::Ui) {
        ui.strong("Origem");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.from_file, false, "Texto");
            ui.radio_value(&mut self.from_file, true, "Arquivo");
        });
        if self.from_file {
            ui.horizontal(|ui| {
                if ui.button("Escolher…").clicked() {
                    self.wants_file = true;
                }
                ui.label(
                    self.path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or("(nenhum arquivo)".into(), |n| n.to_string_lossy()),
                );
            });
            if self.source_is_pdf() {
                ui.horizontal(|ui| {
                    ui.label("Página");
                    ui.add(egui::DragValue::new(&mut self.pdf_page).range(1..=9999));
                });
            }
        } else {
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_rows(2)
                    .hint_text("Ex.: CONFIDENCIAL"),
            );
            look_editor(ui, "marca-fonte", &mut self.look, 500.0);
        }
        ui.add_space(6.0);
        ui.strong("Aparência");
        ui.horizontal(|ui| {
            ui.label("Rotação");
            for (i, (label, _)) in ROTATIONS.iter().enumerate() {
                ui.radio_value(&mut self.rotation, i, *label);
            }
            if ROTATIONS[self.rotation].1.is_none() {
                ui.add(
                    egui::DragValue::new(&mut self.custom_rotation)
                        .range(-360.0..=360.0)
                        .suffix("°"),
                );
            }
        });
        ui.add(egui::Slider::new(&mut self.opacity, 0.0..=100.0).text("Opacidade (%)"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.relative, "Escala relativa à página");
            ui.add_enabled(
                self.relative,
                egui::DragValue::new(&mut self.scale)
                    .range(1.0..=100.0)
                    .suffix(" %"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Local");
            ui.radio_value(&mut self.behind, true, "Atrás da página");
            ui.radio_value(&mut self.behind, false, "Na frente da página");
        });
        ui.add_space(6.0);
        ui.strong("Posição");
        for (label, names, (align, off), id) in [
            ("Vertical", VERTICAL, &mut self.vertical, "marca-v"),
            ("Horizontal", HORIZONTAL, &mut self.horizontal, "marca-h"),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                let current = names.iter().find(|n| n.1 == *align).map_or("", |n| n.0);
                egui::ComboBox::from_id_salt(id)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (name, a) in names {
                            ui.selectable_value(align, a, name);
                        }
                    });
                ui.add(egui::DragValue::new(off).suffix(" pt"));
            });
        }
        ui.add_space(6.0);
        self.pages.ui(ui, "marca-paginas");
    }

    /// Prévia esquemática: a página e a marca (texto girado ou o quadro do arquivo).
    fn preview(&self, ui: &mut egui::Ui, page: (f32, f32)) {
        let (painter, rect, k) = preview_page(ui, page, 260.0);
        let alpha = self.opacity / 100.0;
        let at = |(align, off): (Align, f32), lo: f32, hi: f32, size: f32| {
            align.center(off * k, lo, hi, size)
        };
        if self.from_file {
            let (bw, bh) = if self.relative {
                let f = self.scale / 100.0;
                (rect.width() * f, rect.height() * f)
            } else {
                (rect.width() * 0.4, rect.height() * 0.3)
            };
            let c = egui::pos2(
                at(self.horizontal, rect.left(), rect.right(), bw),
                at(self.vertical, rect.top(), rect.bottom(), bh),
            );
            let r = egui::Rect::from_center_size(c, egui::vec2(bw, bh));
            painter.rect_filled(r, 0.0, egui::Color32::from_gray(160).gamma_multiply(alpha));
            return;
        }
        let [cr, cg, cb] = self.look.color;
        let galley = painter.layout(
            self.text.clone(),
            egui::FontId::proportional((self.look.size * k).max(4.0)),
            egui::Color32::from_rgb(cr, cg, cb).gamma_multiply(alpha),
            f32::INFINITY,
        );
        let size = galley.size();
        let angle = -self.rotation().to_radians();
        let c = egui::pos2(
            at(self.horizontal, rect.left(), rect.right(), size.x),
            at(self.vertical, rect.top(), rect.bottom(), size.y),
        );
        // o texto gira em torno do centro dele
        let (s, co) = angle.sin_cos();
        let half = size / 2.0;
        let corner = c - egui::vec2(half.x * co - half.y * s, half.x * s + half.y * co);
        painter.add(
            egui::epaint::TextShape::new(corner, galley, egui::Color32::BLACK).with_angle(angle),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_acrobat_like_and_need_text() {
        let mut d = WatermarkDialog::default();
        assert!(d.watermark(3).is_err(), "sem texto");
        d.text = "RASCUNHO".into();
        let w = d.watermark(3).unwrap();
        assert_eq!((w.rotation, w.opacity, w.behind), (45.0, 0.5, false));
        assert_eq!(w.pages, PageSet::All);
    }

    #[test]
    fn a_file_source_is_an_image_or_a_pdf_page() {
        let mut d = WatermarkDialog {
            from_file: true,
            ..Default::default()
        };
        assert!(d.watermark(3).is_err(), "sem arquivo");
        d.path = Some("logo.PNG".into());
        assert!(matches!(
            d.watermark(3).unwrap().source,
            WatermarkSource::Image(_)
        ));
        d.path = Some("timbre.pdf".into());
        d.pdf_page = 2;
        assert!(matches!(
            d.watermark(3).unwrap().source,
            WatermarkSource::PdfPage { page: 1, .. }
        ));
    }

    #[test]
    fn a_bad_page_range_blocks_ok() {
        let mut d = WatermarkDialog {
            text: "X".into(),
            pages: Pages {
                choice: 1,
                range: "2-9".into(),
            },
            ..Default::default()
        };
        assert!(d.watermark(3).unwrap_err().contains("3 páginas"));
        d.pages.range = "2-3".into();
        assert_eq!(d.watermark(3).unwrap().pages, PageSet::Range("2-3".into()));
    }

    #[test]
    fn updating_starts_from_the_current_mark() {
        let d = WatermarkDialog {
            text: "FINAL".into(),
            rotation: 3,
            custom_rotation: 30.0,
            relative: true,
            scale: 40.0,
            behind: true,
            vertical: (Align::Start, 20.0),
            pages: Pages {
                choice: 3,
                range: String::new(),
            },
            ..Default::default()
        };
        let w = d.watermark(5).unwrap();
        let back = WatermarkDialog::from_watermark(&w).watermark(5).unwrap();
        assert_eq!(back, w);
    }
}
