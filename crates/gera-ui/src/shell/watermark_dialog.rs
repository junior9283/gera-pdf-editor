//! Janela "Adicionar marca d'água" (como no Acrobat): origem (texto ou arquivo), aparência
//! (rotação, opacidade, escala, atrás/na frente), posição e páginas, com prévia.
use pdf_core::{Align, PageSet, StdFont, TextLook, Watermark, WatermarkSource};
use std::path::PathBuf;

/// Rotações do Acrobat (a última é "outra").
const ROTATIONS: [(&str, f32); 4] = [("45°", 45.0), ("0°", 0.0), ("−45°", -45.0), ("Outra", 0.0)];
pub const FONTS: [(&str, StdFont); 3] = [
    ("Helvetica", StdFont::Helvetica),
    ("Times", StdFont::Times),
    ("Courier", StdFont::Courier),
];
const VERTICAL: [&str; 3] = ["Topo", "Centro", "Base"];
const HORIZONTAL: [&str; 3] = ["Esquerda", "Centro", "Direita"];
pub const PAGES: [&str; 4] = ["Todas", "Intervalo", "Só pares", "Só ímpares"];
/// Arquivos aceitos como origem.
pub const FILE_EXTS: &[&str] = &["pdf", "png", "jpg", "jpeg"];

pub struct WatermarkDialog {
    from_file: bool,
    pub text: String,
    font: usize,
    bold: bool,
    size: f32,
    color: [u8; 3],
    pub path: Option<PathBuf>,
    /// Página do PDF de origem (a partir de 1).
    pdf_page: u32,
    rotation: usize,
    custom_rotation: f32,
    /// 0 a 100.
    opacity: f32,
    relative: bool,
    /// % da página.
    scale: f32,
    behind: bool,
    vertical: (usize, f32),
    horizontal: (usize, f32),
    pub pages: usize,
    pub range: String,
    /// A janela pediu para escolher o arquivo de origem.
    pub wants_file: bool,
}

impl Default for WatermarkDialog {
    /// Como o Acrobat abre: texto, 45°, 50% de opacidade, centrada, na frente, todas as páginas.
    fn default() -> Self {
        Self {
            from_file: false,
            text: String::new(),
            font: 0,
            bold: true,
            size: 48.0,
            color: [200, 0, 0],
            path: None,
            pdf_page: 1,
            rotation: 0,
            custom_rotation: 30.0,
            opacity: 50.0,
            relative: false,
            scale: 50.0,
            behind: false,
            vertical: (1, 0.0),
            horizontal: (1, 0.0),
            pages: 0,
            range: String::new(),
            wants_file: false,
        }
    }
}

fn align(i: usize) -> Align {
    [Align::Start, Align::Center, Align::End][i.min(2)]
}

fn align_index(a: Align) -> usize {
    match a {
        Align::Start => 0,
        Align::Center => 1,
        Align::End => 2,
    }
}

/// Páginas escolhidas na janela.
pub fn page_set(choice: usize, range: &str) -> PageSet {
    match choice {
        1 => PageSet::Range(range.to_owned()),
        2 => PageSet::Even,
        3 => PageSet::Odd,
        _ => PageSet::All,
    }
}

/// Índice da escolha de páginas e o intervalo.
pub fn page_choice(p: &PageSet) -> (usize, String) {
    match p {
        PageSet::All => (0, String::new()),
        PageSet::Range(r) => (1, r.clone()),
        PageSet::Even => (2, String::new()),
        PageSet::Odd => (3, String::new()),
    }
}

/// Problema com o intervalo de páginas (o mesmo texto do pdf-core).
pub fn range_problem(choice: usize, range: &str, total: usize) -> Option<String> {
    (choice == 1)
        .then(|| pdf_core::parse_ranges(range, total).err())
        .flatten()
}

