use app_state::prefs::LeftPanel;
use pdf_core::{AnnotInfo, AnnotKind};

pub enum SidebarAction {
    None,
    /// Seleciona a anotação e leva até ela.
    SelectAnnot {
        page: i32,
        id: i32,
        at: (f32, f32),
    },
}

/// Nome do tipo da anotação, como o Acrobat mostra (a linha com ponta é "Seta").
pub(crate) fn label_of(a: &AnnotInfo) -> &'static str {
    if a.arrow {
        "Seta"
    } else {
        kind_label(a.kind)
    }
}

pub(crate) fn kind_label(kind: AnnotKind) -> &'static str {
    match kind {
        AnnotKind::Highlight => "Destaque",
        AnnotKind::Underline => "Sublinhado",
        AnnotKind::Note => "Nota",
        AnnotKind::Ink => "Desenho",
        AnnotKind::StrikeOut => "Riscado",
        AnnotKind::TextBox => "Caixa de texto",
        AnnotKind::Rect => "Retângulo",
        AnnotKind::Ellipse => "Elipse",
        AnnotKind::Line => "Linha",
        AnnotKind::Stamp => "Carimbo",
        AnnotKind::Redact => "Marca de redação",
        AnnotKind::Polygon => "Polígono",
        AnnotKind::PolyLine => "Linhas conectadas",
        AnnotKind::Cloud => "Nuvem",
        AnnotKind::Other => "Anotação",
    }
}

fn annot_label(a: &AnnotInfo) -> String {
    let mut label = format!("{} — p. {}", label_of(a), a.page + 1);
    if !a.contents.is_empty() {
        label.push_str(&format!(": {}", a.contents));
    }
    label
}

pub fn show(ui: &mut egui::Ui, panel: LeftPanel, annots: &[AnnotInfo]) -> SidebarAction {
    ui.strong(panel.label());
    ui.separator();
    match panel {
        // miniaturas e marcadores têm painel próprio (thumbnails.rs, bookmarks.rs)
        LeftPanel::Thumbnails | LeftPanel::Outline => SidebarAction::None,
        LeftPanel::Annotations => annotation_list(ui, annots),
        // desenhado pela aba (precisa das assinaturas validadas)
        // assinaturas e anexos têm painel próprio
        LeftPanel::Signatures
        | LeftPanel::Attachments
        | LeftPanel::Layers
        | LeftPanel::Destinations => SidebarAction::None,
    }
}

fn annotation_list(ui: &mut egui::Ui, annots: &[AnnotInfo]) -> SidebarAction {
    let mut action = SidebarAction::None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // respostas e status aparecem dentro do comentário, no painel Comentários
            let mut roots = annots.iter().filter(|a| !a.is_reply()).peekable();
            if roots.peek().is_none() {
                ui.label("Nenhuma anotação.");
            }
            for a in roots {
                if ui.button(annot_label(a)).clicked() {
                    action = SidebarAction::SelectAnnot {
                        page: a.page,
                        id: a.id,
                        at: (a.rect.x0, a.rect.y0),
                    };
                }
            }
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    #[test]
    fn labels_mention_kind_page_and_text() {
        let mut a = AnnotInfo {
            id: 1,
            color: None,
            opacity: 1.0,
            width: None,
            kind: AnnotKind::Underline,
            page: 1,
            rect: PdfRect::new(0.0, 0.0, 1.0, 1.0),
            contents: String::new(),
            ..Default::default()
        };
        assert_eq!(annot_label(&a), "Sublinhado — p. 2");
        a.kind = AnnotKind::Note;
        a.contents = "olá".into();
        assert_eq!(annot_label(&a), "Nota — p. 2: olá");
    }
}
