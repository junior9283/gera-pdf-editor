//! Barra de ferramentas do documento e a linha de ferramentas de comentário.
use crate::doc_tab::{DocCommand, DocTab};
use crate::fill_sign::{paint_saved, store_saved, with_saved};
use crate::shell::annot_props::color32;
use crate::shell::commands::{hover, Command};
use app_state::fill_sign::{self as fs, FillTool};
use app_state::stamps::StampKind;
use app_state::tools::Tool;
use app_state::tools_catalog;
use app_state::zoom::Fit;
use pdf_core::{AnnotKind, AnnotOp, Mark};

pub fn show(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        button(ui, &mut out, "📂", Command::Open, true);
        let save = if doc.is_dirty() { "💾•" } else { "💾" };
        let can_save = !doc.edit.saving;
        button(ui, &mut out, save, Command::Doc(DocCommand::Save), can_save);
        button(ui, &mut out, "⎙", Command::Print, true);
        button(ui, &mut out, "🔍", Command::Doc(DocCommand::Find), true);
        ui.separator();
        doc.page_controls(ui);
        ui.separator();
        button(ui, &mut out, "−", Command::Doc(DocCommand::ZoomOut), true);
        if let Some(pct) = doc.zoom_percent() {
            ui.label(format!("{pct:.0}%"));
        }
        button(ui, &mut out, "+", Command::Doc(DocCommand::ZoomIn), true);
        ui.menu_button("⏷", |ui| {
            for cmd in [
                DocCommand::Fit(Fit::Width),
                DocCommand::Fit(Fit::Page),
                DocCommand::ZoomReset,
            ] {
                let cmd = Command::Doc(cmd);
                if ui.button(cmd.label()).clicked() {
                    out = Some(cmd);
                }
            }
        });
        ui.separator();
        doc.search_box(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // uma linha de ferramentas por vez; fechar a linha volta a Selecionar
            for (open, label, id) in [
                (doc.fill_tools, "🖊 Assinar", tools_catalog::FILL_SIGN),
                (doc.comment_tools, "✏ Comentar", tools_catalog::COMMENT),
            ] {
                if ui.add(egui::Button::selectable(open, label)).clicked() {
                    doc.show_row(if open { "" } else { id });
                }
            }
        });
    });
    out
}

/// Linha Proteger PDF: Proteger usando senha, Remover segurança, Desbloquear e Fechar.
pub fn protect_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let sec = doc.security;
    let locked = sec.locked();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!locked, egui::Button::new("🔒 Proteger usando senha…"))
            .on_disabled_hover_text("Desbloqueie com a senha de permissões primeiro")
            .clicked()
        {
            doc.protect = Some(crate::shell::protect_dialog::ProtectDialog::new());
        }
        if ui
            .add_enabled(
                sec.encrypted && !locked,
                egui::Button::new("Remover segurança"),
            )
            .clicked()
        {
            doc.remove_security();
        }
        if ui
            .add_enabled(locked, egui::Button::new("Desbloquear…"))
            .on_hover_text("Informar a senha de permissões")
            .clicked()
        {
            doc.unlock = Some(String::new());
        }
        ui.label(match (sec.encrypted, locked) {
            (false, _) => "Documento sem proteção.",
            (true, true) => "Protegido: algumas ações não são permitidas.",
            (true, false) => "Protegido por senha.",
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Fechar").clicked() {
                doc.show_row("");
            }
        });
    });
    None
}

/// Linha Certificados: Assinar digitalmente, Validar todas as assinaturas e Fechar.
pub fn cert_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::selectable(
                doc.cert.placing,
                "✒ Assinar digitalmente",
            ))
            .on_hover_text("Arraste um retângulo na página ou clique num campo de assinatura")
            .clicked()
        {
            doc.cert.placing = !doc.cert.placing;
        }
        if ui.button("Validar todas as assinaturas").clicked() {
            doc.validate_signatures();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Fechar").clicked() {
                doc.show_row("");
            }
        });
    });
    None
}

