//! Janela "Propriedades do documento" (Arquivo → Propriedades, Ctrl+D), no arranjo do Acrobat.
use app_state::pdf_date::format_pdf_date;
use pdf_core::{
    DocProperties, Duplex, InitialLayout, InitialPanel, InitialZoom, PageSize, PropertiesEdit,
    SecurityInfo,
};

/// Número com vírgula decimal (pt-BR).
fn decimal(v: f64, places: usize) -> String {
    format!("{v:.places$}").replace('.', ",")
}

/// Tamanho de arquivo legível: "900 bytes", "1,5 KB", "1,11 MB".
pub(crate) fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..=1023 => format!("{bytes} bytes"),
        1024..=1_048_575 => format!("{} KB", decimal(b / 1024.0, 1)),
        _ => format!("{} MB", decimal(b / 1_048_576.0, 2)),
    }
}

/// Tamanho da página em milímetros e polegadas.
fn page_size_text(size: PageSize) -> String {
    let (w, h) = (f64::from(size.width), f64::from(size.height));
    let mm = |pt: f64| (pt * 25.4 / 72.0).round();
    format!(
        "{} × {} mm ({} × {} pol)",
        mm(w),
        mm(h),
        decimal(w / 72.0, 2),
        decimal(h / 72.0, 2)
    )
}

fn or_dash(s: &str) -> &str {
    if s.trim().is_empty() {
        "—"
    } else {
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Description,
    Security,
    Fonts,
    InitialView,
    Custom,
    Advanced,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Description,
        Tab::Security,
        Tab::Fonts,
        Tab::InitialView,
        Tab::Custom,
        Tab::Advanced,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Description => "Descrição",
            Tab::Security => "Segurança",
            Tab::Fonts => "Fontes",
            Tab::InitialView => "Visualização inicial",
            Tab::Custom => "Personalizado",
            Tab::Advanced => "Avançado",
        }
    }
}

/// Janela "Propriedades do documento" (Arquivo → Propriedades, Ctrl+D): edita uma cópia; o OK
/// devolve o que mudou.
#[derive(Debug, Clone)]
pub struct PropertiesDialog {
    props: DocProperties,
    security: SecurityInfo,
    pub edit: PropertiesEdit,
    tab: Tab,
    /// Linha nova da aba Personalizado (nome, valor).
    pub new_custom: (String, String),
}

impl PropertiesDialog {
    pub fn new(props: DocProperties, security: SecurityInfo) -> Self {
        Self {
            edit: PropertiesEdit::from(&props),
            props,
            security,
            tab: Tab::default(),
            new_custom: Default::default(),
        }
    }

    /// OK com mudanças: o que gravar. Cancelar, ou OK sem mudar nada: `None`.
    pub fn finish(self, ok: bool) -> Option<PropertiesEdit> {
        (ok && self.edit != PropertiesEdit::from(&self.props)).then_some(self.edit)
    }

