//! Aba Ações (como a do Acrobat): escolher o gatilho e a ação, e a lista de ações por gatilho, com
//! Editar, Excluir, Subir e Descer. Serve aos campos (Mouse para cima…) e às páginas (Abrir e
//! Fechar página).
use pdf_core::{ActionKind, FieldTrigger, PageTrigger, NAMED_ITEMS};

/// Um gatilho (de campo ou de página).
pub trait Trigger: Copy + PartialEq {
    fn label(self) -> &'static str;
}

impl Trigger for FieldTrigger {
    fn label(self) -> &'static str {
        FieldTrigger::label(self)
    }
}

impl Trigger for PageTrigger {
    fn label(self) -> &'static str {
        PageTrigger::label(self)
    }
}

/// As ações que dá para acrescentar (o menu "Selecionar ação" do Acrobat, menos as que o app
/// ainda não executa).
const NEW_ACTIONS: [&str; 8] = [
    "Ir para uma página",
    "Abrir um link da web",
    "Mostrar/ocultar um campo",
    "Redefinir um formulário",
    "Executar um item de menu",
    "Abrir um arquivo",
    "Executar um JavaScript",
    "Enviar um formulário",
];

fn new_action(k: usize) -> ActionKind {
    match k {
        0 => ActionKind::GoTo { page: 0 },
        1 => ActionKind::Uri("https://".into()),
        2 => ActionKind::ShowHide {
            fields: Vec::new(),
            hide: true,
        },
        3 => ActionKind::Reset {
            fields: Vec::new(),
            exclude: false,
        },
        4 => ActionKind::Named(NAMED_ITEMS[0].0.into()),
        5 => ActionKind::Launch(String::new()),
        6 => ActionKind::JavaScript(String::new()),
        _ => ActionKind::Submit {
            url: "https://".into(),
            format: pdf_core::SubmitFormat::Fdf,
        },
    }
}

pub struct ActionsEditor<T> {
    pub list: Vec<(T, ActionKind)>,
    triggers: Vec<T>,
    trigger: T,
    new: usize,
    selected: Option<usize>,
    /// Ação em edição: posição na lista (`None` = nova) e o rascunho.
    editing: Option<(Option<usize>, ActionKind)>,
}

impl<T: Trigger> ActionsEditor<T> {
    pub fn new(list: Vec<(T, ActionKind)>, triggers: &[T]) -> Self {
        Self {
            list,
            triggers: triggers.to_vec(),
            trigger: triggers[0],
            new: 0,
            selected: None,
            editing: None,
        }
    }

    /// Acrescenta a ação no fim das do gatilho (a lista fica agrupada na ordem dos gatilhos).
    pub fn add(&mut self, trigger: T, action: ActionKind) {
        let rank = |t: &T| self.triggers.iter().position(|x| x == t).unwrap_or(0);
        let at = self
            .list
            .iter()
            .rposition(|(t, _)| rank(t) <= rank(&trigger))
            .map_or(0, |k| k + 1);
        self.list.insert(at, (trigger, action));
    }

    /// Sobe ou desce a ação dentro do seu gatilho.
    pub fn shift(&mut self, k: usize, up: bool) -> Option<usize> {
        let other = if up { k.checked_sub(1)? } else { k + 1 };
        let same = self.list.get(other)?.0 == self.list.get(k)?.0;
        same.then(|| {
            self.list.swap(k, other);
            other
        })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, fields: &[String], pages: usize) {
        ui.horizontal(|ui| {
            ui.label("Selecionar gatilho:");
            egui::ComboBox::from_id_salt("gatilho")
                .selected_text(self.trigger.label())
                .show_ui(ui, |ui| {
                    for t in self.triggers.clone() {
                        ui.selectable_value(&mut self.trigger, t, t.label());
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Selecionar ação:");
            egui::ComboBox::from_id_salt("acao")
                .selected_text(NEW_ACTIONS[self.new])
                .show_ui(ui, |ui| {
                    for (k, label) in NEW_ACTIONS.iter().enumerate() {
                        ui.selectable_value(&mut self.new, k, *label);
                    }
                });
            if ui.button("Adicionar…").clicked() {
                self.editing = Some((None, new_action(self.new)));
            }
        });
        ui.label("Ações:");
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(360.0);
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    if self.list.is_empty() {
                        ui.weak("Nenhuma ação.");
                    }
                    let mut last = None;
                    for (k, (t, a)) in self.list.iter().enumerate() {
                        if last != Some(*t) {
                            ui.strong(t.label());
                            last = Some(*t);
                        }
                        let label = crate::links::safe_display(&a.label());
                        if ui
                            .selectable_label(self.selected == Some(k), format!("   {label}"))
                            .clicked()
                        {
                            self.selected = Some(k);
                        }
                    }
                });
        });
        let sel = self.selected.filter(|k| *k < self.list.len());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Subir"))
                .clicked()
            {
                self.selected = sel.and_then(|k| self.shift(k, true)).or(sel);
            }
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Descer"))
                .clicked()
            {
                self.selected = sel.and_then(|k| self.shift(k, false)).or(sel);
            }
            let editable = sel.is_some_and(|k| !matches!(self.list[k].1, ActionKind::Other { .. }));
            if ui
                .add_enabled(editable, egui::Button::new("Editar…"))
                .clicked()
            {
                if let Some(k) = sel {
                    self.editing = Some((Some(k), self.list[k].1.clone()));
                }
            }
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Excluir"))
                .clicked()
            {
                if let Some(k) = sel {
                    self.list.remove(k);
                    self.selected = None;
                }
            }
        });
        if let Some((at, mut draft)) = self.editing.take() {
            let mut done = None;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                edit_action(ui, &mut draft, fields, pages);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        done = Some(false);
                    }
                });
            });
            match (done, at) {
                (Some(true), Some(k)) => self.list[k].1 = draft,
                (Some(true), None) => self.add(self.trigger, draft),
                (Some(false), _) => {}
                (None, _) => self.editing = Some((at, draft)),
            }
        }
    }
}