/// Linha do Preparar formulário: Selecionar, os tipos de campo, desfazer/refazer e Fechar.
pub fn prepare_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        let current = doc.prepare.tool;
        if ui
            .add(egui::Button::selectable(current.is_none(), "Selecionar"))
            .clicked()
        {
            doc.prepare.set_tool(None);
        }
        ui.separator();
        for kind in app_state::prepare::KINDS {
            let active = current == Some(kind);
            if ui
                .add(egui::Button::selectable(
                    active,
                    app_state::prepare::label(kind),
                ))
                .on_hover_text("Clique na página para colocar, ou arraste para escolher o tamanho")
                .clicked()
            {
                doc.prepare.set_tool((!active).then_some(kind));
            }
        }
        ui.separator();
        let can_edit = !doc.edit.saving;
        let can_undo = can_edit && doc.is_dirty();
        button(ui, &mut out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
        button(ui, &mut out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Fechar").clicked() {
                doc.show_row("");
            }
        });
    });
    out
}

/// Linha do Editar PDF: acrescentar, girar, substituir e excluir imagens, desfazer/refazer e
/// Fechar.
pub fn edit_pdf_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        let placing = doc.edit_pdf.placing.is_some();
        if ui
            .add(egui::Button::selectable(placing, "🖼 Adicionar imagem"))
            .on_hover_text("Escolha a imagem e clique na página para colocá-la")
            .clicked()
        {
            if placing {
                doc.edit_pdf.placing = None;
            } else {
                doc.pick_image(ui.ctx(), false);
            }
        }
        ui.separator();
        let selected = doc.edit_pdf.selected.is_some();
        ui.add_enabled_ui(selected, |ui| {
            if ui
                .button("⟲")
                .on_hover_text("Girar no sentido anti-horário")
                .clicked()
            {
                doc.rotate_image(false);
            }
            if ui
                .button("⟳")
                .on_hover_text("Girar no sentido horário")
                .clicked()
            {
                doc.rotate_image(true);
            }
            if ui.button("Substituir imagem…").clicked() {
                doc.pick_image(ui.ctx(), true);
            }
        });
        button(
            ui,
            &mut out,
            "🗑",
            Command::Doc(DocCommand::Delete),
            selected,
        );
        ui.separator();
        let can_edit = !doc.edit.saving;
        let can_undo = can_edit && doc.is_dirty();
        button(ui, &mut out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
        button(ui, &mut out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Fechar").clicked() {
                doc.show_row("");
            }
        });
    });
    out
}

