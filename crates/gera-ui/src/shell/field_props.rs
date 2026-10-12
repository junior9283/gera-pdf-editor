//! Janela "Propriedades do campo" do Preparar formulário (como no Acrobat): nome, dica,
//! obrigatório, somente leitura e as opções de cada tipo.
use pdf_core::{CalcOp, FieldCalc, FieldFormat, FieldKind, FieldProps, FieldValidate};

/// Que partes da janela o campo usa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Button {
        radio: bool,
    },
    Choice {
        combo: bool,
    },
    /// Botão de ação: rótulo e Redefinir formulário.
    Push,
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
    tab: Tab,
    format: FormatDraft,
    validate: ValidateDraft,
    calc: CalcDraft,
    /// Os outros campos do documento (para o Calcular).
    names: Vec<String>,
    /// Aba Ações e os campos que as ações podem citar (todos, inclusive este).
    actions: crate::shell::actions_editor::ActionsEditor<pdf_core::FieldTrigger>,
    all_names: Vec<String>,
    pages: usize,
    /// "Escolher…" do ícone foi clicado: a aba abre o seletor de imagem.
    pub wants_icon: bool,
    /// Código de barras: abas Valor e Opções (`None` nos outros campos).
    barcode: Option<BarcodeDraft>,
}

/// Rascunho do código de barras: os campos codificados, se leva os nomes, e as opções.
pub(crate) struct BarcodeDraft {
    pub fields: Vec<String>,
    pub names: bool,
    pub options: pdf_core::BarcodeOptions,
}

impl BarcodeDraft {
    fn from(props: &FieldProps) -> Option<Self> {
        let options = props.barcode?;
        let (fields, names) = match &props.calc {
            FieldCalc::Barcode { fields, names } => (fields.clone(), *names),
            _ => (Vec::new(), true),
        };
        Some(Self {
            fields,
            names,
            options,
        })
    }

    /// Aba Valor: os campos codificados e os nomes.
    fn value_ui(&mut self, ui: &mut egui::Ui, names: &[String]) {
        ui.label("Codificar os dados delimitados por tabulação destes campos:");
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                for n in names {
                    let mut on = self.fields.contains(n);
                    if ui
                        .checkbox(&mut on, crate::links::safe_display(n))
                        .changed()
                    {
                        if on {
                            self.fields.push(n.clone());
                        } else {
                            self.fields.retain(|x| x != n);
                        }
                    }
                }
            });
        ui.checkbox(&mut self.names, "Incluir os nomes dos campos");
    }

    /// Aba Opções: simbologia e correção de erros.
    fn options_ui(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.options;
        egui::Grid::new("opcoes-codigo")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Simbologia");
                egui::ComboBox::from_id_salt("simbologia")
                    .selected_text(o.symbology.label())
                    .show_ui(ui, |ui| {
                        for s in pdf_core::Symbology::ALL {
                            if ui
                                .selectable_value(&mut o.symbology, s, s.label())
                                .changed()
                            {
                                o.ecc = o.ecc.min(s.ecc_levels().len().saturating_sub(1) as u8);
                            }
                        }
                    });
                ui.end_row();
                let levels = o.symbology.ecc_levels();
                if !levels.is_empty() {
                    ui.label("Correção de erros");
                    egui::ComboBox::from_id_salt("correcao")
                        .selected_text(levels[usize::from(o.ecc).min(levels.len() - 1)])
                        .show_ui(ui, |ui| {
                            for (k, label) in levels.iter().enumerate() {
                                ui.selectable_value(&mut o.ecc, k as u8, *label);
                            }
                        });
                    ui.end_row();
                }
            });
    }
}