    /// Acrescenta (ou troca o valor de) a propriedade da linha nova; recusa nome vazio ou padrão.
    pub fn add_custom(&mut self) -> bool {
        let name = self.new_custom.0.trim().to_owned();
        if !pdf_core::valid_custom_name(&name) {
            return false;
        }
        let value = std::mem::take(&mut self.new_custom.1);
        match self.edit.custom.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = value,
            None => self.edit.custom.push((name, value)),
        }
        self.new_custom.0.clear();
        true
    }

    /// Mostra a janela; `Some(true)` no OK, `Some(false)` no Cancelar ou ao fechar.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut answer = None;
        let mut open = true;
        egui::Window::new("Propriedades do documento")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for t in Tab::ALL {
                        ui.selectable_value(&mut self.tab, t, t.label());
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| self.page(ui));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancelar").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if !open {
            answer = Some(false);
        }
        answer
    }

    fn page(&mut self, ui: &mut egui::Ui) {
        let p = &self.props;
        let e = &mut self.edit;
        match self.tab {
            Tab::Description => {
                egui::Grid::new("descricao").num_columns(2).show(ui, |ui| {
                    ui.strong("Arquivo:");
                    ui.label(p.path.display().to_string());
                    ui.end_row();
                    for (label, value) in [
                        ("Título:", &mut e.title),
                        ("Autor:", &mut e.author),
                        ("Assunto:", &mut e.subject),
                        ("Palavras-chave:", &mut e.keywords),
                    ] {
                        ui.strong(label);
                        ui.add(egui::TextEdit::singleline(value).desired_width(360.0));
                        ui.end_row();
                    }
                    for (label, value) in [
                        ("Criado:", format_pdf_date(&p.created)),
                        ("Modificado:", format_pdf_date(&p.modified)),
                        ("Aplicativo:", or_dash(&p.creator).into()),
                        ("Produtor do PDF:", or_dash(&p.producer).into()),
                        ("Versão do PDF:", or_dash(&p.format).into()),
                        ("Tamanho da página:", page_size_text(p.first_page)),
                        ("Número de páginas:", p.pages.to_string()),
                        ("Tamanho do arquivo:", size_text(p.file_size)),
                    ] {
                        ui.strong(label);
                        ui.label(value);
                        ui.end_row();
                    }
                });
            }
            Tab::Security => {
                let s = &self.security;
                let yes = |b: bool| if b { "Permitido" } else { "Não permitido" };
                ui.label(if s.encrypted {
                    "Método de segurança: Segurança por senha"
                } else {
                    "Método de segurança: Nenhuma segurança"
                });
                ui.add_space(4.0);
                egui::Grid::new("seguranca").num_columns(2).show(ui, |ui| {
                    let perms = s.permissions;
                    for (label, ok) in [
                        ("Impressão:", perms.print),
                        ("Alteração do documento:", perms.modify),
                        ("Cópia de conteúdo:", perms.copy),
                        ("Comentários:", perms.annotate),
                        ("Preenchimento de campos e assinatura:", perms.forms),
                    ] {
                        ui.strong(label);
                        ui.label(yes(ok));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.weak("Para mudar a segurança, use Ferramentas → Proteger PDF.");
            }
            Tab::Fonts => {
                if p.fonts.is_empty() {
                    ui.weak("Nenhuma fonte nas páginas.");
                }
                for f in &p.fonts {
                    let how = match (f.embedded, f.subset) {
                        (true, true) => "subconjunto incorporado",
                        (true, false) => "incorporada",
                        (false, _) => "não incorporada",
                    };
                    ui.label(format!("{} — {} ({how})", f.name, f.kind));
                }
            }
            Tab::InitialView => initial_view_page(ui, e, p.pages),
            Tab::Custom => {
                let mut remove = None;
                egui::Grid::new("personalizado")
                    .num_columns(3)
                    .show(ui, |ui| {
                        for (k, (name, value)) in e.custom.iter_mut().enumerate() {
                            ui.label(name.as_str());
                            ui.add(egui::TextEdit::singleline(value).desired_width(260.0));
                            if ui.small_button("Excluir").clicked() {
                                remove = Some(k);
                            }
                            ui.end_row();
                        }
                    });
                if let Some(k) = remove {
                    e.custom.remove(k);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Nome:");
                    ui.add(egui::TextEdit::singleline(&mut self.new_custom.0).desired_width(120.0));
                    ui.label("Valor:");
                    ui.add(egui::TextEdit::singleline(&mut self.new_custom.1).desired_width(160.0));
                    if ui.button("Adicionar").clicked() && !self.add_custom() {
                        self.new_custom.0.clear();
                    }
                });
            }
            Tab::Advanced => {
                ui.horizontal(|ui| {
                    ui.label("Idioma:");
                    ui.add(
                        egui::TextEdit::singleline(&mut e.lang)
                            .hint_text("pt-BR")
                            .desired_width(80.0),
                    );
                });
                ui.add_space(6.0);
                ui.strong("Predefinições da caixa de diálogo de impressão");
                ui.checkbox(
                    &mut e.print.scaling_none,
                    "Escala de página: nenhuma (tamanho real)",
                );
                egui::ComboBox::from_label("Frente e verso")
                    .selected_text(duplex_label(e.print.duplex))
                    .show_ui(ui, |ui| {
                        for d in [
                            None,
                            Some(Duplex::Simplex),
                            Some(Duplex::LongEdge),
                            Some(Duplex::ShortEdge),
                        ] {
                            ui.selectable_value(&mut e.print.duplex, d, duplex_label(d));
                        }
                    });
                let mut copies = e.print.copies.unwrap_or(1);
                ui.horizontal(|ui| {
                    ui.label("Número de cópias:");
                    if ui
                        .add(egui::DragValue::new(&mut copies).range(1..=999))
                        .changed()
                    {
                        e.print.copies = (copies > 1).then_some(copies);
                    }
                });
            }
        }
    }
}

fn duplex_label(d: Option<Duplex>) -> &'static str {
    match d {
        None => "Padrão",
        Some(Duplex::Simplex) => "Só frente",
        Some(Duplex::LongEdge) => "Virar na borda longa",
        Some(Duplex::ShortEdge) => "Virar na borda curta",
    }
}

