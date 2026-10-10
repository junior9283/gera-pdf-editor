//! Preencher e assinar (como no Acrobat): texto solto, marcas e assinatura/iniciais colocados com um
//! clique, com prévia no cursor.
use crate::edit::{screen_rect, EditState, TextDraft, TextTarget};
use crate::shell::signature_dialog::SignatureDialog;
use app_state::fill_sign::{self as fs, FillTool, Saved, SavedSignatures};
use app_state::store;
use app_state::tools::Tool;
use app_state::transform::PageMap;
use pdf_core::{Mark, NewAnnot, PdfRect};
use std::sync::Mutex;

/// Estado do Preencher e assinar de uma aba.
pub struct FillState {
    /// A linha Preencher e assinar está aberta (a marca redimensionada vira o tamanho padrão).
    pub active: bool,
    pub tool: Option<FillTool>,
    pub mark_size: f32,
    pub text_size: f32,
    pub dialog: Option<SignatureDialog>,
    /// A janela foi aberta por um campo de assinatura: (página, retângulo do campo).
    pub into_field: Option<(i32, PdfRect)>,
}

impl Default for FillState {
    fn default() -> Self {
        Self {
            active: false,
            tool: None,
            mark_size: fs::MARK_SIZE,
            text_size: fs::TEXT_SIZE,
            dialog: None,
            into_field: None,
        }
    }
}

/// Assinatura e iniciais guardadas, lidas do disco uma vez e compartilhadas pelas abas.
static SAVED: Mutex<Option<SavedSignatures>> = Mutex::new(None);

pub fn with_saved<R>(f: impl FnOnce(&SavedSignatures) -> R) -> R {
    let mut cache = SAVED.lock().unwrap_or_else(|e| e.into_inner());
    let saved = cache.get_or_insert_with(|| {
        store::config_file(fs::FILE)
            .map_or_else(SavedSignatures::default, |p| SavedSignatures::load(&p))
    });
    f(saved)
}

/// Troca a assinatura (ou as iniciais) guardada e grava no disco.
pub fn store_saved(initials: bool, saved: Option<Saved>) -> std::io::Result<()> {
    let mut all = with_saved(Clone::clone);
    all.set(initials, saved);
    if let Some(path) = store::config_file(fs::FILE) {
        all.save(&path)?;
    }
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(all);
    Ok(())
}

fn height_for(initials: bool) -> f32 {
    if initials {
        fs::INITIALS_HEIGHT
    } else {
        fs::SIGNATURE_HEIGHT
    }
}

impl EditState {
    /// Escolhe a ferramenta do Preencher e assinar (`None` volta a Selecionar).
    pub fn set_fill_tool(&mut self, tool: Option<FillTool>) {
        self.fill.tool = tool;
        self.tool = if tool.is_some() {
            Tool::Fill
        } else {
            Tool::Select
        };
        self.selected = None;
    }

    /// Gestos com uma ferramenta do Preencher e assinar: o clique coloca o item; o cursor mostra
    /// a prévia.
    pub(crate) fn fill_gestures(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        page: i32,
        map: &PageMap,
    ) {
        // com um texto em digitação, clicar fora só termina a digitação (como no Acrobat)
        if self.text_draft.is_some() {
            if response.clicked() || response.drag_started() {
                self.finish_text();
            }
            return;
        }
        let Some(tool) = self.fill.tool else {
            return;
        };
        if let (true, Some(at)) = (response.clicked(), pointer) {
            self.fill_place(tool, page, at);
            return;
        }
        if let Some(hover) = response.hover_pos() {
            let at = map.to_page(hover.into());
            paint_ghost(ui, tool, &self.fill, at, map);
        }
    }

    fn fill_place(&mut self, tool: FillTool, page: i32, at: (f32, f32)) {
        match tool {
            FillTool::Text => {
                let size = self.fill.text_size;
                self.text_draft = Some(TextDraft {
                    page,
                    rect: PdfRect::new(at.0, at.1, at.0 + 160.0, at.1 + size * 1.4),
                    target: TextTarget::New,
                    text: String::new(),
                    size,
                    color: [0.0; 3],
                    focused: false,
                    fill: true,
                });
            }
            FillTool::Mark(mark) => self.add(NewAnnot::Mark {
                page,
                rect: fs::mark_rect(mark, at, self.fill.mark_size),
                mark,
            }),
            FillTool::Signature | FillTool::Initials => {
                let initials = tool == FillTool::Initials;
                match with_saved(|s| s.get(initials).and_then(Saved::art)) {
                    Some(art) => {
                        let rect = fs::art_rect(at, fs::aspect_of(&art), height_for(initials));
                        self.add(NewAnnot::Signature {
                            page,
                            rect,
                            art,
                            initials,
                        });
                        // como no Acrobat, a assinatura é colocada uma vez
                        self.set_fill_tool(None);
                    }
                    None => self.open_signature_dialog(initials, None),
                }
            }
        }
    }

    /// Janela para criar a assinatura (ou as iniciais); `field`: colocá-la nesse campo depois.
    pub fn open_signature_dialog(&mut self, initials: bool, field: Option<(i32, PdfRect)>) {
        self.fill.dialog = Some(SignatureDialog::new(initials, self.author.as_deref()));
        self.fill.into_field = field;
    }

    /// Campo de assinatura clicado: a assinatura guardada, ajustada ao campo (ou a janela).
    pub fn sign_field(&mut self, page: i32, field: PdfRect) {
        match with_saved(|s| s.get(false).and_then(Saved::art)) {
            Some(art) => {
                let rect = fs::fit_in(field, fs::aspect_of(&art));
                self.add(NewAnnot::Signature {
                    page,
                    rect,
                    art,
                    initials: false,
                });
            }
            None => self.open_signature_dialog(false, Some((page, field))),
        }
    }

