//! Janela "Assinar com um ID digital" (como no Acrobat): escolher o ID, digitar a senha, motivo e
//! local; criar um ID autoassinado ou importar um arquivo `.pfx`/`.p12`.
use crate::certificates::{save_ids, with_ids};
use crate::open_dialog::{FileDialogs, Picked};
use app_state::certificates::IdEntry;
use pdf_core::{DigitalId, SignPlace};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

/// O que a janela decidiu.
pub enum SignOutcome {
    Sign {
        place: SignPlace,
        id: Box<DigitalId>,
        reason: String,
        location: String,
    },
    Cancel,
}

/// Formulário "Criar ID digital".
#[derive(Default)]
struct NewId {
    name: String,
    email: String,
    org: String,
    password: String,
    confirm: String,
}

pub struct SignDialog {
    place: SignPlace,
    /// Impressão digital do ID escolhido.
    chosen: Option<String>,
    password: String,
    reason: String,
    location: String,
    error: String,
    creating: Option<NewId>,
    /// Criação em andamento (gerar a chave leva um instante).
    job: Option<Receiver<Result<(DigitalId, PathBuf), String>>>,
    dialogs: FileDialogs,
    /// Arquivo importado esperando a senha para ser lido.
    importing: Option<PathBuf>,
}

/// Nome de arquivo seguro para o ID ("Ana Souza" → "Ana Souza.pfx").
fn id_file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.pfx", clean.trim())
}

fn entry(id: &DigitalId, path: PathBuf) -> IdEntry {
    IdEntry {
        path,
        name: id.name.clone(),
        email: id.email.clone(),
        fingerprint: id.fingerprint(),
    }
}

impl SignDialog {
    pub fn new(place: SignPlace, author: Option<&str>) -> Self {
        let chosen = with_ids(|ids| ids.ids.last().map(|e| e.fingerprint.clone()));
        let creating = chosen.is_none().then(|| NewId {
            name: author.unwrap_or_default().trim().to_owned(),
            ..Default::default()
        });
        Self {
            place,
            chosen,
            password: String::new(),
            reason: String::new(),
            location: String::new(),
            error: String::new(),
            creating,
            job: None,
            dialogs: FileDialogs::default(),
            importing: None,
        }
    }

