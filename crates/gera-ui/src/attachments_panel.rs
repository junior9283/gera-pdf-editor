//! Painel Anexos (como no Acrobat): lista, Abrir (só PDF, com aviso), Salvar, Adicionar, Excluir
//! e Descrição.
use pdf_core::{AttachmentInfo, AttachmentOrigin};

/// Só anexos PDF abrem no app; o resto só pode ser salvo (o app não roda outros programas).
pub fn opens_in_app(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Nome sugerido ao salvar: só o nome final, sem caminho nem caracteres proibidos.
pub fn save_name(name: &str) -> String {
    let last: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|&c| !crate::links::hidden_mark(c))
        .collect();
    let last = last.trim().trim_start_matches('.');
    if last.is_empty() {
        return "anexo".into();
    }
    pdf_core::safe_file_name(last)
}

/// Nome original do anexo a partir da cópia na pasta privada (None se não for uma cópia).
pub fn spool_name(path: &std::path::Path) -> Option<String> {
    let file = path
        .file_name()?
        .to_str()?
        .strip_prefix(crate::print_job::ATTACHMENT_PREFIX)?;
    let mut parts = file.splitn(3, '-');
    parts.next()?.parse::<u32>().ok()?;
    parts.next()?.parse::<u32>().ok()?;
    parts.next().filter(|n| !n.is_empty()).map(String::from)
}

/// Linha da lista: nome (sem marcas de direção, que disfarçariam a extensão), tamanho e página.
fn row_label(a: &AttachmentInfo) -> String {
    let place = match a.origin {
        AttachmentOrigin::Document => String::new(),
        AttachmentOrigin::Comment { page, .. } => format!(" · p. {}", page + 1),
    };
    format!(
        "📎 {}  ({}{place})",
        crate::links::safe_display(&a.name),
        crate::shell::properties::size_text(a.size as u64)
    )
}

/// O que o painel pede à aba.
#[derive(Debug, Clone, PartialEq)]
pub enum AttachmentAction {
    Open(AttachmentInfo),
    Save(AttachmentInfo),
    Add,
    Remove(String),
    Describe {
        name: String,
        text: String,
    },
    /// Anexo de comentário: ir até ele.
    Go {
        page: i32,
    },
}

#[derive(Debug, Default)]
pub struct AttachmentsPanel {
    selected: Option<usize>,
    /// Descrição em edição: (índice, texto).
    editing: Option<(usize, String)>,
}

impl AttachmentsPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        list: &[AttachmentInfo],
        can_edit: bool,
    ) -> Vec<AttachmentAction> {
        let mut out = Vec::new();
        let sel = self.selected.and_then(|k| list.get(k));
        let non_pdf = sel.is_some_and(|a| !opens_in_app(&a.name));
        ui.horizontal_wrapped(|ui| {
            if !non_pdf
                && ui
                    .add_enabled(sel.is_some(), egui::Button::new("Abrir"))
                    .clicked()
            {
                out.extend(sel.cloned().map(AttachmentAction::Open));
            }
            if ui
                .add_enabled(sel.is_some(), egui::Button::new("Salvar…"))
                .clicked()
            {
                out.extend(sel.cloned().map(AttachmentAction::Save));
            }
            if ui
                .add_enabled(can_edit, egui::Button::new("Adicionar arquivo…"))
                .clicked()
            {
                out.push(AttachmentAction::Add);
            }
            let removable = sel.filter(|a| a.origin == AttachmentOrigin::Document && can_edit);
            if ui
                .add_enabled(removable.is_some(), egui::Button::new("Excluir"))
                .clicked()
            {
                out.extend(removable.map(|a| AttachmentAction::Remove(a.name.clone())));
                self.selected = None;
            }
        });
        if non_pdf {
            ui.weak("Por segurança, só anexos PDF abrem aqui; salve o arquivo para abri-lo.");
        }
        ui.separator();
        if list.is_empty() {
            ui.weak("Este documento não tem anexos.");
            return out;
        }
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for (k, a) in list.iter().enumerate() {
                    let row = ui.add(egui::Button::selectable(
                        self.selected == Some(k),
                        row_label(a),
                    ));
                    if row.clicked() {
                        self.selected = Some(k);
                        if let AttachmentOrigin::Comment { page, .. } = a.origin {
                            out.push(AttachmentAction::Go { page });
                        }
                    }
                    if row.double_clicked() && opens_in_app(&a.name) {
                        out.push(AttachmentAction::Open(a.clone()));
                    }
                    // descrição: clique duplo nela para editar (só os do documento)
                    let editable = can_edit && a.origin == AttachmentOrigin::Document;
                    match self.editing.as_mut().filter(|(i, _)| *i == k) {
                        Some((_, draft)) => {
                            let edit = ui.text_edit_singleline(draft);
                            edit.request_focus();
                            if edit.lost_focus() {
                                if ui.input(|i| !i.key_pressed(egui::Key::Escape)) {
                                    out.push(AttachmentAction::Describe {
                                        name: a.name.clone(),
                                        text: draft.trim().to_owned(),
                                    });
                                }
                                self.editing = None;
                            }
                        }
                        None => {
                            let shown = if a.description.is_empty() {
                                "(sem descrição)".to_owned()
                            } else {
                                crate::links::safe_display(&a.description)
                            };
                            let desc = ui.add(
                                egui::Label::new(egui::RichText::new(shown).weak().italics())
                                    .sense(egui::Sense::click()),
                            );
                            if desc.double_clicked() && editable {
                                self.editing = Some((k, a.description.clone()));
                            }
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pdf_attachments_open() {
        assert!(opens_in_app("relatorio.PDF"));
        assert!(opens_in_app("a.pdf"));
        assert!(!opens_in_app("planilha.xlsx"));
        assert!(!opens_in_app("programa.exe"));
        assert!(!opens_in_app("sem-extensao"));
        assert!(!opens_in_app("truque.pdf.exe"));
    }

    #[test]
    fn save_name_is_sanitized() {
        assert_eq!(save_name("../../etc/passwd"), "passwd");
        assert_eq!(save_name("/etc/shadow"), "shadow");
        assert_eq!(save_name("pasta\\\\nota.txt"), "nota.txt");
        assert_eq!(save_name("a:b*c?.pdf"), "a_b_c_.pdf");
        assert_eq!(save_name(""), "anexo");
        assert_eq!(save_name(".."), "anexo");
        assert_eq!(save_name(".bashrc"), "bashrc", "nada de arquivo oculto");
        assert_eq!(
            save_name("fatura\u{202E}fdp.exe"),
            "faturafdp.exe",
            "sem marca de direção"
        );
    }

    #[test]
    fn direction_marks_cannot_disguise_the_extension() {
        let a = AttachmentInfo {
            name: "fatura\u{202E}fdp.exe".into(),
            description: "ok\u{202E}".into(),
            size: 10,
            origin: AttachmentOrigin::Document,
        };
        let label = row_label(&a);
        assert!(!label.contains('\u{202E}'), "{label}");
        assert!(label.contains("exe"), "{label}");
    }

    #[test]
    fn the_spool_copy_gives_back_the_attachment_name() {
        let p = std::path::Path::new("/run/x/gera-anexo-123-4-meu-relatorio.pdf");
        assert_eq!(spool_name(p).as_deref(), Some("meu-relatorio.pdf"));
        assert_eq!(spool_name(std::path::Path::new("/home/a/outro.pdf")), None);
    }
}
