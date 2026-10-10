//! Criação de caixa de texto, formas, linhas e carimbos (gestos com pré-visualização).
use crate::edit::{screen_rect, EditState, TextDraft, TextTarget};
use crate::shell::annot_props::color32;
use app_state::annot_geom::{rect_from_drag, snap_45};
use app_state::stamps::{subtitle, STAMP_HEIGHT, STAMP_WIDTH};
use app_state::tools::Tool;
use app_state::transform::PageMap;
use pdf_core::{NewAnnot, PdfRect, COMMENT_RED};

/// Tamanho da caixa de texto criada com um clique (sem arrastar), em pontos.
const TEXT_BOX: (f32, f32) = (200.0, 24.0);
/// Tamanho do texto da caixa nova.
pub(crate) const TEXT_SIZE: f32 = 12.0;

/// Arrasto de criação em andamento (formas, linhas e caixa de texto).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShapeDrag {
    page: i32,
    from: (f32, f32),
    to: (f32, f32),
    shift: bool,
}

/// Hora local para o carimbo dinâmico: (ano, mês, dia, hora, minuto).
pub(crate) fn local_now() -> (i32, u32, u32, u32, u32) {
    use chrono::{Datelike, Timelike};
    let t = chrono::Local::now();
    (t.year(), t.month(), t.day(), t.hour(), t.minute())
}

impl EditState {
    /// Fim do arrasto de Retângulo/Elipse/Linha/Seta: cria a anotação (as minúsculas são
    /// descartadas por `NewAnnot::is_empty`).
    pub(crate) fn finish_shape(
        &mut self,
        tool: Tool,
        page: i32,
        a: (f32, f32),
        b: (f32, f32),
        shift: bool,
    ) {
        let annot = match tool {
            Tool::Rect | Tool::Ellipse => NewAnnot::Shape {
                page,
                rect: rect_from_drag(a, b, shift),
                ellipse: tool == Tool::Ellipse,
            },
            Tool::RedactArea => NewAnnot::Redact {
                page,
                rects: vec![rect_from_drag(a, b, false)],
            },
            Tool::Line | Tool::Arrow => NewAnnot::Line {
                page,
                a,
                b: if shift { snap_45(a, b) } else { b },
                arrow: tool == Tool::Arrow,
            },
            _ => return,
        };
        self.add(annot);
    }

    /// Carimbo escolhido centrado em `at`. O dinâmico precisa do nome: sem ele devolve `false`
    /// (a aba pede a Identidade e chama de novo).
    pub(crate) fn place_stamp(
        &mut self,
        page: i32,
        at: (f32, f32),
        author: Option<&str>,
        now: (i32, u32, u32, u32, u32),
    ) -> bool {
        let kind = self.stamp;
        let sub = match (kind.is_dynamic(), author) {
            (false, _) => None,
            (true, Some(name)) if !name.trim().is_empty() => Some(subtitle(name, now)),
            (true, _) => return false,
        };
        let (w, h) = (STAMP_WIDTH, STAMP_HEIGHT);
        self.add(NewAnnot::Stamp {
            page,
            rect: PdfRect::new(
                at.0 - w / 2.0,
                at.1 - h / 2.0,
                at.0 + w / 2.0,
                at.1 + h / 2.0,
            ),
            title: kind.title().into(),
            subtitle: sub,
            color: kind.color(),
        });
        true
    }