impl FieldPropsWindow {
    pub fn new(
        page: i32,
        id: i32,
        kind: &FieldKind,
        props: FieldProps,
        names: Vec<String>,
        pages: usize,
    ) -> Self {
        let kind = match kind {
            FieldKind::Text { .. } => Kind::Text,
            FieldKind::Checkbox { .. } => Kind::Button { radio: false },
            FieldKind::Radio { .. } => Kind::Button { radio: true },
            FieldKind::Combo { .. } => Kind::Choice { combo: true },
            FieldKind::List { .. } => Kind::Choice { combo: false },
            FieldKind::Button => Kind::Push,
            FieldKind::Signature => Kind::Other,
        };
        Self {
            page,
            id,
            kind,
            tab: Tab::General,
            format: FormatDraft::from(&props.format),
            validate: ValidateDraft::from(&props.validate),
            calc: CalcDraft::from(&props.calc),
            actions: crate::shell::actions_editor::ActionsEditor::new(
                props.actions.clone(),
                &pdf_core::FieldTrigger::ALL,
            ),
            all_names: names.clone(),
            pages,
            wants_icon: false,
            barcode: BarcodeDraft::from(&props),
            names: names.into_iter().filter(|n| *n != props.name).collect(),
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
        let mut tabs = vec![(Tab::General, "Geral")];
        if self.kind == Kind::Push || self.barcode.is_some() {
            tabs.push((Tab::Options, "Opções"));
        }
        if self.barcode.is_some() {
            tabs.push((Tab::Value, "Valor"));
        }
        tabs.push((Tab::Actions, "Ações"));
        if self.kind == Kind::Text && self.barcode.is_none() {
            tabs.extend([
                (Tab::Format, "Formatar"),
                (Tab::Validate, "Validar"),
                (Tab::Calculate, "Calcular"),
            ]);
        }
        ui.horizontal(|ui| {
            for (tab, label) in tabs {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        match self.tab {
            Tab::Actions => self.actions.ui(ui, &self.all_names, self.pages),
            Tab::Options => match self.barcode.as_mut() {
                Some(b) => b.options_ui(ui),
                None => self.button_options(ui),
            },
            Tab::Value => {
                if let Some(b) = self.barcode.as_mut() {
                    b.value_ui(ui, &self.names);
                }
            }
            Tab::Format => self.format.ui(ui),
            Tab::Validate => self.validate.ui(ui),
            Tab::Calculate => self.calc.ui(ui, &self.names),
            Tab::General => self.general(ui),
        }
        ui.separator();
        let mut out = None;
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                out = Some(Some(self.built()));
            }
            if ui.button("Cancelar").clicked() {
                out = Some(None);
            }
        });
        out
    }

    /// As propriedades com o que está nas abas.
    fn built(&self) -> FieldProps {
        let mut props = self.props.clone();
        props.max_len = self.max_len.trim().parse().ok().filter(|&n: &usize| n > 0);
        if self.kind == Kind::Text {
            props.format = self.format.format();
            props.validate = self.validate.validate();
            props.calc = self.calc.calc();
        }
        props.actions = self.actions.list.clone();
        if let Some(b) = &self.barcode {
            props.calc = FieldCalc::Barcode {
                fields: b.fields.clone(),
                names: b.names,
            };
            props.barcode = Some(b.options);
        }
        props
    }

    /// A imagem escolhida para o ícone do botão.
    pub fn set_icon(&mut self, path: std::path::PathBuf) {
        self.props.icon_path = Some(path);
        self.props.has_icon = true;
    }

    fn clear_icon(&mut self) {
        self.props.icon_path = None;
        self.props.has_icon = false;
    }

    /// A aba Opções dos botões (como no Acrobat): layout, comportamento, rótulo e ícone.
    fn button_options(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.props;
        egui::Grid::new("opcoes-botao")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Layout");
                let layout = usize::from(p.layout.min(6));
                egui::ComboBox::from_id_salt("layout-botao")
                    .selected_text(pdf_core::LAYOUTS[layout])
                    .show_ui(ui, |ui| {
                        for (k, label) in pdf_core::LAYOUTS.iter().enumerate() {
                            ui.selectable_value(&mut p.layout, k as u8, *label);
                        }
                    });
                ui.end_row();
                ui.label("Comportamento");
                let behavior = usize::from(p.behavior.min(3));
                egui::ComboBox::from_id_salt("comportamento-botao")
                    .selected_text(pdf_core::BEHAVIORS[behavior].1)
                    .show_ui(ui, |ui| {
                        for (k, (_, label)) in pdf_core::BEHAVIORS.iter().enumerate() {
                            ui.selectable_value(&mut p.behavior, k as u8, *label);
                        }
                    });
                ui.end_row();
                ui.label("Rótulo");
                ui.add_enabled(
                    p.layout != 1,
                    egui::TextEdit::singleline(&mut p.button_label),
                );
                ui.end_row();
                ui.label("Ícone");
                ui.horizontal(|ui| {
                    let state = match (&p.icon_path, p.has_icon) {
                        (Some(path), _) => path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        (None, true) => "imagem do arquivo".into(),
                        (None, false) => "nenhum".into(),
                    };
                    ui.label(crate::links::safe_display(&state));
                    if ui.button("Escolher…").clicked() {
                        self.wants_icon = true;
                    }
                });
                ui.end_row();
            });
        if self.props.has_icon && ui.button("Limpar ícone").clicked() {
            self.clear_icon();
        }
    }

    /// A aba Geral (e tudo, nos tipos sem abas).
    fn general(&mut self, ui: &mut egui::Ui) {
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
            Kind::Push | Kind::Other => {}
        }
        if !matches!(self.kind, Kind::Other | Kind::Button { .. }) {
            ui.horizontal(|ui| {
                ui.label("Tamanho da fonte");
                ui.add(egui::DragValue::new(&mut p.font_size).range(0.0..=72.0))
                    .on_hover_text("0 = automático");
            });
        }
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