/// Linha do Preencher e assinar: texto, tamanho, marcas, assinatura e iniciais (como no Acrobat).
pub fn fill_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    if doc.edit.tool != Tool::Fill {
        doc.edit.fill.tool = None; // Esc ou outra ferramenta
    }
    ui.horizontal(|ui| {
        let current = doc.edit.fill.tool;
        let mut pick = |ui: &mut egui::Ui, label: &str, tip: &str, tool: FillTool| {
            let active = current == Some(tool);
            if ui
                .add(egui::Button::selectable(active, label))
                .on_hover_text(tip)
                .clicked()
            {
                doc.edit.set_fill_tool((!active).then_some(tool));
            }
        };
        pick(
            ui,
            "Ab Adicionar texto",
            "Clique onde o texto deve começar",
            FillTool::Text,
        );
        for (label, tip, mark) in [
            ("✖", "Xis", Mark::Cross),
            ("✔", "Visto", Mark::Check),
            ("○", "Círculo", Mark::Circle),
            ("—", "Linha", Mark::Line),
            ("•", "Ponto", Mark::Dot),
        ] {
            pick(ui, label, tip, FillTool::Mark(mark));
        }
        ui.separator();
        text_size_buttons(ui, doc);
        ui.separator();
        sign_menu(ui, doc);
        ui.separator();
        let can_edit = !doc.edit.saving;
        let can_undo = can_edit && doc.is_dirty();
        button(ui, &mut out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
        button(ui, &mut out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
    });
    out
}

/// A−/A+: muda o texto selecionado (e o tamanho dos próximos), em passos de 2 pt.
fn text_size_buttons(ui: &mut egui::Ui, doc: &mut DocTab) {
    for (label, tip, grow) in [
        ("A−", "Diminuir texto", false),
        ("A+", "Aumentar texto", true),
    ] {
        if !ui.button(label).on_hover_text(tip).clicked() {
            continue;
        }
        let selected = doc.edit.selected.and_then(|(page, id)| {
            doc.annots
                .iter()
                .find(|a| a.page == page && a.id == id && a.kind == AnnotKind::TextBox)
        });
        match selected {
            Some(a) => {
                let (size, color) = a.font.unwrap_or((fs::TEXT_SIZE, [0.0; 3]));
                let size = fs::step_text_size(size, grow);
                doc.edit.fill.text_size = size;
                doc.edit.push_op(AnnotOp::Font {
                    page: a.page,
                    id: a.id,
                    size,
                    color,
                });
            }
            None => doc.edit.fill.text_size = fs::step_text_size(doc.edit.fill.text_size, grow),
        }
    }
}

/// Menu Assinar: a assinatura e as iniciais guardadas (clique para colocar), criar ou trocar, e
/// apagar o que está guardado.
fn sign_menu(ui: &mut egui::Ui, doc: &mut DocTab) {
    let active = matches!(
        doc.edit.fill.tool,
        Some(FillTool::Signature | FillTool::Initials)
    );
    let button = egui::Button::selectable(active, "🖊 Assinar ⏷");
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        for (initials, name) in [(false, "assinatura"), (true, "iniciais")] {
            let has = with_saved(|s| s.get(initials).is_some());
            if has {
                let (r, resp) =
                    ui.allocate_exact_size(egui::vec2(180.0, 44.0), egui::Sense::click());
                ui.painter().rect_filled(r, 4.0, egui::Color32::WHITE);
                with_saved(|s| {
                    if let Some(saved) = s.get(initials) {
                        paint_saved(ui.painter(), saved, r.shrink(6.0), egui::Color32::BLACK);
                    }
                });
                if resp.on_hover_text(format!("Colocar {name}")).clicked() {
                    doc.edit.set_fill_tool(Some(if initials {
                        FillTool::Initials
                    } else {
                        FillTool::Signature
                    }));
                    ui.close();
                }
            }
            let label = match (has, initials) {
                (false, false) => "Adicionar assinatura…",
                (false, true) => "Adicionar iniciais…",
                (true, false) => "Alterar assinatura…",
                (true, true) => "Alterar iniciais…",
            };
            if ui.button(label).clicked() {
                doc.edit.open_signature_dialog(initials, None);
                ui.close();
            }
            ui.separator();
        }
        let any = with_saved(|s| s.signature.is_some() || s.initials.is_some());
        if ui
            .add_enabled(
                any,
                egui::Button::new("Apagar assinatura e iniciais guardadas"),
            )
            .clicked()
        {
            // store_saved também apaga a cópia da imagem
            for initials in [false, true] {
                if let Err(e) = store_saved(initials, None) {
                    doc.status = format!("Não foi possível apagar: {e}");
                }
            }
            ui.close();
        }
    });
}

/// Ferramentas de anotação, desfazer e refazer (aparece com "Comentar" ligado).
pub fn comment_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        for tool in Tool::ALL.into_iter().filter(|t| *t != Tool::Stamp) {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        stamp_menu(ui, doc);
        ui.separator();
        let can_edit = !doc.edit.saving;
        let can_undo = can_edit && doc.is_dirty();
        button(ui, &mut out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
        button(ui, &mut out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
    });
    out
}

/// Ferramenta Carimbo com o menu dos carimbos, nos dois grupos do Acrobat.
fn stamp_menu(ui: &mut egui::Ui, doc: &mut DocTab) {
    let active = doc.edit.tool == Tool::Stamp;
    let label = if active {
        format!("Carimbo: {}", doc.edit.stamp.title())
    } else {
        "Carimbo".to_owned()
    };
    let button = egui::Button::selectable(active, label);
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        for (n, (group, kinds)) in [
            ("Dinâmicos", &StampKind::DYNAMIC[..]),
            ("Padrão comercial", &StampKind::STANDARD[..]),
        ]
        .into_iter()
        .enumerate()
        {
            if n > 0 {
                ui.separator();
            }
            ui.label(egui::RichText::new(group).small().strong());
            for &kind in kinds {
                let text = egui::RichText::new(kind.title()).color(color32(kind.color()));
                if ui.button(text).clicked() {
                    doc.edit.stamp = kind;
                    doc.edit.tool = Tool::Stamp;
                }
            }
        }
    });
}

/// Botão de ícone que dispara `cmd`, com a dica (nome e atalho) tirada da tabela de comandos.
fn button(ui: &mut egui::Ui, out: &mut Option<Command>, icon: &str, cmd: Command, enabled: bool) {
    let tip = hover(&cmd);
    if ui
        .add_enabled(enabled, egui::Button::new(icon))
        .on_hover_text(tip)
        .clicked()
    {
        *out = Some(cmd);
    }
}
