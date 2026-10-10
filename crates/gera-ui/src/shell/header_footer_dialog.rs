//! Janela "Adicionar cabeçalho e rodapé" (como no Acrobat): seis caixas, fonte, margens, número da
//! página, data e numeração Bates, páginas e prévia.
use crate::shell::watermark_dialog::{look_editor, ok_cancel_window, preview_page, Pages};
use pdf_core::{Bates, DateFormat, HeaderFooter, StdFont, TextLook};

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
    /// As escolhas, menos o Bates ligado/desligado e as páginas (que a janela guarda à parte).
    h: HeaderFooter,
    /// Caixa em que "Inserir…" escreve (a última com o foco).
    target: usize,
    number_format: usize,
    use_bates: bool,
    bates: Bates,
    pages: Pages,
    /// Data de hoje no formato escolhido (para a prévia; refeita quando o formato muda).
    today: (DateFormat, String),
}

impl Default for HeaderFooterDialog {
    /// Como o Acrobat abre: Helvetica 10 preta, margens de 0,5 pol (36 pt), número a partir de 1.
    fn default() -> Self {
        let date_format = DateFormat::default();
        Self {
            h: HeaderFooter {
                texts: Default::default(),
                look: TextLook {
                    font: StdFont::Helvetica,
                    bold: false,
                    size: 10.0,
                    color: [0, 0, 0],
                },
                margins: [36.0; 4],
                start: 1,
                date_format,
                bates: None,
                pages: pdf_core::PageSet::All,
            },
            target: 5,
            number_format: 0,
            use_bates: false,
            bates: Bates {
                prefix: String::new(),
                suffix: String::new(),
                digits: 6,
                start: 1,
            },
            pages: Pages::default(),
            today: (date_format, date_format.today()),
        }
    }
}

impl HeaderFooterDialog {
    /// A janela do Atualizar: as escolhas do cabeçalho e rodapé existentes.
    pub fn from_header_footer(h: &HeaderFooter) -> Self {
        let d = Self::default();
        Self {
            use_bates: h.bates.is_some(),
            bates: h.bates.clone().unwrap_or(d.bates),
            pages: Pages::from_set(&h.pages),
            today: (h.date_format, h.date_format.today()),
            h: h.clone(),
            ..d
        }
    }

    #[cfg(test)]
    pub fn texts(&mut self) -> &mut [String; 6] {
        &mut self.h.texts
    }

    /// Escreve `token` no fim da caixa escolhida (como os botões "Inserir" do Acrobat).
    pub fn insert(&mut self, token: &str) {
        let t = &mut self.h.texts[self.target];
        if !t.is_empty() && !t.ends_with(' ') {
            t.push(' ');
        }
        t.push_str(token);
    }

    pub fn header_footer(&self, total: usize) -> Result<HeaderFooter, String> {
        if self.h.texts.iter().all(|t| t.trim().is_empty()) {
            return Err("Escreva o texto de alguma das caixas.".into());
        }
        if self.h.texts.iter().any(|t| t.contains("<<bates>>")) && !self.use_bates {
            return Err("Ligue a numeração Bates ou tire <<bates>> das caixas.".into());
        }
        Ok(HeaderFooter {
            bates: self.use_bates.then(|| self.bates.clone()),
            pages: self.pages.set(total)?,
            ..self.h.clone()
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
        if self.today.0 != self.h.date_format {
            self.today = (self.h.date_format, self.h.date_format.today());
        }
        ok_cancel_window(ctx, title, |ui| {
            look_editor(ui, "cabecalho-fonte", &mut self.h.look, 72.0);
            ui.horizontal(|ui| {
                ui.label("Margens (pt):");
                for (label, m) in ["Superior", "Inferior", "Esquerda", "Direita"]
                    .iter()
                    .zip(&mut self.h.margins)
                {
                    ui.label(*label);
                    ui.add(egui::DragValue::new(m).range(0.0..=300.0));
                }
            });
            ui.separator();
            // duas linhas (cabeçalho, rodapé) de três caixas de largura fixa
            for row in [0..3, 3..6] {
                ui.horizontal_top(|ui| {
                    for i in row {
                        ui.vertical(|ui| {
                            ui.set_width(180.0);
                            ui.small(BOXES[i]);
                            let r = ui.add(
                                egui::TextEdit::multiline(&mut self.h.texts[i])
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
                    self.pages.ui(ui, "cabecalho-paginas");
                });
                ui.add_space(12.0);
                self.preview(ui, page, total);
            });
            self.header_footer(total)
        })
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
            ui.add(egui::DragValue::new(&mut self.h.start).range(1..=999_999));
            ui.separator();
            let current = DATE_FORMATS
                .iter()
                .find(|f| f.1 == self.h.date_format)
                .map_or("", |f| f.0);
            egui::ComboBox::from_id_salt("cabecalho-data")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (label, f) in DATE_FORMATS {
                        ui.selectable_value(&mut self.h.date_format, f, label);
                    }
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

    /// Prévia: a primeira página escolhida com as seis caixas preenchidas (pela mesma regra do
    /// pdf-core).
    fn preview(&self, ui: &mut egui::Ui, page: (f32, f32), total: usize) {
        let (painter, rect, k) = preview_page(ui, page, 220.0);
        let shown = HeaderFooter {
            bates: self.use_bates.then(|| self.bates.clone()),
            ..self.h.clone()
        };
        let [cr, cg, cb] = self.h.look.color;
        let [mt, mb, ml, mr] = self.h.margins.map(|m| m * k);
        for (i, text) in self.h.texts.iter().enumerate() {
            let text = shown.expand(text, 0, total as i32, &self.today.1);
            if text.trim().is_empty() {
                continue;
            }
            use egui::Align::{Center, Max, Min};
            let header = i < 3;
            let anchor = egui::Align2([[Min, Center, Max][i % 3], if header { Min } else { Max }]);
            let x = [rect.left() + ml, rect.center().x, rect.right() - mr][i % 3];
            let y = if header {
                rect.top() + mt
            } else {
                rect.bottom() - mb
            };
            painter.text(
                egui::pos2(x, y),
                anchor,
                text,
                egui::FontId::proportional((self.h.look.size * k).max(5.0)),
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
        d.texts()[5] = "Folha".into();
        d.insert(NUMBER_FORMATS[1].1);
        assert_eq!(d.texts()[5], "Folha <<n>> de <<total>>");
        d.target = 0;
        d.insert("<<data>>");
        assert_eq!(d.texts()[0], "<<data>>");
    }

    #[test]
    fn something_must_be_written_and_bates_needs_its_option() {
        let mut d = HeaderFooterDialog::default();
        assert!(d.header_footer(3).is_err());
        d.texts()[4] = "<<bates>>".into();
        assert!(d.header_footer(3).unwrap_err().contains("Bates"));
        d.use_bates = true;
        d.bates.prefix = "PROC".into();
        let h = d.header_footer(3).unwrap();
        assert_eq!(h.bates.unwrap().prefix, "PROC");
        assert_eq!(h.margins, [36.0; 4]);
    }

    #[test]
    fn updating_starts_from_the_current_choices() {
        let mut d = HeaderFooterDialog::default();
        d.h.start = 7;
        d.h.date_format = DateFormat::Iso;
        d.pages = Pages::from_set(&pdf_core::PageSet::Even);
        d.texts()[1] = "Relatório".into();
        let h = d.header_footer(5).unwrap();
        assert_eq!(
            HeaderFooterDialog::from_header_footer(&h)
                .header_footer(5)
                .unwrap(),
            h
        );
    }
}