impl WatermarkDialog {
    /// A janela do Atualizar: as escolhas da marca existente.
    pub fn from_watermark(w: &Watermark) -> Self {
        let mut d = Self::default();
        match &w.source {
            WatermarkSource::Text { text, look } => {
                d.text = text.clone();
                d.font = FONTS.iter().position(|f| f.1 == look.font).unwrap_or(0);
                d.bold = look.bold;
                d.size = look.size;
                d.color = look.color;
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
            .take(3)
            .position(|r| r.1 == w.rotation)
            .unwrap_or(3);
        d.custom_rotation = w.rotation;
        d.opacity = w.opacity * 100.0;
        d.relative = w.scale.is_some();
        d.scale = w.scale.map_or(50.0, |s| s * 100.0);
        d.behind = w.behind;
        d.vertical = (align_index(w.vertical.0), w.vertical.1);
        d.horizontal = (align_index(w.horizontal.0), w.horizontal.1);
        (d.pages, d.range) = page_choice(&w.pages);
        d
    }

    fn rotation(&self) -> f32 {
        if self.rotation == 3 {
            self.custom_rotation
        } else {
            ROTATIONS[self.rotation].1
        }
    }

    /// A marca das escolhas, ou o que falta (para o OK ficar desligado).
    pub fn watermark(&self, total: usize) -> Result<Watermark, String> {
        let source = if self.from_file {
            let path = self
                .path
                .clone()
                .ok_or("Escolha o arquivo da marca d'água.")?;
            let is_pdf = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            if is_pdf {
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
                look: TextLook {
                    font: FONTS[self.font.min(2)].1,
                    bold: self.bold,
                    size: self.size,
                    color: self.color,
                },
            }
        };
        if let Some(p) = range_problem(self.pages, &self.range, total) {
            return Err(p);
        }
        Ok(Watermark {
            source,
            rotation: self.rotation(),
            opacity: (self.opacity / 100.0).clamp(0.0, 1.0),
            scale: self.relative.then_some(self.scale / 100.0),
            behind: self.behind,
            vertical: (align(self.vertical.0), self.vertical.1),
            horizontal: (align(self.horizontal.0), self.horizontal.1),
            pages: page_set(self.pages, &self.range),
        })
    }

    /// Desenha a janela. `Some(Some(_))`: OK; `Some(None)`: cancelou. `page` é o tamanho da página
    /// atual (para a prévia) e `total`, o número de páginas.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        page: (f32, f32),
        total: usize,
    ) -> Option<Option<Watermark>> {
        let mut out = None;
        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| self.choices(ui));
                    // (um separador vertical aqui esticaria a janela até a borda da tela)
                    ui.add_space(12.0);
                    self.preview(ui, page);
                });
                ui.separator();
                let result = self.watermark(total);
                if let Err(p) = &result {
                    ui.colored_label(ui.visuals().weak_text_color(), p);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(result.is_ok(), egui::Button::new("OK"))
                        .clicked()
                    {
                        out = result.ok().map(Some);
                    }
                    if ui.button("Cancelar").clicked() {
                        out = Some(None);
                    }
                });
            });
        if !open {
            return Some(None);
        }
        out
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
                ui.label(self.path.as_ref().map_or("(nenhum arquivo)".into(), |p| {
                    p.file_name()
                        .map_or(String::new(), |n| n.to_string_lossy().into_owned())
                }));
            });
            if self
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
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
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("marca-fonte")
                    .selected_text(FONTS[self.font].0)
                    .show_index(ui, &mut self.font, FONTS.len(), |i| FONTS[i].0);
                ui.checkbox(&mut self.bold, "Negrito");
                ui.add(
                    egui::DragValue::new(&mut self.size)
                        .range(4.0..=500.0)
                        .suffix(" pt"),
                );
                ui.color_edit_button_srgb(&mut self.color);
            });
        }
        ui.add_space(6.0);
        ui.strong("Aparência");
        ui.horizontal(|ui| {
            ui.label("Rotação");
            for (i, (label, _)) in ROTATIONS.iter().enumerate() {
                ui.radio_value(&mut self.rotation, i, *label);
            }
            if self.rotation == 3 {
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
        for (label, names, (choice, off), id) in [
            ("Vertical", VERTICAL, &mut self.vertical, "marca-v"),
            ("Horizontal", HORIZONTAL, &mut self.horizontal, "marca-h"),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(id)
                    .selected_text(names[*choice])
                    .show_index(ui, choice, 3, |i| names[i]);
                ui.add(egui::DragValue::new(off).suffix(" pt"));
            });
        }
        ui.add_space(6.0);
        pages_choice(ui, "marca-paginas", &mut self.pages, &mut self.range);
    }

    /// Prévia esquemática: a página e a marca (texto girado ou o quadro do arquivo).
    fn preview(&self, ui: &mut egui::Ui, page: (f32, f32)) {
        let h = 260.0;
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
        let k = w / page.0.max(1.0);
        let alpha = (self.opacity / 100.0 * 255.0) as u8;
        let at = |(choice, off): (usize, f32), lo: f32, hi: f32, size: f32| match choice {
            0 => lo + off * k + size / 2.0,
            2 => hi - off * k - size / 2.0,
            _ => (lo + hi) / 2.0 + off * k,
        };
        if self.from_file {
            let (bw, bh) = if self.relative {
                (w * self.scale / 100.0, h * self.scale / 100.0)
            } else {
                (w * 0.4, h * 0.3)
            };
            let c = egui::pos2(
                at(self.horizontal, rect.left(), rect.right(), bw),
                at(self.vertical, rect.top(), rect.bottom(), bh),
            );
            let r = egui::Rect::from_center_size(c, egui::vec2(bw, bh));
            painter.rect_filled(
                r,
                0.0,
                egui::Color32::from_gray(160).gamma_multiply(alpha as f32 / 255.0),
            );
            return;
        }
        let [cr, cg, cb] = self.color;
        let galley = painter.layout(
            self.text.clone(),
            egui::FontId::proportional((self.size * k).max(4.0)),
            egui::Color32::from_rgba_unmultiplied(cr, cg, cb, alpha),
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
        let half = egui::vec2(size.x / 2.0, size.y / 2.0);
        let corner = c - egui::vec2(half.x * co - half.y * s, half.x * s + half.y * co);
        painter.add(
            egui::epaint::TextShape::new(corner, galley, egui::Color32::BLACK).with_angle(angle),
        );
    }
}