    /// Desenha a janela; devolve a decisão quando houver.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<SignOutcome> {
        self.poll(ctx);
        let mut out = None;
        let mut open = true;
        egui::Window::new("Assinar com um ID digital")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                out = if self.creating.is_some() {
                    self.create_form(ui)
                } else {
                    self.sign_form(ui)
                }
            });
        if !open {
            return Some(SignOutcome::Cancel);
        }
        out
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(Picked::Open(files)) = self.dialogs.poll() {
            self.importing = files.into_iter().next();
            self.error = "Digite a senha do arquivo e clique em Assinar.".into();
        }
        if let Some(rx) = &self.job {
            if let Ok(result) = rx.try_recv() {
                self.job = None;
                match result {
                    Ok((id, path)) => {
                        with_ids(|ids| ids.add(entry(&id, path)));
                        if let Err(e) = save_ids() {
                            self.error = format!("Não foi possível guardar a lista de IDs: {e}");
                        }
                        self.chosen = Some(id.fingerprint());
                        self.creating = None;
                    }
                    Err(e) => self.error = e,
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    fn sign_form(&mut self, ui: &mut egui::Ui) -> Option<SignOutcome> {
        let ids = with_ids(|ids| ids.ids.clone());
        let label = |e: &IdEntry| {
            if e.email.is_empty() {
                e.name.clone()
            } else {
                format!("{} <{}>", e.name, e.email)
            }
        };
        egui::Grid::new("assinar-id").num_columns(2).show(ui, |ui| {
            ui.label("Assinar como");
            let current = match &self.importing {
                Some(p) => p
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                None => ids
                    .iter()
                    .find(|e| Some(&e.fingerprint) == self.chosen.as_ref())
                    .map(label)
                    .unwrap_or_else(|| "Escolha um ID digital".into()),
            };
            egui::ComboBox::from_id_salt("id-digital")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for e in &ids {
                        if ui
                            .selectable_label(
                                self.chosen.as_ref() == Some(&e.fingerprint),
                                label(e),
                            )
                            .clicked()
                        {
                            self.chosen = Some(e.fingerprint.clone());
                            self.importing = None;
                        }
                    }
                });
            ui.end_row();
            ui.label("Senha");
            ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            ui.end_row();
            ui.label("Motivo");
            ui.add(egui::TextEdit::singleline(&mut self.reason).hint_text("(opcional)"));
            ui.end_row();
            ui.label("Local");
            ui.add(egui::TextEdit::singleline(&mut self.location).hint_text("(opcional)"));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button("Criar ID digital…").clicked() {
                self.creating = Some(NewId::default());
                self.error.clear();
            }
            if ui.button("Importar arquivo de ID…").clicked() {
                self.dialogs.pick_id_file(ui.ctx());
            }
        });
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().warn_fg_color, &self.error);
        }
        ui.separator();
        let mut out = None;
        ui.horizontal(|ui| {
            let path = self.importing.clone().or_else(|| {
                ids.iter()
                    .find(|e| Some(&e.fingerprint) == self.chosen.as_ref())
                    .map(|e| e.path.clone())
            });
            if ui
                .add_enabled(
                    path.is_some() && !self.password.is_empty(),
                    egui::Button::new("Assinar"),
                )
                .clicked()
            {
                let path = path.expect("habilitado só com ID");
                match DigitalId::open(&path, &self.password) {
                    Ok(id) => {
                        if self.importing.take().is_some() {
                            with_ids(|ids| ids.add(entry(&id, path)));
                            let _ = save_ids();
                        }
                        self.password.clear();
                        out = Some(SignOutcome::Sign {
                            place: self.place,
                            id: Box::new(id),
                            reason: self.reason.trim().to_owned(),
                            location: self.location.trim().to_owned(),
                        });
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            if ui.button("Cancelar").clicked() {
                out = Some(SignOutcome::Cancel);
            }
        });
        out
    }

    fn create_form(&mut self, ui: &mut egui::Ui) -> Option<SignOutcome> {
        let busy = self.job.is_some();
        let n = self
            .creating
            .as_mut()
            .expect("chamado com o formulário aberto");
        ui.label("Criar um ID digital autoassinado (certificado guardado neste computador).");
        egui::Grid::new("criar-id").num_columns(2).show(ui, |ui| {
            ui.label("Nome");
            ui.text_edit_singleline(&mut n.name);
            ui.end_row();
            ui.label("E-mail");
            ui.text_edit_singleline(&mut n.email);
            ui.end_row();
            ui.label("Organização");
            ui.add(egui::TextEdit::singleline(&mut n.org).hint_text("(opcional)"));
            ui.end_row();
            ui.label("Senha");
            ui.add(egui::TextEdit::singleline(&mut n.password).password(true));
            ui.end_row();
            ui.label("Confirmar senha");
            ui.add(egui::TextEdit::singleline(&mut n.confirm).password(true));
            ui.end_row();
        });
        let ok = !n.name.trim().is_empty() && n.password.len() >= 6 && n.password == n.confirm;
        if !n.password.is_empty() && n.password.len() < 6 {
            ui.label("A senha precisa de pelo menos 6 caracteres.");
        } else if !n.confirm.is_empty() && n.password != n.confirm {
            ui.label("As senhas não conferem.");
        }
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().warn_fg_color, &self.error);
        }
        ui.separator();
        let mut cancel = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ok && !busy, egui::Button::new("Criar"))
                .clicked()
            {
                let (name, email, org, pw) = (
                    n.name.trim().to_owned(),
                    n.email.trim().to_owned(),
                    n.org.trim().to_owned(),
                    std::mem::take(&mut n.password),
                );
                n.confirm.clear();
                let Some(path) =
                    app_state::store::config_file(&format!("ids/{}", id_file_name(&name)))
                else {
                    self.error = "Pasta de configuração indisponível.".into();
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let ctx = ui.ctx().clone();
                let spawned =
                    std::thread::Builder::new()
                        .name("criar-id".into())
                        .spawn(move || {
                            let r = DigitalId::create_self_signed(&name, &email, &org, &pw, &path)
                                .map(|id| (id, path))
                                .map_err(|e| e.to_string());
                            let _ = tx.send(r);
                            ctx.request_repaint();
                        });
                if spawned.is_ok() {
                    self.job = Some(rx);
                    self.error.clear();
                }
            }
            if busy {
                ui.spinner();
                ui.label("Criando…");
            }
            if ui.add_enabled(!busy, egui::Button::new("Voltar")).clicked() {
                cancel = true;
            }
        });
        if cancel {
            self.creating = None;
            self.error.clear();
            if with_ids(|ids| ids.ids.is_empty()) {
                return Some(SignOutcome::Cancel);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_files_get_safe_names() {
        assert_eq!(id_file_name("Ana Souza"), "Ana Souza.pfx");
        assert_eq!(id_file_name("../etc/x"), "___etc_x.pfx");
    }
}