    /// Gestos das ferramentas de criação sobre uma página. Devolve o ponto clicado com a ferramenta
    /// Carimbo (a aba o coloca, pedindo a Identidade se precisar).
    pub(crate) fn create_gestures(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        pointer: Option<(f32, f32)>,
        page: i32,
        map: &PageMap,
    ) -> Option<(f32, f32)> {
        let shift = ui.input(|i| i.modifiers.shift);
        if self.tool == Tool::Stamp {
            return pointer.filter(|_| response.clicked());
        }
        // com uma caixa em digitação, clicar fora só termina a digitação (como no Acrobat)
        if self.text_draft.is_some() {
            if response.clicked() || response.drag_started() {
                self.finish_text();
            }
            return None;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = crate::edit::press_point(response, map, pointer) {
                self.shape_drag = Some(ShapeDrag {
                    page,
                    from: p,
                    to: p,
                    shift,
                });
            }
        }
        if let Some(drag) = self.shape_drag.as_mut().filter(|d| d.page == page) {
            if let Some(p) = pointer {
                drag.to = p;
            }
            drag.shift = shift;
            if response.drag_stopped_by(egui::PointerButton::Primary) {
                let d = *drag;
                self.shape_drag = None;
                if self.tool == Tool::TextBox {
                    self.start_text_box(page, rect_from_drag(d.from, d.to, false));
                } else {
                    self.finish_shape(self.tool, page, d.from, d.to, d.shift);
                }
            }
        } else if self.tool == Tool::TextBox && response.clicked() {
            if let Some(p) = pointer {
                let r = PdfRect::new(p.0, p.1, p.0 + TEXT_BOX.0, p.1 + TEXT_BOX.1);
                self.start_text_box(page, r);
            }
        }
        None
    }

    /// Abre a digitação de uma caixa de texto nova (um arrasto pequeno vira o tamanho padrão).
    fn start_text_box(&mut self, page: i32, rect: PdfRect) {
        let rect = if rect.width() < 20.0 || rect.height() < 10.0 {
            PdfRect::new(rect.x0, rect.y0, rect.x0 + TEXT_BOX.0, rect.y0 + TEXT_BOX.1)
        } else {
            rect
        };
        self.text_draft = Some(TextDraft {
            page,
            rect,
            target: TextTarget::New,
            text: String::new(),
            size: TEXT_SIZE,
            color: COMMENT_RED,
            focused: false,
            fill: false,
        });
    }

