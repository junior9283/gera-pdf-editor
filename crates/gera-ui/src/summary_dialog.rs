//! Diálogo Resumir comentários (menu "···" do painel Comentários), como o do Acrobat.
use pdf_core::{SummaryFont, SummaryLayout, SummaryOptions, SummarySort};

/// Escolhas do diálogo (páginas 1-based, como na tela).
pub struct SummaryDialog {
    pub layout: SummaryLayout,
    pub sort: SummarySort,
    /// Só os comentários que o filtro do painel mostra.
    pub only_shown: bool,
    pub all_pages: bool,
    pub from: i32,
    pub to: i32,
    pub font: SummaryFont,
    pub include_empty: bool,
    pages: i32,
}

/// O que o diálogo pediu neste quadro.
pub enum SummaryChoice {
    Create,
    Cancel,
}

impl SummaryDialog {
    pub fn new(pages: i32) -> Self {
        Self {
            layout: SummaryLayout::ConnectorSeparate,
            sort: SummarySort::Page,
            only_shown: false,
            all_pages: true,
            from: 1,
            to: pages.max(1),
            font: SummaryFont::Medium,
            include_empty: false,
            pages: pages.max(1),
        }
    }

    /// As opções para o núcleo; `shown`: os comentários que o filtro do painel mostra.
    pub fn options(&self, shown: Option<Vec<(i32, i32)>>) -> SummaryOptions {
        let last = self.pages;
        let from = self.from.clamp(1, last);
        let to = self.to.clamp(from, last);
        SummaryOptions {
            layout: self.layout,
            sort: self.sort,
            only: if self.only_shown { shown } else { None },
            pages: (!self.all_pages).then_some((from - 1, to - 1)),
            font: self.font,
            include_empty: self.include_empty,
        }
    }

    /// Desenha a janela; devolve a escolha quando o usuário clica num botão.
    pub fn window(&mut self, ctx: &egui::Context) -> Option<SummaryChoice> {
        let mut choice = None;
        let mut open = true;
        egui::Window::new("Resumir comentários")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.strong("Layout");
                for l in SummaryLayout::ALL {
                    ui.radio_value(&mut self.layout, l, l.label());
                }
                ui.separator();
                egui::Grid::new("resumo-opcoes")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Ordenar comentários por:");
                        egui::ComboBox::from_id_salt("resumo-ordem")
                            .selected_text(self.sort.label())
                            .show_ui(ui, |ui| {
                                for s in SummarySort::ALL {
                                    ui.selectable_value(&mut self.sort, s, s.label());
                                }
                            });
                        ui.end_row();
                        ui.label("Tamanho da fonte:");
                        ui.horizontal(|ui| {
                            for f in SummaryFont::ALL {
                                ui.radio_value(&mut self.font, f, f.label());
                            }
                        });
                        ui.end_row();
                    });
                ui.separator();
                ui.radio_value(&mut self.only_shown, false, "Todos os comentários");
                ui.radio_value(
                    &mut self.only_shown,
                    true,
                    "Só os comentários mostrados no painel (filtro)",
                );
                ui.separator();
                ui.radio_value(&mut self.all_pages, true, "Todas as páginas");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.all_pages, false, "Páginas de");
                    let last = self.pages;
                    ui.add_enabled(
                        !self.all_pages,
                        egui::DragValue::new(&mut self.from).range(1..=last),
                    );
                    ui.label("a");
                    ui.add_enabled(
                        !self.all_pages,
                        egui::DragValue::new(&mut self.to).range(1..=last),
                    );
                });
                ui.checkbox(&mut self.include_empty, "Incluir páginas sem comentários");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Criar resumo").clicked() {
                        choice = Some(SummaryChoice::Create);
                    }
                    if ui.button("Cancelar").clicked() {
                        choice = Some(SummaryChoice::Cancel);
                    }
                });
            });
        if !open {
            choice = Some(SummaryChoice::Cancel);
        }
        choice
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_from_dialog() {
        let mut d = SummaryDialog::new(10);
        d.layout = SummaryLayout::NumbersSeparate;
        d.only_shown = true;
        d.all_pages = false;
        d.from = 3;
        d.to = 99;
        let o = d.options(Some(vec![(2, 7)]));
        assert_eq!(o.layout, SummaryLayout::NumbersSeparate);
        assert_eq!(o.only, Some(vec![(2, 7)]));
        assert_eq!(
            o.pages,
            Some((2, 9)),
            "1-based na tela, 0-based no núcleo, até a última"
        );
        d.only_shown = false;
        d.all_pages = true;
        let o = d.options(Some(vec![(2, 7)]));
        assert_eq!((o.only, o.pages), (None, None));
    }
}
