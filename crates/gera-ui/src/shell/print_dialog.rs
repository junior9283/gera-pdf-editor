//! Diálogo "Imprimir" no arranjo do Acrobat: impressora, cópias, páginas, dimensionamento,
//! orientação, comentários e pré-visualização. Quem envia ao CUPS é o `DocTab`.
use crate::view::DocView;
use app_state::print::{parse_ranges, Duplex, Orientation, Pages, PrintOptions, Sizing, Subset};
use pdf_core::Worker;

pub enum PrintChoice {
    Print(PrintOptions),
    Cancel,
}

pub struct PrintDialog {
    pub options: PrintOptions,
    /// Texto do campo "Páginas" (vale quando `options.pages` é `Ranges`).
    pub ranges: String,
    pub printers: Vec<String>,
    /// Página mostrada na pré-visualização (0-based).
    preview: usize,
}

/// Largura da pré-visualização, em pontos da tela.
const PREVIEW_WIDTH: f32 = 220.0;

impl PrintDialog {
    /// `current`: página atual (0-based), que também abre a pré-visualização.
    pub fn new(printers: Vec<String>, default: Option<String>, current: usize) -> Self {
        let printer = default
            .filter(|d| printers.contains(d))
            .or_else(|| printers.first().cloned())
            .unwrap_or_default();
        Self {
            options: PrintOptions {
                printer,
                ..Default::default()
            },
            ranges: String::new(),
            printers,
            preview: current,
        }
    }

