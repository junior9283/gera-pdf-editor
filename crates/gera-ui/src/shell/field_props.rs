//! Janela "Propriedades do campo" do Preparar formulário (como no Acrobat): nome, dica,
//! obrigatório, somente leitura e as opções de cada tipo.
use pdf_core::{FieldKind, FieldProps};

/// Que partes da janela o campo usa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Button { radio: bool },
    Choice { combo: bool },
    Other,
}

pub struct FieldPropsWindow {
    pub page: i32,
    pub id: i32,
    kind: Kind,
    props: FieldProps,
    /// Texto do limite de caracteres ("" = sem limite).
    max_len: String,
    /// Opção nova sendo digitada: (valor de exportação, rótulo).
    new_option: (String, String),
}

impl FieldPropsWindow {
    pub fn new(page: i32, id: i32, kind: &FieldKind, props: FieldProps) -> Self {
        let kind = match kind {
            FieldKind::Text { .. } => Kind::Text,
            FieldKind::Checkbox { .. } => Kind::Button { radio: false },
            FieldKind::Radio { .. } => Kind::Button { radio: true },
            FieldKind::Combo { .. } => Kind::Choice { combo: true },
            FieldKind::List { .. } => Kind::Choice { combo: false },
            FieldKind::Signature | FieldKind::Button => Kind::Other,
        };
        Self {
            page,
            id,
            kind,
            max_len: props.max_len.map_or_else(String::new, |n| n.to_string()),
            props,
            new_option: Default::default(),
        }
    }

    /// Desenha a janela. `Some(Some(_))`: OK com as propriedades; `Some(None)`: cancelou.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<FieldProps>> {
        let mut out = None;
        let mut open = true;
        egui::Window::new("Propriedades do campo")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| out = self.contents(ui));
        if !open {
            return Some(None);
        }
        out
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> Option<Option<FieldProps>> {
        let p = &mut self.props;
        egui::Grid::new("propriedades-campo")
            .num_columns(2)
            .show(ui, |ui| {
                let name_label = if matches!(self.kind, Kind::Button { radio: true }) {
                    "Nome do grupo"
                } else {
                    "Nome"
                };
                ui.label(name_label);
                ui.text_edit_singleline(&mut p.name);
                ui.end_row();
                ui.label("Dica");
                ui.text_edit_singleline(&mut p.tooltip);
                ui.end_row();
            });
        ui.horizontal(|ui| {
            ui.checkbox(&mut p.required, "Obrigatório");
            ui.checkbox(&mut p.readonly, "Somente leitura");
        });
        ui.separator();
        match self.kind {
            Kind::Text => {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut p.multiline, "Várias linhas");
                    ui.checkbox(&mut p.password, "Senha");
                });
                egui::Grid::new("texto-campo")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Limite de caracteres");
                        ui.add(egui::TextEdit::singleline(&mut self.max_len).desired_width(60.0));
                        ui.end_row();
                        ui.label("Valor padrão");
                        ui.text_edit_singleline(&mut p.default);
                        ui.end_row();
                    });
            }
            Kind::Button { .. } => {
                egui::Grid::new("botao-campo")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Valor de exportação");
                        ui.text_edit_singleline(&mut p.export);
                        ui.end_row();
                    });
                ui.checkbox(&mut p.checked_default, "Marcado por padrão");
            }
            Kind::Choice { combo } => {
                if combo {
                    ui.checkbox(&mut p.editable, "Aceitar texto digitado");
                }
                options_editor(ui, &mut p.options, &mut self.new_option);
            }
            Kind::Other => {}
        }
        if self.kind != Kind::Other && !matches!(self.kind, Kind::Button { .. }) {
            ui.horizontal(|ui| {
                ui.label("Tamanho da fonte");
                ui.add(egui::DragValue::new(&mut p.font_size).range(0.0..=72.0))
                    .on_hover_text("0 = automático");
            });
        }
        ui.separator();
        let mut out = None;
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                let mut props = self.props.clone();
                props.max_len = self.max_len.trim().parse().ok().filter(|&n: &usize| n > 0);
                out = Some(Some(props));
            }
            if ui.button("Cancelar").clicked() {
                out = Some(None);
            }
        });
        out
    }
}

/// Opções da lista: rótulo e valor de exportação, com adicionar, remover, subir e descer.
fn options_editor(
    ui: &mut egui::Ui,
    options: &mut Vec<(String, String)>,
    new: &mut (String, String),
) {
    ui.label("Opções");
    let mut action: Option<(usize, i8)> = None; // (linha, -1 sobe / 1 desce / 0 remove)
    let n = options.len();
    egui::Grid::new("opcoes-campo")
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Rótulo");
            ui.strong("Valor de exportação");
            ui.label("");
            ui.end_row();
            for (i, (export, label)) in options.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(label).desired_width(140.0));
                ui.add(egui::TextEdit::singleline(export).desired_width(110.0));
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("▲")).clicked() {
                        action = Some((i, -1));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("▼")).clicked() {
                        action = Some((i, 1));
                    }
                    if ui.button("Remover").clicked() {
                        action = Some((i, 0));
                    }
                });
                ui.end_row();
            }
            ui.add(
                egui::TextEdit::singleline(&mut new.1)
                    .hint_text("Nova opção")
                    .desired_width(140.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut new.0)
                    .hint_text("(igual ao rótulo)")
                    .desired_width(110.0),
            );
            if ui
                .add_enabled(!new.1.trim().is_empty(), egui::Button::new("Adicionar"))
                .clicked()
            {
                options.push(added(std::mem::take(new)));
            }
            ui.end_row();
        });
    match action {
        Some((i, 0)) => {
            options.remove(i);
        }
        Some((i, -1)) => options.swap(i, i - 1),
        Some((i, _)) => options.swap(i, i + 1),
        None => {}
    }
}

/// Opção nova: sem valor de exportação, vale o rótulo (como no Acrobat).
fn added((export, label): (String, String)) -> (String, String) {
    let label = label.trim().to_owned();
    let export = export.trim();
    let export = if export.is_empty() {
        label.clone()
    } else {
        export.to_owned()
    };
    (export, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_option_without_export_value_uses_its_label() {
        assert_eq!(
            added((" ".into(), " Azul ".into())),
            ("Azul".into(), "Azul".into())
        );
        assert_eq!(
            added(("az".into(), "Azul".into())),
            ("az".into(), "Azul".into())
        );
    }
}