/// O editor de uma ação (o que muda conforme o tipo).
fn edit_action(ui: &mut egui::Ui, a: &mut ActionKind, fields: &[String], pages: usize) {
    let pick = |ui: &mut egui::Ui, list: &mut Vec<String>| {
        crate::shell::field_props::field_checklist(ui, "campos-da-acao", 110.0, fields, list)
    };
    match a {
        ActionKind::GoTo { page } => {
            ui.horizontal(|ui| {
                ui.label("Página:");
                let mut n = *page + 1;
                ui.add(egui::DragValue::new(&mut n).range(1..=pages.max(1) as i32));
                *page = n - 1;
            });
        }
        ActionKind::Uri(u) => {
            ui.label("Endereço (http, https ou mailto):");
            ui.text_edit_singleline(u);
        }
        ActionKind::ShowHide { fields: list, hide } => {
            ui.horizontal(|ui| {
                ui.radio_value(hide, false, "Mostrar");
                ui.radio_value(hide, true, "Ocultar");
            });
            pick(ui, list);
        }
        ActionKind::Reset {
            fields: list,
            exclude,
        } => {
            ui.label("Sem campos marcados, redefine o formulário inteiro.");
            pick(ui, list);
            ui.checkbox(exclude, "Todos os campos, menos os marcados");
        }
        ActionKind::Named(n) => {
            egui::ComboBox::from_id_salt("item-de-menu")
                .selected_text(pdf_core::named_label(n))
                .show_ui(ui, |ui| {
                    for (key, label) in NAMED_ITEMS {
                        if ui.selectable_label(n == key, label).clicked() {
                            *n = key.to_string();
                        }
                    }
                });
        }
        ActionKind::Launch(f) => {
            ui.label("Arquivo PDF (caminho relativo a este documento):");
            ui.text_edit_singleline(f);
        }
        ActionKind::JavaScript(js) => {
            ui.label("O script é gravado no arquivo, mas o app não o executa.");
            ui.add(egui::TextEdit::multiline(js).desired_rows(4).code_editor());
        }
        ActionKind::Submit { url, format } => {
            ui.label("Endereço (http, https ou mailto:):");
            ui.text_edit_singleline(url);
            if !url.trim().is_empty() && pdf_core::submit_target(url).is_none() {
                ui.colored_label(ui.visuals().warn_fg_color, "Endereço não aceito.");
            }
            ui.horizontal(|ui| {
                ui.label("Formato:");
                egui::ComboBox::from_id_salt("formato-do-envio")
                    .selected_text(format.label())
                    .show_ui(ui, |ui| {
                        for f in pdf_core::SubmitFormat::ALL {
                            ui.selectable_value(format, f, f.label());
                        }
                    });
            });
        }
        ActionKind::ImportIcon => {
            ui.label("Escolher a imagem do botão (campo Imagem).");
        }
        ActionKind::Other { kind, .. } => {
            ui.label(format!("Ação «{kind}» do arquivo, mantida como está."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_is_offered() {
        let k = NEW_ACTIONS
            .iter()
            .position(|l| *l == "Enviar um formulário")
            .unwrap();
        assert!(matches!(new_action(k), ActionKind::Submit { .. }));
    }

    #[test]
    fn actions_tab_builds_the_props() {
        use FieldTrigger as T;
        let mut e = ActionsEditor::new(Vec::new(), &T::ALL);
        e.add(T::Focus, ActionKind::Named("Print".into()));
        e.add(T::MouseUp, ActionKind::GoTo { page: 2 });
        e.add(T::MouseUp, ActionKind::Uri("https://a.b".into()));
        // agrupadas na ordem dos gatilhos: Mouse para cima antes de Em foco
        let triggers: Vec<T> = e.list.iter().map(|(t, _)| *t).collect();
        assert_eq!(triggers, [T::MouseUp, T::MouseUp, T::Focus]);
        assert_eq!(e.shift(1, true), Some(0));
        assert_eq!(e.list[0].1, ActionKind::Uri("https://a.b".into()));
        assert_eq!(e.shift(1, false), None, "não passa para outro gatilho");
    }
}