    /// Campo de digitação da caixa de texto, posto sobre a página. Clicar fora ou Esc termina:
    /// a caixa nova vazia é descartada; a existente só muda se o texto mudou.
    pub(crate) fn text_overlay(&mut self, ui: &mut egui::Ui, page: i32, map: &PageMap) {
        let Some(d) = self.text_draft.as_mut().filter(|d| d.page == page) else {
            return;
        };
        // a caixa cresce para baixo conforme o texto ganha linhas (como no Acrobat)
        let font = egui::FontId::proportional(d.size * map.zoom);
        let wrap = screen_rect(&d.rect, map).width() - 4.0;
        let text_h = ui.ctx().fonts_mut(|f| {
            f.layout(d.text.clone(), font.clone(), egui::Color32::BLACK, wrap)
                .size()
                .y
        });
        let needed = (text_h + 6.0) / map.zoom;
        if d.rect.height() < needed {
            d.rect.y1 = d.rect.y0 + needed;
        }
        let r = screen_rect(&d.rect, map);
        let color = color32(d.color);
        ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
        ui.painter().rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
        let field = egui::TextEdit::multiline(&mut d.text)
            .font(font)
            .text_color(color)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::same(2))
            .desired_width(r.width() - 4.0)
            .id(egui::Id::new(("caixa-de-texto", page)));
        let resp = ui.put(r, field);
        // o clique que criou a caixa pode tirar o foco no mesmo quadro: insiste até pegar
        if resp.has_focus() {
            d.focused = true;
        } else if !d.focused {
            resp.request_focus();
        } else {
            self.finish_text();
        }
    }

    /// Termina a digitação da caixa de texto.
    pub(crate) fn finish_text(&mut self) {
        let Some(d) = self.text_draft.take() else {
            return;
        };
        match d.target {
            TextTarget::New if d.fill => self.add(NewAnnot::FillText {
                page: d.page,
                at: (d.rect.x0, d.rect.y0),
                text: d.text,
                size: d.size,
            }),
            TextTarget::New => self.add(NewAnnot::TextBox {
                page: d.page,
                rect: d.rect,
                text: d.text,
                size: d.size,
                color: d.color,
            }),
            TextTarget::Existing { id, original } => {
                if !d.text.trim().is_empty() && d.text != original {
                    self.push_op(pdf_core::AnnotOp::SetContents {
                        page: d.page,
                        id,
                        text: d.text,
                    });
                }
            }
        }
    }

    /// Pré-visualização do arrasto de criação nesta página.
    pub(crate) fn paint_creation(&self, painter: &egui::Painter, page: i32, map: &PageMap) {
        let Some(d) = self.shape_drag.filter(|d| d.page == page) else {
            return;
        };
        let stroke = egui::Stroke::new(1.5, color32(COMMENT_RED));
        match self.tool {
            Tool::Rect | Tool::TextBox | Tool::RedactArea => {
                let r = screen_rect(
                    &rect_from_drag(d.from, d.to, d.shift && self.tool == Tool::Rect),
                    map,
                );
                painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
            }
            Tool::Ellipse => {
                let r = screen_rect(&rect_from_drag(d.from, d.to, d.shift), map);
                painter.add(egui::Shape::ellipse_stroke(
                    r.center(),
                    r.size() / 2.0,
                    stroke,
                ));
            }
            Tool::Line | Tool::Arrow => {
                let b = if d.shift { snap_45(d.from, d.to) } else { d.to };
                let (p, q): (egui::Pos2, egui::Pos2) =
                    (map.to_screen(d.from).into(), map.to_screen(b).into());
                if self.tool == Tool::Arrow {
                    painter.arrow(p, q - p, stroke);
                } else {
                    painter.line_segment([p, q], stroke);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_state::stamps::StampKind;
    use pdf_core::AnnotOp;

    #[test]
    fn tiny_shape_drags_create_nothing() {
        let mut e = EditState::default();
        e.finish_shape(Tool::Rect, 0, (10.0, 10.0), (11.0, 11.5), false);
        e.finish_shape(Tool::Line, 0, (10.0, 10.0), (11.0, 11.0), false);
        assert!(e.take_ops().is_empty());
    }

    #[test]
    fn shapes_follow_the_tool_and_shift() {
        let mut e = EditState::default();
        e.finish_shape(Tool::Ellipse, 1, (0.0, 0.0), (40.0, 20.0), true);
        e.finish_shape(Tool::Arrow, 1, (0.0, 0.0), (100.0, 7.0), true);
        let ops = e.take_ops();
        assert!(matches!(
            &ops[..],
            [
                AnnotOp::Create(NewAnnot::Shape { ellipse: true, rect, .. }),
                AnnotOp::Create(NewAnnot::Line { arrow: true, b, .. }),
            ] if rect.width() == 40.0 && rect.height() == 40.0 && b.1 == 0.0
        ));
    }

    #[test]
    fn dynamic_stamps_wait_for_a_name() {
        let mut e = EditState::default();
        e.stamp = StampKind::DynApproved;
        assert!(!e.place_stamp(0, (200.0, 200.0), None, (2026, 10, 9, 14, 30)));
        assert!(e.take_ops().is_empty(), "sem nome: pede a identidade antes");
        assert!(e.place_stamp(0, (200.0, 200.0), Some("Maria"), (2026, 10, 9, 14, 30)));
        assert!(matches!(
            &e.take_ops()[..],
            [AnnotOp::Create(NewAnnot::Stamp { subtitle: Some(s), rect, .. })]
                if s == "Maria, 09/10/2026 14:30" && rect.x0 == 105.0 && rect.y0 == 170.0
        ));
    }

    #[test]
    fn standard_stamps_need_no_name() {
        let mut e = EditState::default();
        e.stamp = StampKind::Confidential;
        assert!(e.place_stamp(2, (100.0, 100.0), None, (2026, 1, 1, 0, 0)));
        assert!(matches!(
            &e.take_ops()[..],
            [AnnotOp::Create(NewAnnot::Stamp { page: 2, subtitle: None, title, .. })] if title == "CONFIDENCIAL"
        ));
    }
}