/// Categoria da aba Formatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    None,
    Number,
    Percent,
    Date,
    Time,
    Special,
    /// Script do arquivo que o app não entende (fica como está).
    Custom,
}

impl Category {
    const PICK: [(Self, &'static str); 6] = [
        (Self::None, "Nenhum"),
        (Self::Number, "Número"),
        (Self::Percent, "Porcentagem"),
        (Self::Date, "Data"),
        (Self::Time, "Hora"),
        (Self::Special, "Especial"),
    ];
}

/// Estilos do separador e dos negativos, como na aba Formatar do Acrobat.
const SEPARATORS: [&str; 5] = ["1,234.56", "1234.56", "1.234,56", "1234,56", "1'234.56"];
const NEGATIVES: [&str; 4] = [
    "-1.234,56",
    "1.234,56 em vermelho",
    "(1.234,56)",
    "(1.234,56) em vermelho",
];
const DATE_PRESETS: [&str; 9] = [
    "dd/mm/yyyy",
    "dd/mm/yy",
    "d/m/yyyy",
    "dd-mm-yyyy",
    "yyyy-mm-dd",
    "mm/dd/yyyy",
    "d mmmm yyyy",
    "dd/mm/yyyy HH:MM",
    "mmmm yyyy",
];
/// Máscaras prontas do Especial (as do Brasil e as do Acrobat).
pub(crate) const SPECIAL_PRESETS: [(&str, &str); 8] = [
    ("CEP", "99999-999"),
    ("Telefone", "(99) 99999-9999"),
    ("CPF", "999.999.999-99"),
    ("CNPJ", "99.999.999/9999-99"),
    ("CEP dos EUA", "99999"),
    ("CEP + 4 dos EUA", "99999-9999"),
    ("Telefone dos EUA", "(999) 999-9999"),
    ("Seguro social dos EUA", "999-99-9999"),
];

/// Rascunho da aba Formatar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FormatDraft {
    pub category: Category,
    pub dec: u8,
    pub sep: u8,
    pub neg: u8,
    pub currency: String,
    pub prepend: bool,
    pub date: String,
    pub time: u8,
    pub mask: String,
}

impl FormatDraft {
    pub fn from(f: &FieldFormat) -> Self {
        let mut d = Self {
            category: Category::None,
            dec: 2,
            sep: 2,
            neg: 0,
            currency: String::new(),
            prepend: true,
            date: DATE_PRESETS[0].into(),
            time: 0,
            mask: SPECIAL_PRESETS[0].1.into(),
        };
        match f {
            FieldFormat::None => {}
            FieldFormat::Number {
                dec,
                sep,
                neg,
                currency,
                prepend,
            } => {
                (d.category, d.dec, d.sep, d.neg) = (Category::Number, *dec, *sep, *neg);
                (d.currency, d.prepend) = (currency.clone(), *prepend);
            }
            FieldFormat::Percent { dec, sep } => {
                (d.category, d.dec, d.sep) = (Category::Percent, *dec, *sep)
            }
            FieldFormat::Date(f) => (d.category, d.date) = (Category::Date, f.clone()),
            FieldFormat::Time(n) => (d.category, d.time) = (Category::Time, *n),
            FieldFormat::Special(m) => (d.category, d.mask) = (Category::Special, m.clone()),
            FieldFormat::Custom => d.category = Category::Custom,
        }
        d
    }

    pub fn format(&self) -> FieldFormat {
        match self.category {
            Category::None => FieldFormat::None,
            Category::Number => FieldFormat::Number {
                dec: self.dec,
                sep: self.sep,
                neg: self.neg,
                currency: self.currency.clone(),
                prepend: self.prepend,
            },
            Category::Percent => FieldFormat::Percent {
                dec: self.dec,
                sep: self.sep,
            },
            Category::Date => FieldFormat::Date(self.date.trim().to_string()),
            Category::Time => FieldFormat::Time(self.time),
            Category::Special => FieldFormat::Special(self.mask.trim().to_string()),
            Category::Custom => FieldFormat::Custom,
        }
    }
}

