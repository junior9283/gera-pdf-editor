//! Janela "Proteger usando senha" (como no Acrobat): senha para abrir, senha de permissões e as
//! restrições de impressão, alterações e cópia.
use pdf_core::{Permissions, Protection};

/// "Alterações permitidas" do Acrobat.
const CHANGES: [&str; 4] = [
    "Nenhuma",
    "Preencher campos de formulário e assinar",
    "Comentar, preencher campos e assinar",
    "Qualquer uma, exceto extrair páginas",
];

const PRINTING: [&str; 3] = ["Não permitida", "Baixa resolução", "Alta resolução"];

#[derive(Default)]
pub struct ProtectDialog {
    use_open: bool,
    open: String,
    open_confirm: String,
    use_owner: bool,
    owner: String,
    owner_confirm: String,
    printing: usize,
    changes: usize,
    copy: bool,
}

/// Permissões das escolhas da janela.
pub fn permissions(printing: usize, changes: usize, copy: bool) -> Permissions {
    Permissions {
        print: printing > 0,
        print_hq: printing > 1,
        copy,
        modify: changes >= 3,
        annotate: changes >= 2,
        forms: changes >= 1,
        assemble: changes >= 3,
    }
}

impl ProtectDialog {
    pub fn new() -> Self {
        Self {
            printing: 2,
            changes: 3,
            copy: true,
            ..Default::default()
        }
    }

    /// O que está errado nas escolhas (ou `None`, pronto para OK).
    fn problem(&self) -> Option<&'static str> {
        if !self.use_open && !self.use_owner {
            return Some("Escolha ao menos uma das senhas.");
        }
        if self.use_open && (self.open.is_empty() || self.open != self.open_confirm) {
            return Some("Confirme a senha para abrir.");
        }
        if self.use_owner && (self.owner.is_empty() || self.owner != self.owner_confirm) {
            return Some("Confirme a senha de permissões.");
        }
        if self.use_open && self.use_owner && self.open == self.owner {
            return Some("A senha de permissões tem de ser diferente da senha para abrir.");
        }
        None
    }

    fn protection(&self) -> Protection {
        let permissions = if self.use_owner {
            permissions(self.printing, self.changes, self.copy)
        } else {
            Permissions::ALL
        };
        Protection {
            open_password: if self.use_open {
                self.open.clone()
            } else {
                String::new()
            },
            owner_password: if self.use_owner {
                self.owner.clone()
            } else {
                String::new()
            },
            permissions,
        }
    }

    /// Desenha a janela. `Some(Some(_))`: OK; `Some(None)`: cancelou.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<Protection>> {
        let mut out = None;
        let mut open = true;
        egui::Window::new("Proteger usando senha")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.use_open, "Exigir senha para abrir o documento");
                ui.add_enabled_ui(self.use_open, |ui| {
                    egui::Grid::new("senha-abrir")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Senha");
                            ui.add(egui::TextEdit::singleline(&mut self.open).password(true));
                            ui.end_row();
                            ui.label("Confirmar");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.open_confirm).password(true),
                            );
                            ui.end_row();
                        });
                });
                ui.separator();
                ui.checkbox(
                    &mut self.use_owner,
                    "Restringir edição e impressão (senha de permissões)",
                );
                ui.add_enabled_ui(self.use_owner, |ui| {
                    egui::Grid::new("senha-dono").num_columns(2).show(ui, |ui| {
                        ui.label("Senha de permissões");
                        ui.add(egui::TextEdit::singleline(&mut self.owner).password(true));
                        ui.end_row();
                        ui.label("Confirmar");
                        ui.add(egui::TextEdit::singleline(&mut self.owner_confirm).password(true));
                        ui.end_row();
                        ui.label("Impressão permitida");
                        egui::ComboBox::from_id_salt("impressao")
                            .selected_text(PRINTING[self.printing])
                            .show_index(ui, &mut self.printing, PRINTING.len(), |i| PRINTING[i]);
                        ui.end_row();
                        ui.label("Alterações permitidas");
                        egui::ComboBox::from_id_salt("alteracoes")
                            .selected_text(CHANGES[self.changes])
                            .width(280.0)
                            .show_index(ui, &mut self.changes, CHANGES.len(), |i| CHANGES[i]);
                        ui.end_row();
                    });
                    ui.checkbox(
                        &mut self.copy,
                        "Permitir copiar texto, imagens e outros conteúdos",
                    );
                });
                ui.separator();
                ui.label(
                    "Criptografia AES de 256 bits (compatível com o Acrobat 9 e posteriores).",
                );
                ui.label("A proteção é aplicada quando o documento for salvo.");
                let problem = self.problem();
                if let Some(p) = problem {
                    ui.colored_label(ui.visuals().weak_text_color(), p);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(problem.is_none(), egui::Button::new("OK"))
                        .clicked()
                    {
                        out = Some(Some(self.protection()));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_map_to_acrobat_permissions() {
        let none = permissions(0, 0, false);
        assert!(!none.print && !none.copy && !none.forms && !none.annotate && !none.modify);
        let fill = permissions(1, 1, true);
        assert!(fill.print && !fill.print_hq && fill.forms && !fill.annotate && fill.copy);
        let all_but_extract = permissions(2, 3, false);
        assert!(all_but_extract.modify && all_but_extract.assemble && !all_but_extract.copy);
    }

    #[test]
    fn passwords_must_be_confirmed_and_distinct() {
        let mut d = ProtectDialog::new();
        assert!(d.problem().is_some());
        d.use_open = true;
        d.open = "a".into();
        assert!(d.problem().is_some(), "sem confirmar");
        d.open_confirm = "a".into();
        assert!(d.problem().is_none());
        d.use_owner = true;
        d.owner = "a".into();
        d.owner_confirm = "a".into();
        assert!(d.problem().is_some(), "iguais");
        d.owner = "b".into();
        d.owner_confirm = "b".into();
        assert!(d.problem().is_none());
        assert_eq!(d.protection().open_password, "a");
    }
}
