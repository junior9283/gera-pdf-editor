//! Barra de ferramentas do documento e a linha de ferramentas de comentário.
use crate::doc_tab::{DocCommand, DocTab};
use crate::fill_sign::{paint_saved, store_saved, with_saved};
use crate::shell::annot_props::color32;
use crate::shell::commands::{hover, Command};
use app_state::fill_sign::{self as fs, FillTool};
use app_state::stamps::StampKind;
use app_state::tools::Tool;
use app_state::tools_catalog;
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
        // ferramentas de tela, como no Acrobat: Selecionar, Mão e Zoom com marca
        for tool in Tool::SCREEN {
            let icon = match tool {
                Tool::Hand => "✋",
                Tool::Zoom => "🔎",
                _ => "↖",
            };
            let cmd = Command::Doc(DocCommand::Tool(tool));
            let on = doc.edit.tool == tool;
            if ui
                .add(egui::Button::selectable(on, icon))
                .on_hover_ui(|ui| {
                    ui.label(hover(&cmd));
                })
                .clicked()
            {
                out = Some(cmd);
            }
        }
        ui.separator();
        doc.page_controls(ui);
        ui.separator();
        zoom_group(ui, doc, &mut out);
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

/// − + campo de zoom e ⏷ (barra do documento e barra do modo leitura).
pub(crate) fn zoom_group(ui: &mut egui::Ui, doc: &mut DocTab, out: &mut Option<Command>) {
    button(ui, out, "−", Command::Doc(DocCommand::ZoomOut), true);
    button(ui, out, "+", Command::Doc(DocCommand::ZoomIn), true);
    if let Some(fit) = doc.zoom_controls(ui) {
        *out = Some(Command::Doc(fit));
    }
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
        undo_redo(ui, doc, &mut out);
        close_row(ui, doc);
    });
    out
}

/// Linha do Editar PDF: acrescentar texto e imagens, girar, substituir e excluir imagens,
/// desfazer/refazer e Fechar.
pub fn edit_pdf_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        use crate::edit_pdf::Placing;
        let adding = matches!(doc.edit_pdf.placing, Some(Placing::Text));
        if ui
            .add(egui::Button::selectable(adding, "T Adicionar texto"))
            .on_hover_text("Clique na página onde o texto deve começar")
            .clicked()
        {
            doc.edit_pdf.placing = (!adding).then_some(Placing::Text);
        }
        let placing = matches!(doc.edit_pdf.placing, Some(Placing::Image(_)));
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
            for (icon, tip, clockwise) in [
                ("⟲", "Girar no sentido anti-horário", false),
                ("⟳", "Girar no sentido horário", true),
            ] {
                if ui.button(icon).on_hover_text(tip).clicked() {
                    doc.edit_image(pdf_core::ImageEdit::Rotate { clockwise });
                }
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
        undo_redo(ui, doc, &mut out);
        close_row(ui, doc);
    });
    out
}

/// Linha do Redigir (como no Acrobat): marcar texto ou área, Localizar texto e redigir,
/// Propriedades, Aplicar, Limpar documento, desfazer/refazer e Fechar.
pub fn redact_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    use app_state::tools::Tool;
    let mut out = None;
    ui.horizontal(|ui| {
        for (tool, label) in [
            (Tool::RedactText, "⬛ Marcar texto"),
            (Tool::RedactArea, "⬚ Marcar área"),
        ] {
            ui.selectable_value(&mut doc.edit.tool, tool, label)
                .on_hover_text(tool.label());
        }
        if ui.button("🔍 Localizar texto…").clicked() {
            doc.redact.find = Some(Default::default());
        }
        if ui.button("Propriedades…").clicked() {
            doc.redact.open_props();
        }
        let marks = doc
            .annots
            .iter()
            .filter(|a| a.kind == pdf_core::AnnotKind::Redact)
            .count();
        if ui
            .add_enabled(marks > 0, egui::Button::new(format!("Aplicar ({marks})")))
            .on_hover_text("Apaga de vez o que está sob as marcas")
            .clicked()
        {
            doc.redact.confirm = true;
        }
        if ui.button("Limpar documento…").clicked() {
            doc.redact.sanitize = Some(Default::default());
        }
        ui.separator();
        undo_redo(ui, doc, &mut out);
        close_row(ui, doc);
    });
    out
}

