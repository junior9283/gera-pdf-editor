//! Diálogo Rótulos de página (como no Acrobat): Páginas (Todas, Selecionadas, De–Até) e
//! Numeração (Iniciar nova seção, com Estilo, Prefixo e Início; ou Estender a numeração usada na
//! seção anterior).
use pdf_core::{format_label, PageLabelRange, PageLabelStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Selected,
    Range,
}

const STYLES: [PageLabelStyle; 6] = [
    PageLabelStyle::None,
    PageLabelStyle::Decimal,
    PageLabelStyle::LowerRoman,
    PageLabelStyle::UpperRoman,
    PageLabelStyle::LowerAlpha,
    PageLabelStyle::UpperAlpha,
];

fn style_label(style: PageLabelStyle) -> &'static str {
    match style {
        PageLabelStyle::None => "Nenhum",
        PageLabelStyle::Decimal => "1, 2, 3, …",
        PageLabelStyle::LowerRoman => "i, ii, iii, …",
        PageLabelStyle::UpperRoman => "I, II, III, …",
        PageLabelStyle::LowerAlpha => "a, b, c, …",
        PageLabelStyle::UpperAlpha => "A, B, C, …",
    }
}

pub struct PageLabelsDialog {
    pub scope: Scope,
    /// De e Até (a partir de 1), para `Scope::Range`.
    pub from: usize,
    pub to: usize,
    /// Primeira e última selecionadas (a partir de 0), se houver seleção.
    selected: Option<(i32, i32)>,
    total: usize,
    /// Iniciar nova seção (senão, estender a anterior).
    pub new_section: bool,
    pub style: PageLabelStyle,
    pub prefix: String,
    pub start: i32,
}

impl PageLabelsDialog {
    pub fn new(selected: &[i32], total: usize) -> Self {
        let selected = selected.iter().min().zip(selected.iter().max());
        let selected = selected.map(|(&a, &b)| (a, b));
        let (from, to) = selected.map_or((1, total), |(a, b)| (a as usize + 1, b as usize + 1));
        Self {
            scope: if selected.is_some() {
                Scope::Selected
            } else {
                Scope::All
            },
            from,
            to,
            selected,
            total,
            new_section: true,
            style: PageLabelStyle::Decimal,
            prefix: String::new(),
            start: 1,
        }
    }

    /// O pedido para o worker: páginas `from..=to` (a partir de 0) e a seção nova, se houver.
    pub fn request(&self) -> (i32, i32, Option<PageLabelRange>) {
        let last = self.total.max(1) as i32 - 1;
        let (from, to) = match (self.scope, self.selected) {
            (Scope::All, _) | (Scope::Selected, None) => (0, last),
            (Scope::Selected, Some((a, b))) => (a, b),
            (Scope::Range, _) => {
                let a = (self.from.max(1) as i32 - 1).min(last);
                let b = (self.to.max(1) as i32 - 1).min(last);
                (a.min(b), a.max(b))
            }
        };
        let new = self.new_section.then(|| PageLabelRange {
            start_page: from,
            style: self.style,
            prefix: self.prefix.clone(),
            first: self.start.max(1),
        });
        (from, to, new)
    }

    /// Desenha; devolve `Some(true)` no OK e `Some(false)` no Cancelar.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut answer = None;
        egui::Window::new("Rótulos de página")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.strong("Páginas");
                ui.radio_value(&mut self.scope, Scope::All, "Todas");
                ui.add_enabled_ui(self.selected.is_some(), |ui| {
                    ui.radio_value(&mut self.scope, Scope::Selected, "Selecionadas");
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.scope, Scope::Range, "De:");
                    let total = self.total.max(1);
                    let from = ui.add(egui::DragValue::new(&mut self.from).range(1..=total));
                    ui.label("Até:");
                    let to = ui.add(egui::DragValue::new(&mut self.to).range(1..=total));
                    if from.changed() || to.changed() {
                        self.scope = Scope::Range;
                    }
                    ui.label(format!("de {total}"));
                });
                ui.separator();
                ui.strong("Numeração");
                ui.radio_value(&mut self.new_section, true, "Iniciar nova seção");
                ui.add_enabled_ui(self.new_section, |ui| {
                    egui::Grid::new("rotulos-secao").show(ui, |ui| {
                        ui.label("Estilo:");
                        egui::ComboBox::from_id_salt("rotulos-estilo")
                            .selected_text(style_label(self.style))
                            .show_ui(ui, |ui| {
                                for s in STYLES {
                                    ui.selectable_value(&mut self.style, s, style_label(s));
                                }
                            });
                        ui.end_row();
                        ui.label("Prefixo:");
                        ui.add(egui::TextEdit::singleline(&mut self.prefix).char_limit(32));
                        ui.end_row();
                        ui.label("Início:");
                        ui.add(egui::DragValue::new(&mut self.start).range(1..=9999));
                        ui.end_row();
                    });
                });
                ui.radio_value(
                    &mut self.new_section,
                    false,
                    "Estender a numeração usada na seção anterior às páginas selecionadas",
                );
                let example = if self.new_section {
                    format_label(self.style, &self.prefix, self.start.max(1))
                } else {
                    "(continua a seção anterior)".into()
                };
                ui.weak(format!("Exemplo: {example}"));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancelar").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        answer = Some(false);
                    }
                });
            });
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_becomes_the_range_and_a_new_section() {
        let mut d = PageLabelsDialog::new(&[4, 2, 3], 10);
        assert_eq!(d.scope, Scope::Selected);
        d.style = PageLabelStyle::LowerRoman;
        let (from, to, new) = d.request();
        assert_eq!((from, to), (2, 4));
        let new = new.unwrap();
        assert_eq!(
            (new.start_page, new.style, new.first),
            (2, PageLabelStyle::LowerRoman, 1)
        );
    }

    #[test]
    fn range_is_ordered_clamped_and_extend_has_no_section() {
        let mut d = PageLabelsDialog::new(&[], 5);
        assert_eq!(d.scope, Scope::All);
        assert_eq!(d.request().0..=d.request().1, 0..=4);
        d.scope = Scope::Range;
        d.from = 9;
        d.to = 2;
        d.new_section = false;
        assert_eq!(d.request(), (1, 4, None));
    }
}