    /// Mostra a janela de assinatura, se aberta. Ao aplicar, guarda e já coloca no campo que a
    /// abriu, ou entra no modo de colocar.
    pub fn signature_window(&mut self, ctx: &egui::Context) -> Option<String> {
        let dialog = self.fill.dialog.as_mut()?;
        let initials = dialog.initials;
        let outcome = dialog.show(ctx)?;
        self.fill.dialog = None;
        let saved = outcome?;
        if let Err(e) = store_saved(initials, Some(saved)) {
            return Some(format!("Não foi possível guardar a assinatura: {e}"));
        }
        match self.fill.into_field.take() {
            Some((page, field)) => self.sign_field(page, field),
            None => self.set_fill_tool(Some(if initials {
                FillTool::Initials
            } else {
                FillTool::Signature
            })),
        }
        None
    }
}

/// Prévia do que o clique vai colocar, centrada no cursor.
fn paint_ghost(ui: &egui::Ui, tool: FillTool, st: &FillState, at: (f32, f32), map: &PageMap) {
    let painter = ui.painter();
    let ink = egui::Color32::from_black_alpha(140);
    let stroke = egui::Stroke::new(1.5, ink);
    match tool {
        FillTool::Text => {
            let p = map.to_screen(at);
            painter.text(
                egui::pos2(p.0, p.1),
                egui::Align2::LEFT_TOP,
                "Ab",
                egui::FontId::proportional(st.text_size * map.zoom),
                ink,
            );
        }
        FillTool::Mark(mark) => {
            let r = screen_rect(&fs::mark_rect(mark, at, st.mark_size), map);
            let at = |fx: f32, fy: f32| r.min + egui::vec2(fx * r.width(), fy * r.height());
            match mark {
                Mark::Cross => {
                    painter.line_segment([at(0.0, 0.0), at(1.0, 1.0)], stroke);
                    painter.line_segment([at(1.0, 0.0), at(0.0, 1.0)], stroke);
                }
                Mark::Check => {
                    painter.add(egui::Shape::line(
                        vec![at(0.0, 0.55), at(0.35, 0.95), at(1.0, 0.05)],
                        stroke,
                    ));
                }
                Mark::Line => {
                    painter.line_segment([at(0.0, 0.5), at(1.0, 0.5)], stroke);
                }
                Mark::Circle => {
                    painter.circle_stroke(r.center(), r.width().min(r.height()) / 2.0, stroke);
                }
                Mark::Dot => {
                    painter.circle_filled(r.center(), r.width().min(r.height()) / 2.0, ink);
                }
            }
        }
        FillTool::Signature | FillTool::Initials => {
            let initials = tool == FillTool::Initials;
            with_saved(|s| {
                let Some(saved) = s.get(initials) else {
                    return;
                };
                let aspect = match saved {
                    Saved::Drawn { aspect, .. } => *aspect,
                    // a prévia do digitado usa a fonte manuscrita do egui; a medida exata vem ao
                    // gravar
                    Saved::Typed { text } => 0.45 * text.chars().count().max(1) as f32,
                    Saved::Image { file } => pdf_core::signature::image_aspect(file).unwrap_or(2.0),
                };
                let r = screen_rect(&fs::art_rect(at, aspect, height_for(initials)), map);
                paint_saved(painter, saved, r, ink);
            });
        }
    }
}

/// Desenha a assinatura guardada no retângulo da tela `r` (prévias e menu).
pub fn paint_saved(painter: &egui::Painter, saved: &Saved, r: egui::Rect, ink: egui::Color32) {
    match saved {
        Saved::Typed { text } => paint_script(painter, text, r, ink),
        Saved::Drawn { strokes, .. } => {
            let stroke = egui::Stroke::new((r.height() * 0.045).max(1.0), ink);
            for s in strokes {
                let pts = s
                    .iter()
                    .map(|&(x, y)| r.min + egui::vec2(x * r.height(), y * r.height()))
                    .collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
        Saved::Image { .. } => {
            painter.rect_stroke(
                r,
                2.0,
                egui::Stroke::new(1.0, ink),
                egui::StrokeKind::Inside,
            );
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "Imagem",
                egui::FontId::proportional(12.0),
                ink,
            );
        }
    }
}

/// Texto na fonte manuscrita, o maior que couber em `r`, centrado.
pub fn paint_script(painter: &egui::Painter, text: &str, r: egui::Rect, ink: egui::Color32) {
    let font = |size| egui::FontId::new(size, script_family());
    let size = r.height() * 0.8;
    let width = painter
        .layout_no_wrap(text.to_owned(), font(size), ink)
        .size()
        .x;
    let size = if width > r.width() {
        size * r.width() / width
    } else {
        size
    };
    painter.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font(size),
        ink,
    );
}

/// Família de fonte manuscrita registrada no egui (a mesma que vai para o PDF).
pub fn script_family() -> egui::FontFamily {
    egui::FontFamily::Name("assinatura".into())
}

/// Registra a fonte manuscrita no egui (prévia da assinatura digitada).
pub fn install_script_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "assinatura".into(),
        std::sync::Arc::new(egui::FontData::from_static(
            pdf_core::signature::SCRIPT_FONT,
        )),
    );
    fonts
        .families
        .insert(script_family(), vec!["assinatura".into()]);
    ctx.set_fonts(fonts);
}
