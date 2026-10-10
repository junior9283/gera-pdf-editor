//! Janela "Adicionar cabeçalho e rodapé" (como no Acrobat): seis caixas, fonte, margens, número da
//! página, data e numeração Bates, páginas e prévia.
use crate::shell::watermark_dialog::{page_choice, page_set, pages_choice, range_problem, FONTS};
use pdf_core::{Bates, DateFormat, HeaderFooter, TextLook};

const BOXES: [&str; 6] = [
    "Cabeçalho à esquerda",
    "Cabeçalho ao centro",
    "Cabeçalho à direita",
    "Rodapé à esquerda",
    "Rodapé ao centro",
    "Rodapé à direita",
];
/// Formatos do número da página (o que "Inserir número da página" escreve).
const NUMBER_FORMATS: [(&str, &str); 4] = [
    ("1", "<<n>>"),
    ("1 de n", "<<n>> de <<total>>"),
    ("Página 1", "Página <<n>>"),
    ("Página 1 de n", "Página <<n>> de <<total>>"),
];
const DATE_FORMATS: [(&str, DateFormat); 3] = [
    ("dd/mm/aaaa", DateFormat::DayMonthYear),
    ("dd/mm/aa", DateFormat::DayMonthShort),
    ("aaaa-mm-dd", DateFormat::Iso),
];

pub struct HeaderFooterDialog {
    pub texts: [String; 6],
    /// Caixa em que "Inserir…" escreve (a última com o foco).
    target: usize,
    font: usize,
    bold: bool,
    size: f32,
    color: [u8; 3],
    margins: [f32; 4],
    start: u32,
    number_format: usize,
    date_format: usize,
    use_bates: bool,
    bates: Bates,
    pages: usize,
    range: String,
}

impl Default for HeaderFooterDialog {
    /// Como o Acrobat abre: Helvetica 10 preta, margens de 0,5 pol (36 pt), número a partir de 1.
    fn default() -> Self {
        Self {
            texts: Default::default(),
            target: 5,
            font: 0,
            bold: false,
            size: 10.0,
            color: [0, 0, 0],
            margins: [36.0; 4],
            start: 1,
            number_format: 0,
            date_format: 0,
            use_bates: false,
            bates: Bates {
                prefix: String::new(),
                suffix: String::new(),
                digits: 6,
                start: 1,
            },
            pages: 0,
            range: String::new(),
        }
    }
}

impl HeaderFooterDialog {
    /// A janela do Atualizar: as escolhas do cabeçalho e rodapé existentes.
    pub fn from_header_footer(h: &HeaderFooter) -> Self {
        let mut d = Self {
            texts: h.texts.clone(),
            font: FONTS.iter().position(|f| f.1 == h.look.font).unwrap_or(0),
            bold: h.look.bold,
            size: h.look.size,
            color: h.look.color,
            margins: h.margins,
            start: h.start,
            date_format: DATE_FORMATS
                .iter()
                .position(|f| f.1 == h.date_format)
                .unwrap_or(0),
            use_bates: h.bates.is_some(),
            ..Self::default()
        };
        if let Some(b) = &h.bates {
            d.bates = b.clone();
        }
        (d.pages, d.range) = page_choice(&h.pages);
        d
    }

    /// Escreve `token` no fim da caixa escolhida (como os botões "Inserir" do Acrobat).
    pub fn insert(&mut self, token: &str) {
        let t = &mut self.texts[self.target];
        if !t.is_empty() && !t.ends_with(' ') {
            t.push(' ');
        }
        t.push_str(token);
    }

