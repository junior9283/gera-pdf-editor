//! Janela "Assinatura"/"Iniciais" do Preencher e assinar (como no Acrobat): Digitar, Desenhar ou
//! Imagem; "Aplicar" guarda para reutilizar.
use crate::fill_sign::{paint_saved, script_family};
use crate::open_dialog::{FileDialogs, Picked};
use app_state::fill_sign::{self as fs, Saved};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Type,
    Draw,
    Image,
}

pub struct SignatureDialog {
    pub initials: bool,
    tab: Tab,
    text: String,
    /// Traços desenhados, em pixels da área de desenho.
    strokes: Vec<Vec<(f32, f32)>>,
    image: Option<PathBuf>,
    dialogs: FileDialogs,
    error: String,
}

/// Iniciais de um nome ("Ana Maria Souza" → "AMS").
pub fn initials_of(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

const CANVAS: egui::Vec2 = egui::vec2(440.0, 150.0);

impl SignatureDialog {
    pub fn new(initials: bool, author: Option<&str>) -> Self {
        let name = author.unwrap_or_default().trim();
        Self {
            initials,
            tab: Tab::Type,
            text: if initials {
                initials_of(name)
            } else {
                name.to_owned()
            },
            strokes: Vec::new(),
            image: None,
            dialogs: FileDialogs::default(),
            error: String::new(),
        }
    }

    /// O que "Aplicar" guardaria agora (`None`: ainda não há assinatura).
    fn result(&self) -> Option<Saved> {
        match self.tab {
            Tab::Type => (!self.text.trim().is_empty()).then(|| Saved::Typed {
                text: self.text.trim().to_owned(),
            }),
            Tab::Draw => {
                let smooth: Vec<_> = self.strokes.iter().map(|s| fs::smooth(s)).collect();
                fs::normalize(&smooth).map(|(strokes, aspect)| Saved::Drawn { strokes, aspect })
            }
            Tab::Image => self.image.clone().map(|file| Saved::Image { file }),
        }
    }

    /// Desenha a janela. `Some(Some(_))`: aplicou; `Some(None)`: cancelou; `None`: segue aberta.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<Saved>> {
        if let Some(Picked::Open(files)) = self.dialogs.poll() {
            if let Some(file) = files.into_iter().next() {
                match pdf_core::signature::image_aspect(&file) {
                    Ok(_) => {
                        self.image = Some(file);
                        self.error.clear();
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
        }
        let title = if self.initials {
            "Iniciais"
        } else {
            "Assinatura"
        };
        let mut out = None;
        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| out = self.contents(ui));
        if !open {
            return Some(None);
        }
        out
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> Option<Option<Saved>> {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Type, "Digitar");
            ui.selectable_value(&mut self.tab, Tab::Draw, "Desenhar");
            ui.selectable_value(&mut self.tab, Tab::Image, "Imagem");
        });
        ui.separator();
        match self.tab {
            Tab::Type => {
                ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .hint_text(if self.initials { "Iniciais" } else { "Nome" })
                        .desired_width(CANVAS.x),
                );
                let (r, painter) = canvas(ui);
                if !self.text.trim().is_empty() {
                    painter.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        self.text.trim(),
                        egui::FontId::new(64.0, script_family()),
                        egui::Color32::BLACK,
                    );
                }
            }
            Tab::Draw => {
                let (r, painter, response) = draw_area(ui);
                if response.drag_started() {
                    self.strokes.push(Vec::new());
                }
                if response.dragged() {
                    if let (Some(p), Some(s)) =
                        (response.interact_pointer_pos(), self.strokes.last_mut())
                    {
                        let local = p - r.min;
                        if s.last() != Some(&(local.x, local.y)) {
                            s.push((local.x, local.y));
                        }
                    }
                }
                let stroke = egui::Stroke::new(2.5, egui::Color32::BLACK);
                for s in &self.strokes {
                    let pts = s.iter().map(|&(x, y)| r.min + egui::vec2(x, y)).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
                if ui.button("Limpar").clicked() {
                    self.strokes.clear();
                }
            }
            Tab::Image => {
                ui.horizontal(|ui| {
                    if ui.button("Escolher imagem…").clicked() {
                        self.dialogs.pick_image(ui.ctx());
                    }
                    if let Some(f) = &self.image {
                        ui.label(
                            f.file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        );
                    }
                });
                let (r, painter) = canvas(ui);
                if let Some(saved) = self.result() {
                    paint_saved(&painter, &saved, r.shrink(20.0), egui::Color32::DARK_GRAY);
                }
            }
        }
        if !self.error.is_empty() {
            ui.colored_label(ui.visuals().warn_fg_color, &self.error);
        }
        ui.separator();
        let mut out = None;
        ui.horizontal(|ui| {
            let result = self.result();
            if ui
                .add_enabled(result.is_some(), egui::Button::new("Aplicar"))
                .clicked()
            {
                out = self.finish(result);
            }
            if ui.button("Cancelar").clicked() {
                out = Some(None);
            }
        });
        out
    }

    /// Aplicar: a imagem é copiada para a pasta de configuração (o original pode sumir depois);
    /// texto sem nenhuma letra da fonte vira aviso.
    fn finish(&mut self, result: Option<Saved>) -> Option<Option<Saved>> {
        let saved = result?;
        if saved.art().is_none() {
            self.error = "Não foi possível montar a assinatura com esse texto.".into();
            return None;
        }
        match saved {
            Saved::Image { file } => match copy_image(&file, self.initials) {
                Ok(copy) => Some(Some(Saved::Image { file: copy })),
                Err(e) => {
                    self.error = format!("Não foi possível guardar a imagem: {e}");
                    None
                }
            },
            other => Some(Some(other)),
        }
    }
}