/// Linha Comparação (resultado do Comparar arquivos): a alteração atual, anterior/próxima
/// (Ctrl+, / Ctrl+.), Nova comparação e Fechar.
pub fn compare_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    ui.horizontal(|ui| {
        let changes = doc.changes();
        let at = doc.change_at(&changes);
        let n = changes.len();
        ui.label(match (n, at) {
            (0, _) => "Nenhuma alteração".to_string(),
            (_, Some(i)) => format!("Alteração {} de {n}", i + 1),
            (1, None) => "1 alteração".to_string(),
            (_, None) => format!("{n} alterações"),
        });
        for (label, cmd, enabled) in [
            (
                "◀ Anterior",
                DocCommand::PrevChange,
                at.is_some_and(|i| i > 0),
            ),
            (
                "Próxima ▶",
                DocCommand::NextChange,
                n > 0 && at.is_none_or(|i| i + 1 < n),
            ),
        ] {
            button(ui, &mut out, label, Command::Doc(cmd), enabled);
        }
        ui.separator();
        if ui.button("Nova comparação…").clicked() {
            out = Some(Command::Compare);
        }
        close_row(ui, doc);
    });
    out
}

/// Linha da Marca d'água ou do Cabeçalho e rodapé: Adicionar…, Atualizar…, Remover, desfazer/refazer
/// e Fechar (como no Acrobat).
pub fn pagination_row(ui: &mut egui::Ui, doc: &mut DocTab) -> Option<Command> {
    let mut out = None;
    let present = doc.pagination.present;
    let kind = doc.pagination.tool?;
    let what = crate::pagination_tool::label(kind);
    ui.horizontal(|ui| {
        if ui.button("➕ Adicionar…").clicked() {
            doc.pagination.add();
        }
        if ui
            .add_enabled(present, egui::Button::new("Atualizar…"))
            .on_disabled_hover_text(format!("O documento não tem {what}"))
            .clicked()
        {
            doc.pagination.open(true);
        }
        if ui
            .add_enabled(present, egui::Button::new("🗑 Remover"))
            .on_hover_text(format!("Tira {what} de todas as páginas"))
            .clicked()
        {
            doc.remove_pagination();
        }
        ui.label(if present {
            format!("O documento tem {what}.")
        } else {
            format!("Sem {what}.")
        });
        ui.separator();
        undo_redo(ui, doc, &mut out);
        close_row(ui, doc);
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
        undo_redo(ui, doc, &mut out);
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
        let markup = app_state::tools::ToolGroup::Markup.tools();
        for &tool in std::iter::once(&Tool::Select).chain(markup) {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        tool_menu(
            ui,
            doc,
            "Editar texto",
            &[Tool::ReplaceText, Tool::InsertText],
        );
        for tool in [Tool::Note, Tool::TextBox, Tool::Callout] {
            ui.selectable_value(&mut doc.edit.tool, tool, tool.label());
        }
        tool_menu(
            ui,
            doc,
            "Desenho",
            app_state::tools::ToolGroup::Drawing.tools(),
        );
        stamp_menu(ui, doc);
        ui.selectable_value(&mut doc.edit.tool, Tool::Attach, Tool::Attach.label());
        ui.separator();
        undo_redo(ui, doc, &mut out);
    });
    out
}

/// Botão com menu de ferramentas (como "Ferramentas de desenho" no Acrobat): mostra a ativa do
/// grupo, ou o nome do grupo.
fn tool_menu(ui: &mut egui::Ui, doc: &mut DocTab, group: &str, tools: &[Tool]) {
    let active = tools.contains(&doc.edit.tool);
    let label = if active {
        format!("{group}: {}", doc.edit.tool.label())
    } else {
        group.to_owned()
    };
    let button = egui::Button::selectable(active, label);
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        for &tool in tools {
            if ui
                .selectable_label(doc.edit.tool == tool, tool.label())
                .clicked()
            {
                doc.edit.tool = tool;
                ui.close();
            }
        }
    });
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
/// Desfazer e refazer da linha de ferramenta.
fn undo_redo(ui: &mut egui::Ui, doc: &DocTab, out: &mut Option<Command>) {
    let can_edit = !doc.edit.saving;
    let can_undo = can_edit && doc.is_dirty();
    button(ui, out, "⮪", Command::Doc(DocCommand::Undo), can_undo);
    button(ui, out, "⮫", Command::Doc(DocCommand::Redo), can_edit);
}

/// "Fechar" à direita da linha de ferramenta.
fn close_row(ui: &mut egui::Ui, doc: &mut DocTab) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Fechar").clicked() {
            doc.show_row("");
        }
    });
}

fn button(ui: &mut egui::Ui, out: &mut Option<Command>, icon: &str, cmd: Command, enabled: bool) {
    if ui
        .add_enabled(enabled, egui::Button::new(icon))
        .on_hover_ui(|ui| {
            ui.label(hover(&cmd)); // montada só quando a dica aparece
        })
        .clicked()
    {
        *out = Some(cmd);
    }
}