/// Modo da aba Validar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidateMode {
    None,
    Range,
    Custom,
}

/// Rascunho da aba Validar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidateDraft {
    pub mode: ValidateMode,
    pub min: String,
    pub max: String,
}

impl ValidateDraft {
    pub fn from(v: &FieldValidate) -> Self {
        let text = |n: &Option<f64>| n.map_or_else(String::new, |n| n.to_string());
        let (mode, min, max) = match v {
            FieldValidate::Range { min, max } => (ValidateMode::Range, text(min), text(max)),
            FieldValidate::Custom => (ValidateMode::Custom, String::new(), String::new()),
            FieldValidate::None => (ValidateMode::None, String::new(), String::new()),
        };
        Self { mode, min, max }
    }

    pub fn validate(&self) -> FieldValidate {
        let num = |s: &str| {
            s.trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
        };
        match self.mode {
            ValidateMode::Range => FieldValidate::Range {
                min: num(&self.min),
                max: num(&self.max),
            },
            ValidateMode::Custom => FieldValidate::Custom,
            ValidateMode::None => FieldValidate::None,
        }
    }
}

/// Modo da aba Calcular.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalcMode {
    None,
    Simple,
    Sfn,
    Custom,
}

/// Rascunho da aba Calcular.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CalcDraft {
    pub mode: CalcMode,
    pub op: CalcOp,
    pub fields: Vec<String>,
    pub sfn: String,
}

impl CalcDraft {
    pub fn from(c: &FieldCalc) -> Self {
        let mut d = Self {
            mode: CalcMode::None,
            op: CalcOp::Sum,
            fields: Vec::new(),
            sfn: String::new(),
        };
        match c {
            FieldCalc::None => {}
            FieldCalc::Simple { op, fields } => {
                (d.mode, d.op, d.fields) = (CalcMode::Simple, *op, fields.clone())
            }
            FieldCalc::Sfn(e) => (d.mode, d.sfn) = (CalcMode::Sfn, e.clone()),
            // o código de barras tem as abas próprias; aqui fica como está
            FieldCalc::Custom | FieldCalc::Barcode { .. } => d.mode = CalcMode::Custom,
        }
        d
    }

    pub fn calc(&self) -> FieldCalc {
        match self.mode {
            CalcMode::None => FieldCalc::None,
            CalcMode::Simple if self.fields.is_empty() => FieldCalc::None,
            CalcMode::Simple => FieldCalc::Simple {
                op: self.op,
                fields: self.fields.clone(),
            },
            CalcMode::Sfn if self.sfn.trim().is_empty() => FieldCalc::None,
            CalcMode::Sfn => FieldCalc::Sfn(self.sfn.trim().to_string()),
            CalcMode::Custom => FieldCalc::Custom,
        }
    }
}

/// Aba aberta na janela de um campo de texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Options,
    Value,
    Actions,
    Format,
    Validate,
    Calculate,
}

/// Lista de opções numa caixa de combinação, por índice.
fn combo(ui: &mut egui::Ui, id: &str, value: &mut u8, labels: &[&str]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(labels.get(*value as usize).copied().unwrap_or(""))
        .show_ui(ui, |ui| {
            for (k, label) in labels.iter().enumerate() {
                ui.selectable_value(value, k as u8, *label);
            }
        });
}