    /// O que impede de imprimir agora (o botão fica desativado e a mensagem aparece).
    fn problem(&self, total: usize) -> Option<String> {
        if self.options.printer.is_empty() {
            return Some("Nenhuma impressora encontrada.".into());
        }
        match self.options.pages {
            Pages::Ranges(_) => parse_ranges(&self.ranges, total).err(),
            _ => None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &mut DocView,
        worker: &Worker,
    ) -> Option<PrintChoice> {
        let total = view.sizes.len();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("imprimir")).show(ui.ctx(), |ui| {
            ui.heading("Imprimir");
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| self.options_ui(ui, total));
                ui.separator();
                ui.vertical(|ui| self.preview_ui(ui, view, worker, total));
            });
            ui.separator();
            let problem = self.problem(total);
            if let Some(p) = &problem {
                ui.colored_label(ui.visuals().error_fg_color, p);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(problem.is_none(), egui::Button::new("Imprimir"))
                    .clicked()
                {
                    let mut options = self.options.clone();
                    if let Pages::Ranges(_) = options.pages {
                        options.pages = Pages::Ranges(self.ranges.clone());
                    }
                    choice = Some(PrintChoice::Print(options));
                }
                if ui.button("Cancelar").clicked() {
                    choice = Some(PrintChoice::Cancel);
                }
            });
        });
        choice
    }

    fn options_ui(&mut self, ui: &mut egui::Ui, total: usize) {
        let o = &mut self.options;
        egui::Grid::new("imprimir-opcoes")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Impressora:");
                egui::ComboBox::from_id_salt("impressora")
                    .selected_text(if o.printer.is_empty() {
                        "—"
                    } else {
                        &o.printer
                    })
                    .show_ui(ui, |ui| {
                        for p in &self.printers {
                            ui.selectable_value(&mut o.printer, p.clone(), p);
                        }
                    });
                ui.end_row();
                ui.label("Cópias:");
                ui.add(egui::DragValue::new(&mut o.copies).range(1..=999));
                ui.end_row();
            });
        ui.checkbox(&mut o.grayscale, "Imprimir em escala de cinza");
        ui.add_space(6.0);
        ui.strong("Páginas a imprimir");
        ui.radio_value(&mut o.pages, Pages::All, format!("Todas ({total})"));
        ui.radio_value(&mut o.pages, Pages::Current, "Página atual");
        ui.horizontal(|ui| {
            let is_ranges = matches!(o.pages, Pages::Ranges(_));
            if ui.radio(is_ranges, "Páginas").clicked() {
                o.pages = Pages::Ranges(String::new());
            }
            let field = ui.add_enabled(
                matches!(o.pages, Pages::Ranges(_)),
                egui::TextEdit::singleline(&mut self.ranges)
                    .hint_text("1-3, 7")
                    .desired_width(110.0),
            );
            if field.gained_focus() {
                o.pages = Pages::Ranges(String::new());
            }
        });
        egui::CollapsingHeader::new("Mais opções").show(ui, |ui| {
            egui::ComboBox::from_id_salt("subconjunto")
                .selected_text(o.subset.label())
                .show_ui(ui, |ui| {
                    for v in Subset::ALL {
                        ui.selectable_value(&mut o.subset, v, v.label());
                    }
                });
            ui.checkbox(&mut o.reverse, "Ordem inversa");
        });
        ui.add_space(6.0);
        ui.strong("Dimensionamento e manuseio de página");
        ui.horizontal(|ui| {
            ui.radio_value(&mut o.sizing, Sizing::Fit, "Ajustar");
            ui.radio_value(&mut o.sizing, Sizing::Actual, "Tamanho real");
        });
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut o.sizing,
                Sizing::ShrinkLarge,
                "Reduzir páginas grandes",
            );
        });
        ui.horizontal(|ui| {
            let custom = matches!(o.sizing, Sizing::Custom(_));
            let mut pct = match o.sizing {
                Sizing::Custom(p) => p,
                _ => 100,
            };
            if ui.radio(custom, "Escala personalizada:").clicked() {
                o.sizing = Sizing::Custom(pct);
            }
            if ui
                .add_enabled(
                    custom,
                    egui::DragValue::new(&mut pct).range(1..=1000).suffix(" %"),
                )
                .changed()
            {
                o.sizing = Sizing::Custom(pct);
            }
        });
        egui::Grid::new("imprimir-folha")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Páginas por folha:");
                egui::ComboBox::from_id_salt("por-folha")
                    .selected_text(o.per_sheet.to_string())
                    .show_ui(ui, |ui| {
                        for n in [1u8, 2, 4, 6, 9] {
                            ui.selectable_value(&mut o.per_sheet, n, n.to_string());
                        }
                    });
                ui.end_row();
                ui.label("Frente e verso:");
                egui::ComboBox::from_id_salt("duplex")
                    .selected_text(o.duplex.label())
                    .show_ui(ui, |ui| {
                        for v in Duplex::ALL {
                            ui.selectable_value(&mut o.duplex, v, v.label());
                        }
                    });
                ui.end_row();
                ui.label("Orientação:");
                egui::ComboBox::from_id_salt("orientacao")
                    .selected_text(o.orientation.label())
                    .show_ui(ui, |ui| {
                        for v in Orientation::ALL {
                            ui.selectable_value(&mut o.orientation, v, v.label());
                        }
                    });
                ui.end_row();
                ui.label("Comentários e formulários:");
                egui::ComboBox::from_id_salt("comentarios")
                    .selected_text(if o.with_annots {
                        "Documento e marcações"
                    } else {
                        "Documento"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut o.with_annots, true, "Documento e marcações");
                        ui.selectable_value(&mut o.with_annots, false, "Documento");
                    });
                ui.end_row();
            });
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, view: &mut DocView, worker: &Worker, total: usize) {
        ui.strong("Pré-visualização");
        let Some(size) = view.sizes.get(self.preview).copied() else {
            return;
        };
        let scale = PREVIEW_WIDTH / size.width.max(1.0);
        let height = size.height * scale;
        view.request_at(worker, self.preview as i32, scale);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(PREVIEW_WIDTH, height), egui::Sense::hover());
        match view.texture_at(self.preview as i32, scale) {
            Some(id) => {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(id, rect, uv, egui::Color32::WHITE);
            }
            None => {
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_gray(230));
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.preview > 0, egui::Button::new("◀"))
                .clicked()
            {
                self.preview -= 1;
            }
            ui.label(format!("Página {} de {total}", self.preview + 1));
            if ui
                .add_enabled(self.preview + 1 < total, egui::Button::new("▶"))
                .clicked()
            {
                self.preview += 1;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn starts_on_the_default_printer_or_the_first_one() {
        let d = PrintDialog::new(list(&["A", "B"]), Some("B".into()), 0);
        assert_eq!(d.options.printer, "B");
        let d = PrintDialog::new(list(&["A", "B"]), Some("sumiu".into()), 0);
        assert_eq!(d.options.printer, "A");
        let d = PrintDialog::new(vec![], None, 0);
        assert_eq!(d.options.printer, "");
    }

    #[test]
    fn printing_needs_a_printer_and_valid_ranges() {
        let mut d = PrintDialog::new(list(&["A"]), None, 0);
        assert_eq!(d.problem(10), None);
        d.options.pages = Pages::Ranges(String::new());
        d.ranges = "0".into();
        assert!(d.problem(10).is_some());
        d.ranges = "1-3".into();
        assert_eq!(d.problem(10), None);
        let none = PrintDialog::new(vec![], None, 0);
        assert_eq!(
            none.problem(10).as_deref(),
            Some("Nenhuma impressora encontrada.")
        );
    }
}