/// Aba Visualização inicial: painel, layout, ampliação, página e opções de janela.
fn initial_view_page(ui: &mut egui::Ui, e: &mut PropertiesEdit, pages: i32) {
    let v = &mut e.initial;
    ui.strong("Layout e ampliação");
    let panels = [
        (None, "Somente a página"),
        (Some(InitialPanel::Bookmarks), "Painel Marcadores e página"),
        (Some(InitialPanel::Thumbnails), "Painel Miniaturas e página"),
        (Some(InitialPanel::Attachments), "Painel Anexos e página"),
    ];
    egui::ComboBox::from_label("Mostrar")
        .selected_text(
            panels
                .iter()
                .find(|(p, _)| *p == v.panel)
                .map_or("", |(_, l)| *l),
        )
        .show_ui(ui, |ui| {
            for (p, l) in panels {
                ui.selectable_value(&mut v.panel, p, l);
            }
        });
    let layouts = [
        (None, "Padrão"),
        (Some(InitialLayout::Single), "Página única"),
        (Some(InitialLayout::OneColumn), "Página única contínua"),
        (
            Some(InitialLayout::TwoPage { cover: false }),
            "Duas páginas",
        ),
        (
            Some(InitialLayout::TwoPage { cover: true }),
            "Duas páginas (capa)",
        ),
        (
            Some(InitialLayout::TwoColumn { cover: false }),
            "Duas páginas contínuas",
        ),
        (
            Some(InitialLayout::TwoColumn { cover: true }),
            "Duas páginas contínuas (capa)",
        ),
    ];
    egui::ComboBox::from_label("Layout da página")
        .selected_text(
            layouts
                .iter()
                .find(|(l, _)| *l == v.layout)
                .map_or("", |(_, t)| *t),
        )
        .show_ui(ui, |ui| {
            for (l, t) in layouts {
                ui.selectable_value(&mut v.layout, l, t);
            }
        });
    let zooms = [
        (None, "Padrão".to_string()),
        (Some(InitialZoom::FitPage), "Ajustar à página".to_string()),
        (Some(InitialZoom::FitWidth), "Ajustar à largura".to_string()),
    ]
    .into_iter()
    .chain(app_state::zoom::LEVELS.into_iter().map(|z| {
        (
            Some(InitialZoom::Factor(z)),
            app_state::zoom::percent_text(z),
        )
    }))
    .collect::<Vec<_>>();
    egui::ComboBox::from_label("Ampliação")
        .selected_text(
            zooms
                .iter()
                .find(|(z, _)| *z == v.zoom)
                .map_or("", |(_, t)| t.as_str()),
        )
        .show_ui(ui, |ui| {
            for (z, t) in &zooms {
                ui.selectable_value(&mut v.zoom, *z, t.as_str());
            }
        });
    let mut page = v.page.map_or(1, |p| p + 1);
    ui.horizontal(|ui| {
        ui.label("Abrir na página:");
        if ui
            .add(egui::DragValue::new(&mut page).range(1..=pages.max(1)))
            .changed()
        {
            v.page = (page > 1).then_some(page - 1);
        }
    });
    ui.add_space(6.0);
    ui.strong("Opções de janela");
    let w = &mut e.window;
    ui.checkbox(
        &mut w.fit_window,
        "Redimensionar a janela para a página inicial",
    );
    ui.checkbox(&mut w.center_window, "Centralizar a janela na tela");
    ui.checkbox(&mut v.full_screen, "Abrir no modo de tela cheia");
    ui.checkbox(&mut w.display_title, "Mostrar o título do documento");
    ui.add_space(6.0);
    ui.strong("Opções da interface (para outros leitores)");
    ui.checkbox(&mut w.hide_menubar, "Ocultar a barra de menus");
    ui.checkbox(&mut w.hide_toolbar, "Ocultar as barras de ferramentas");
    ui.checkbox(&mut w.hide_window_ui, "Ocultar os controles da janela");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_security() -> SecurityInfo {
        SecurityInfo {
            encrypted: false,
            needs_user_password: false,
            permissions: pdf_core::Permissions::ALL,
            unlocked: false,
        }
    }

    fn props() -> DocProperties {
        DocProperties {
            path: "/a.pdf".into(),
            title: "Antigo".into(),
            author: String::new(),
            subject: String::new(),
            keywords: String::new(),
            creator: String::new(),
            producer: String::new(),
            created: String::new(),
            modified: String::new(),
            format: "PDF 1.7".into(),
            encrypted: false,
            pages: 1,
            first_page: PageSize {
                width: 612.0,
                height: 792.0,
            },
            file_size: 10,
            custom: vec![],
            initial: Default::default(),
            window: Default::default(),
            lang: String::new(),
            print: Default::default(),
            fonts: vec![],
        }
    }

    #[test]
    fn properties_draft_cancel_keeps_the_document() {
        let mut d = PropertiesDialog::new(props(), open_security());
        d.edit.title = "Novo".into();
        assert_eq!(
            PropertiesDialog::new(props(), open_security()).finish(true),
            None,
            "OK sem mudar nada: nada a gravar"
        );
        let cancelled = PropertiesDialog { ..d.clone() }.finish(false);
        assert_eq!(cancelled, None);
        assert_eq!(d.finish(true).map(|e| e.title), Some("Novo".to_string()));
    }

    #[test]
    fn custom_properties_are_added_and_removed() {
        let mut d = PropertiesDialog::new(props(), open_security());
        d.new_custom = ("Projeto".into(), "Gera".into());
        assert!(d.add_custom());
        assert_eq!(
            d.edit.custom,
            vec![("Projeto".to_string(), "Gera".to_string())]
        );
        d.new_custom = ("Title".into(), "x".into());
        assert!(!d.add_custom(), "nome padrão recusado");
        d.new_custom = ("Projeto".into(), "outro".into());
        assert!(d.add_custom(), "mesmo nome: troca o valor");
        assert_eq!(
            d.edit.custom,
            vec![("Projeto".to_string(), "outro".to_string())]
        );
    }

    #[test]
    fn sizes_are_human() {
        assert_eq!(size_text(900), "900 bytes");
        assert_eq!(size_text(1_536), "1,5 KB");
        assert_eq!(size_text(1_166_161), "1,11 MB");
    }

    #[test]
    fn page_size_in_mm_and_inches() {
        let a4 = PageSize {
            width: 595.0,
            height: 842.0,
        };
        assert_eq!(page_size_text(a4), "210 × 297 mm (8,26 × 11,69 pol)");
    }

    #[test]
    fn empty_fields_show_a_dash() {
        assert_eq!(or_dash(""), "—");
        assert_eq!(or_dash("  "), "—");
        assert_eq!(or_dash("Gera"), "Gera");
    }
}