impl FormatDraft {
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Selecionar categoria de formato:");
            let current = Category::PICK
                .iter()
                .find(|(c, _)| *c == self.category)
                .map_or("Personalizado (script)", |(_, l)| l);
            egui::ComboBox::from_id_salt("formato-categoria")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (c, l) in Category::PICK {
                        ui.selectable_value(&mut self.category, c, l);
                    }
                });
        });
        ui.separator();
        match self.category {
            Category::Number | Category::Percent => {
                egui::Grid::new("formato-numero")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Casas decimais");
                        ui.add(egui::DragValue::new(&mut self.dec).range(0..=10));
                        ui.end_row();
                        ui.label("Estilo do separador");
                        combo(ui, "formato-sep", &mut self.sep, &SEPARATORS);
                        ui.end_row();
                        if self.category == Category::Number {
                            ui.label("Símbolo da moeda");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.currency).desired_width(60.0),
                            );
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut self.prepend, "Símbolo antes do número");
                            ui.end_row();
                            ui.label("Números negativos");
                            combo(ui, "formato-neg", &mut self.neg, &NEGATIVES);
                            ui.end_row();
                        }
                    });
            }
            Category::Date => {
                presets(ui, "formato-data", &mut self.date, &DATE_PRESETS);
            }
            Category::Time => combo(ui, "formato-hora", &mut self.time, &pdf_core::TIME_FORMATS),
            Category::Special => {
                ui.horizontal(|ui| {
                    ui.label("Máscara");
                    ui.add(egui::TextEdit::singleline(&mut self.mask).desired_width(160.0));
                    ui.menu_button("⏷", |ui| {
                        for (label, mask) in SPECIAL_PRESETS {
                            if ui.button(format!("{label}  {mask}")).clicked() {
                                self.mask = mask.into();
                            }
                        }
                    });
                });
                ui.label(
                    egui::RichText::new("9 = dígito, A = letra, O = letra ou dígito, X = qualquer")
                        .small()
                        .weak(),
                );
            }
            Category::Custom => {
                ui.label("O campo tem um script de formatação que o app mantém, mas não executa.");
            }
            Category::None => {}
        }
        if self.category != Category::Custom && self.category != Category::None {
            let sample = match self.category {
                Category::Number | Category::Percent => "-1234.5",
                Category::Date => "2026-03-05",
                Category::Time => "14:05",
                _ => "",
            };
            if let Ok(f) = pdf_core::format_value(&self.format(), sample) {
                ui.label(format!("Exemplo: {}", f.text));
            }
        }
    }
}

/// Texto com uma lista de valores prontos ao lado.
fn presets(ui: &mut egui::Ui, id: &str, value: &mut String, list: &[&str]) {
    ui.horizontal(|ui| {
        ui.label("Formato");
        ui.add(egui::TextEdit::singleline(value).desired_width(160.0));
        ui.push_id(id, |ui| {
            ui.menu_button("⏷", |ui| {
                for item in list {
                    if ui.button(*item).clicked() {
                        *value = item.to_string();
                    }
                }
            });
        });
    });
}

impl ValidateDraft {
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.mode == ValidateMode::Custom {
            ui.label("O campo tem um script de validação que o app mantém, mas não executa.");
        }
        ui.radio_value(
            &mut self.mode,
            ValidateMode::None,
            "O valor do campo não é validado",
        );
        ui.radio_value(
            &mut self.mode,
            ValidateMode::Range,
            "O valor do campo está no intervalo:",
        );
        ui.add_enabled_ui(self.mode == ValidateMode::Range, |ui| {
            ui.horizontal(|ui| {
                ui.label("De");
                ui.add(egui::TextEdit::singleline(&mut self.min).desired_width(70.0));
                ui.label("até");
                ui.add(egui::TextEdit::singleline(&mut self.max).desired_width(70.0));
            });
        });
    }
}

impl CalcDraft {
    fn ui(&mut self, ui: &mut egui::Ui, names: &[String]) {
        if self.mode == CalcMode::Custom {
            ui.label("O campo tem um script de cálculo que o app mantém, mas não executa.");
        }
        ui.radio_value(&mut self.mode, CalcMode::None, "O valor não é calculado");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.mode, CalcMode::Simple, "O valor é o(a)");
            egui::ComboBox::from_id_salt("calc-op")
                .selected_text(self.op.label())
                .show_ui(ui, |ui| {
                    for op in CalcOp::ALL {
                        ui.selectable_value(&mut self.op, op, op.label());
                    }
                });
            ui.label("dos campos a seguir:");
        });
        ui.add_enabled_ui(self.mode == CalcMode::Simple, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("calc-campos")
                .max_height(110.0)
                .show(ui, |ui| {
                    for name in names {
                        let mut on = self.fields.contains(name);
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                self.fields.push(name.clone());
                            } else {
                                self.fields.retain(|f| f != name);
                            }
                        }
                    }
                });
        });
        ui.radio_value(
            &mut self.mode,
            CalcMode::Sfn,
            "Notação simplificada de campos:",
        );
        ui.add_enabled(
            self.mode == CalcMode::Sfn,
            egui::TextEdit::singleline(&mut self.sfn)
                .hint_text("Preco * Quantidade")
                .desired_width(260.0),
        );
        let expr = self.sfn.trim();
        if self.mode == CalcMode::Sfn && !expr.is_empty() && !pdf_core::sfn_ok(expr) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "A expressão não é válida: use nomes de campos, números, + - * / e parênteses.",
            );
        }
    }
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