/// Escolha de páginas (todas, intervalo, pares, ímpares), comum às duas janelas.
pub fn pages_choice(ui: &mut egui::Ui, id: &str, choice: &mut usize, range: &mut String) {
    ui.horizontal(|ui| {
        ui.label("Páginas");
        egui::ComboBox::from_id_salt(id)
            .selected_text(PAGES[*choice])
            .show_index(ui, choice, PAGES.len(), |i| PAGES[i]);
        if *choice == 1 {
            ui.add(
                egui::TextEdit::singleline(range)
                    .desired_width(90.0)
                    .hint_text("1-3, 5"),
            );
        }
    });
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
            pages: 1,
            range: "2-9".into(),
            ..Default::default()
        };
        assert!(d.watermark(3).unwrap_err().contains("3 páginas"));
        d.range = "2-3".into();
        assert_eq!(d.watermark(3).unwrap().pages, PageSet::Range("2-3".into()));
    }

    #[test]
    fn updating_starts_from_the_current_mark() {
        let mut d = WatermarkDialog {
            text: "FINAL".into(),
            rotation: 3,
            custom_rotation: 30.0,
            relative: true,
            scale: 40.0,
            behind: true,
            pages: 3,
            ..Default::default()
        };
        d.vertical = (0, 20.0);
        let w = d.watermark(5).unwrap();
        let back = WatermarkDialog::from_watermark(&w).watermark(5).unwrap();
        assert_eq!(back, w);
    }
}