fn copy_image(file: &std::path::Path, initials: bool) -> std::io::Result<PathBuf> {
    let ext = file
        .extension()
        .map_or_else(|| "png".into(), |e| e.to_string_lossy().to_lowercase());
    let name = format!("{}.{ext}", if initials { "iniciais" } else { "assinatura" });
    let dest = app_state::store::config_file(&name)
        .ok_or_else(|| std::io::Error::other("pasta de configuração indisponível"))?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::copy(file, &dest)?;
    Ok(dest)
}

/// Área branca de prévia, com a linha de base da assinatura.
fn canvas(ui: &mut egui::Ui) -> (egui::Rect, egui::Painter) {
    let (resp, painter) = ui.allocate_painter(CANVAS, egui::Sense::hover());
    paint_paper(&painter, resp.rect);
    (resp.rect, painter)
}

fn draw_area(ui: &mut egui::Ui) -> (egui::Rect, egui::Painter, egui::Response) {
    let (resp, painter) = ui.allocate_painter(CANVAS, egui::Sense::drag());
    paint_paper(&painter, resp.rect);
    let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
    (resp.rect, painter, resp)
}

fn paint_paper(painter: &egui::Painter, r: egui::Rect) {
    painter.rect_filled(r, 4.0, egui::Color32::WHITE);
    let y = r.bottom() - 30.0;
    painter.line_segment(
        [
            egui::pos2(r.left() + 20.0, y),
            egui::pos2(r.right() - 20.0, y),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_gray(190)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_come_from_the_name() {
        assert_eq!(initials_of("ana maria  souza"), "AMS");
        assert_eq!(initials_of(""), "");
    }

    #[test]
    fn a_drawing_needs_ink_to_apply() {
        let mut d = SignatureDialog::new(false, None);
        d.tab = Tab::Draw;
        assert!(d.result().is_none());
        d.strokes = vec![vec![(5.0, 5.0)]];
        assert!(d.result().is_none(), "um ponto só não é assinatura");
        d.strokes = vec![vec![(5.0, 5.0), (80.0, 40.0), (150.0, 10.0)]];
        assert!(matches!(d.result(), Some(Saved::Drawn { .. })));
    }

    #[test]
    fn typed_signature_starts_with_the_identity_name() {
        assert_eq!(
            SignatureDialog::new(false, Some(" Ana Lima ")).text,
            "Ana Lima"
        );
        assert_eq!(SignatureDialog::new(true, Some("Ana Lima")).text, "AL");
    }
}