#[cfg(test)]
mod draft_tests {
    use super::*;
    use pdf_core::{CalcOp, FieldCalc, FieldFormat, FieldValidate};

    #[test]
    fn barcode_tabs_build_the_props() {
        let props = FieldProps {
            name: "Código".into(),
            calc: FieldCalc::Barcode {
                fields: vec!["Nome".into()],
                names: true,
            },
            barcode: Some(pdf_core::BarcodeOptions::default()),
            ..Default::default()
        };
        let text = pdf_core::FieldKind::Text {
            multiline: true,
            password: false,
            max_len: None,
        };
        let names = vec!["Nome".into(), "Cidade".into(), "Código".into()];
        let mut w = FieldPropsWindow::new(0, 1, &text, props, names, 1);
        let b = w.barcode.as_mut().expect("abas do código de barras");
        b.fields.push("Cidade".into());
        b.names = false;
        b.options.symbology = pdf_core::Symbology::Pdf417;
        b.options.ecc = 5;
        let out = w.built();
        assert_eq!(
            out.calc,
            FieldCalc::Barcode {
                fields: vec!["Nome".into(), "Cidade".into()],
                names: false
            }
        );
        assert_eq!(
            out.barcode,
            Some(pdf_core::BarcodeOptions {
                symbology: pdf_core::Symbology::Pdf417,
                ecc: 5
            })
        );
    }

    #[test]
    fn options_tab_builds_the_props() {
        let props = FieldProps {
            name: "Foto".into(),
            layout: 1,
            behavior: 1,
            has_icon: true,
            ..Default::default()
        };
        let mut w = FieldPropsWindow::new(0, 1, &pdf_core::FieldKind::Button, props, vec![], 1);
        w.props.layout = 2;
        w.props.button_label = "Escolha".into();
        w.set_icon(std::path::PathBuf::from("/tmp/a.png"));
        let out = w.built();
        assert_eq!((out.layout, out.button_label.as_str()), (2, "Escolha"));
        assert_eq!(
            out.icon_path.as_deref(),
            Some(std::path::Path::new("/tmp/a.png"))
        );
        assert!(out.has_icon);
        w.clear_icon();
        let out = w.built();
        assert!(
            !out.has_icon && out.icon_path.is_none(),
            "Limpar tira o ícone"
        );
    }

    #[test]
    fn props_tabs_build_the_right_ops() {
        let mut f = FormatDraft::from(&FieldFormat::None);
        f.category = Category::Number;
        f.dec = 2;
        f.sep = 2;
        f.currency = "R$ ".into();
        assert_eq!(
            f.format(),
            FieldFormat::Number {
                dec: 2,
                sep: 2,
                neg: 0,
                currency: "R$ ".into(),
                prepend: true
            }
        );
        f.category = Category::Special;
        f.mask = SPECIAL_PRESETS[0].1.into();
        assert_eq!(f.format(), FieldFormat::Special("99999-999".into()));
        let back = FormatDraft::from(&FieldFormat::Date("dd/mm/yyyy".into()));
        assert_eq!(back.category, Category::Date);
        assert_eq!(back.format(), FieldFormat::Date("dd/mm/yyyy".into()));
        // script do arquivo que o app não entende continua como estava
        assert_eq!(
            FormatDraft::from(&FieldFormat::Custom).format(),
            FieldFormat::Custom
        );

        let mut v = ValidateDraft::from(&FieldValidate::None);
        v.mode = ValidateMode::Range;
        v.min = "0".into();
        assert_eq!(
            v.validate(),
            FieldValidate::Range {
                min: Some(0.0),
                max: None
            }
        );

        let mut c = CalcDraft::from(&FieldCalc::None);
        c.mode = CalcMode::Simple;
        c.op = CalcOp::Sum;
        c.fields = vec!["A".into(), "B".into()];
        assert_eq!(
            c.calc(),
            FieldCalc::Simple {
                op: CalcOp::Sum,
                fields: vec!["A".into(), "B".into()]
            }
        );
        c.mode = CalcMode::Sfn;
        c.sfn = " A * 2 ".into();
        assert_eq!(c.calc(), FieldCalc::Sfn("A * 2".into()));
    }
}