    pub fn header_footer(&self, total: usize) -> Result<HeaderFooter, String> {
        if self.texts.iter().all(|t| t.trim().is_empty()) {
            return Err("Escreva o texto de alguma das caixas.".into());
        }
        if self.texts.iter().any(|t| t.contains("<<bates>>")) && !self.use_bates {
            return Err("Ligue a numeração Bates ou tire <<bates>> das caixas.".into());
        }
        if let Some(p) = range_problem(self.pages, &self.range, total) {
            return Err(p);
        }
        Ok(HeaderFooter {
            texts: self.texts.clone(),
            look: TextLook {
                font: FONTS[self.font.min(2)].1,
                bold: self.bold,
                size: self.size,
                color: self.color,
            },
            margins: self.margins,
            start: self.start,
            date_format: DATE_FORMATS[self.date_format.min(2)].1,
            bates: self.use_bates.then(|| self.bates.clone()),
            pages: page_set(self.pages, &self.range),
        })
    }

    /// Desenha a janela. `Some(Some(_))`: OK; `Some(None)`: cancelou.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        page: (f32, f32),
        total: usize,
    ) -> Option<Option<HeaderFooter>> {
        let mut out = None;
        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                self.look_row(ui);
                ui.separator();
                // duas linhas (cabeçalho, rodapé) de três caixas de largura fixa
                for row in [0..3, 3..6] {
                    ui.horizontal_top(|ui| {
                        for i in row {
                            ui.vertical(|ui| {
                                ui.set_width(180.0);
                                ui.small(BOXES[i]);
                                let r = ui.add(
                                    egui::TextEdit::multiline(&mut self.texts[i])
                                        .desired_rows(2)
                                        .desired_width(176.0),
                                );
                                if r.has_focus() || r.gained_focus() {
                                    self.target = i;
                                }
                            });
                        }
                    });
                }
                self.insert_row(ui);
                ui.separator();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        self.bates_row(ui);
                        pages_choice(ui, "cabecalho-paginas", &mut self.pages, &mut self.range);
                    });
                    ui.add_space(12.0);
                    self.preview(ui, page, total);
                });
                ui.separator();
                let result = self.header_footer(total);
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

    fn look_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Fonte");
            egui::ComboBox::from_id_salt("cabecalho-fonte")
                .selected_text(FONTS[self.font].0)
                .show_index(ui, &mut self.font, FONTS.len(), |i| FONTS[i].0);
            ui.checkbox(&mut self.bold, "Negrito");
            ui.add(
                egui::DragValue::new(&mut self.size)
                    .range(4.0..=72.0)
                    .suffix(" pt"),
            );
            ui.color_edit_button_srgb(&mut self.color);
        });
        ui.horizontal(|ui| {
            ui.label("Margens (pt):");
            for (label, m) in ["Superior", "Inferior", "Esquerda", "Direita"]
                .iter()
                .zip(&mut self.margins)
            {
                ui.label(*label);
                ui.add(egui::DragValue::new(m).range(0.0..=300.0));
            }
        });
    }

    fn insert_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("cabecalho-numero")
                .selected_text(NUMBER_FORMATS[self.number_format].0)
                .show_index(ui, &mut self.number_format, NUMBER_FORMATS.len(), |i| {
                    NUMBER_FORMATS[i].0
                });
            if ui
                .button("Inserir número da página")
                .on_hover_text(format!("Na caixa: {}", BOXES[self.target]))
                .clicked()
            {
                self.insert(NUMBER_FORMATS[self.number_format].1);
            }
            ui.label("Começar em");
            ui.add(egui::DragValue::new(&mut self.start).range(1..=999_999));
            ui.separator();
            egui::ComboBox::from_id_salt("cabecalho-data")
                .selected_text(DATE_FORMATS[self.date_format].0)
                .show_index(ui, &mut self.date_format, DATE_FORMATS.len(), |i| {
                    DATE_FORMATS[i].0
                });
            if ui.button("Inserir data").clicked() {
                self.insert("<<data>>");
            }
        });
    }

    fn bates_row(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.use_bates, "Numeração Bates");
        ui.add_enabled_ui(self.use_bates, |ui| {
            egui::Grid::new("bates").num_columns(4).show(ui, |ui| {
                ui.label("Prefixo");
                ui.add(egui::TextEdit::singleline(&mut self.bates.prefix).desired_width(80.0));
                ui.label("Sufixo");
                ui.add(egui::TextEdit::singleline(&mut self.bates.suffix).desired_width(80.0));
                ui.end_row();
                ui.label("Dígitos");
                ui.add(egui::DragValue::new(&mut self.bates.digits).range(1..=12));
                ui.label("Começar em");
                ui.add(egui::DragValue::new(&mut self.bates.start).range(0..=u32::MAX));
                ui.end_row();
            });
            if ui.button("Inserir número Bates").clicked() {
                self.insert("<<bates>>");
            }
        });
    }

    /// Prévia: a primeira página escolhida com as seis caixas preenchidas.
    fn preview(&self, ui: &mut egui::Ui, page: (f32, f32), total: usize) {
        let h = 220.0;
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
        let today = chrono::Local::now()
            .format(DATE_FORMATS[self.date_format.min(2)].1.pattern())
            .to_string();
        let bates = format!(
            "{}{:0d$}{}",
            self.bates.prefix,
            self.bates.start,
            self.bates.suffix,
            d = self.bates.digits.clamp(1, 12) as usize
        );
        let [cr, cg, cb] = self.color;
        let [mt, mb, ml, mr] = self.margins.map(|m| m * k);
        for (i, text) in self.texts.iter().enumerate() {
            let text = text
                .replace("<<n>>", &self.start.to_string())
                .replace("<<total>>", &total.to_string())
                .replace("<<data>>", &today)
                .replace("<<bates>>", &bates);
            if text.trim().is_empty() {
                continue;
            }
            let anchor = [
                egui::Align2::LEFT_TOP,
                egui::Align2::CENTER_TOP,
                egui::Align2::RIGHT_TOP,
            ][i % 3];
            let x = [rect.left() + ml, rect.center().x, rect.right() - mr][i % 3];
            let (y, anchor) = if i < 3 {
                (rect.top() + mt, anchor)
            } else {
                let bottom = [
                    egui::Align2::LEFT_BOTTOM,
                    egui::Align2::CENTER_BOTTOM,
                    egui::Align2::RIGHT_BOTTOM,
                ][i % 3];
                (rect.bottom() - mb, bottom)
            };
            painter.text(
                egui::pos2(x, y),
                anchor,
                text,
                egui::FontId::proportional((self.size * k).max(5.0)),
                egui::Color32::from_rgb(cr, cg, cb),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserting_writes_into_the_chosen_box() {
        let mut d = HeaderFooterDialog::default();
        assert_eq!(d.target, 5, "rodapé à direita, como no Acrobat");
        d.texts[5] = "Folha".into();
        d.insert(NUMBER_FORMATS[1].1);
        assert_eq!(d.texts[5], "Folha <<n>> de <<total>>");
        d.target = 0;
        d.insert("<<data>>");
        assert_eq!(d.texts[0], "<<data>>");
    }

    #[test]
    fn something_must_be_written_and_bates_needs_its_option() {
        let mut d = HeaderFooterDialog::default();
        assert!(d.header_footer(3).is_err());
        d.texts[4] = "<<bates>>".into();
        assert!(d.header_footer(3).unwrap_err().contains("Bates"));
        d.use_bates = true;
        d.bates.prefix = "PROC".into();
        let h = d.header_footer(3).unwrap();
        assert_eq!(h.bates.unwrap().prefix, "PROC");
        assert_eq!(h.margins, [36.0; 4]);
    }

    #[test]
    fn updating_starts_from_the_current_choices() {
        let mut d = HeaderFooterDialog {
            start: 7,
            date_format: 2,
            pages: 2,
            ..Default::default()
        };
        d.texts[1] = "Relatório".into();
        let h = d.header_footer(5).unwrap();
        assert_eq!(
            HeaderFooterDialog::from_header_footer(&h)
                .header_footer(5)
                .unwrap(),
            h
        );
    }
}
