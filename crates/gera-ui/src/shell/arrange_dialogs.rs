//! Janelas do Preparar formulário: Duplicar (o campo em outras páginas) e Criar várias cópias (uma
//! grade de campos novos), como no Acrobat.
use app_state::arrange::copies_grid;
use pdf_core::{PdfRect, PrepareOp};

/// Janela aberta e o campo de que ela trata.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrangeDialog {
    Duplicate {
        page: i32,
        id: i32,
        all: bool,
        /// Intervalo (1 = primeira página).
        from: u32,
        to: u32,
    },
    Copies {
        page: i32,
        id: i32,
        rect: PdfRect,
        down: usize,
        across: usize,
        /// Espaço entre as cópias (pontos): horizontal e vertical.
        gap: (f32, f32),
    },
}

impl ArrangeDialog {
    pub fn duplicate(page: i32, id: i32, pages: usize) -> Self {
        Self::Duplicate {
            page,
            id,
            all: true,
            from: 1,
            to: pages.max(1) as u32,
        }
    }

    pub fn copies(page: i32, id: i32, rect: PdfRect) -> Self {
        Self::Copies {
            page,
            id,
            rect,
            down: 2,
            across: 1,
            gap: (0.0, 0.0),
        }
    }

    /// A operação que a janela pede, com os valores atuais.
    pub fn op(&self, pages: usize) -> Option<PrepareOp> {
        match *self {
            Self::Duplicate {
                page,
                id,
                all,
                from,
                to,
            } => {
                let last = pages as u32;
                let (a, b) = if all {
                    (1, last)
                } else {
                    (from.min(to), from.max(to))
                };
                let list: Vec<i32> = (a.max(1)..=b.min(last)).map(|p| p as i32 - 1).collect();
                (!list.is_empty()).then_some(PrepareOp::Duplicate {
                    page,
                    id,
                    pages: list,
                    offset: (0.0, 0.0),
                })
            }
            Self::Copies { page, id, .. } => {
                let rects = self.preview();
                (!rects.is_empty()).then_some(PrepareOp::Copies { page, id, rects })
            }
        }
    }

    /// Onde as cópias vão ficar (a prévia desenhada na página).
    pub fn preview(&self) -> Vec<PdfRect> {
        match *self {
            Self::Copies {
                rect,
                down,
                across,
                gap,
                ..
            } => copies_grid(rect, down, across, gap),
            Self::Duplicate { .. } => Vec::new(),
        }
    }

    /// Página da prévia.
    pub fn page(&self) -> i32 {
        match *self {
            Self::Duplicate { page, .. } | Self::Copies { page, .. } => page,
        }
    }
}

/// Desenha a janela. Devolve `Some(op)` ao confirmar e `Some(None)` ao cancelar.
pub fn show(
    ctx: &egui::Context,
    dialog: &mut ArrangeDialog,
    pages: usize,
) -> Option<Option<PrepareOp>> {
    let mut open = true;
    let mut done = None;
    let title = match dialog {
        ArrangeDialog::Duplicate { .. } => "Duplicar campo",
        ArrangeDialog::Copies { .. } => "Criar várias cópias de campos",
    };
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            match dialog {
                ArrangeDialog::Duplicate { all, from, to, .. } => {
                    ui.label("Duplicar o campo nas páginas:");
                    ui.radio_value(all, true, "Todas");
                    ui.horizontal(|ui| {
                        ui.radio_value(all, false, "De");
                        let max = pages.max(1) as u32;
                        ui.add_enabled(!*all, egui::DragValue::new(from).range(1..=max));
                        ui.label("a");
                        ui.add_enabled(!*all, egui::DragValue::new(to).range(1..=max));
                    });
                    ui.label("As cópias mostram o mesmo valor (são o mesmo campo).");
                }
                ArrangeDialog::Copies {
                    down, across, gap, ..
                } => {
                    egui::Grid::new("copias").num_columns(2).show(ui, |ui| {
                        ui.label("Copiar o campo para baixo:");
                        ui.add(egui::DragValue::new(down).range(1..=20));
                        ui.end_row();
                        ui.label("Copiar o campo para o lado:");
                        ui.add(egui::DragValue::new(across).range(1..=20));
                        ui.end_row();
                        ui.label("Espaço horizontal (pt):");
                        ui.add(egui::DragValue::new(&mut gap.0).range(0.0..=200.0));
                        ui.end_row();
                        ui.label("Espaço vertical (pt):");
                        ui.add(egui::DragValue::new(&mut gap.1).range(0.0..=200.0));
                        ui.end_row();
                    });
                    ui.label("A prévia aparece na página.");
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let op = dialog.op(pages);
                if ui
                    .add_enabled(op.is_some(), egui::Button::new("OK"))
                    .clicked()
                {
                    done = Some(op);
                }
                if ui.button("Cancelar").clicked() {
                    done = Some(None);
                }
            });
        });
    if !open {
        done = Some(None);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_range_and_copies_grid() {
        let mut d = ArrangeDialog::duplicate(0, 7, 3);
        assert_eq!(
            d.op(3),
            Some(PrepareOp::Duplicate {
                page: 0,
                id: 7,
                pages: vec![0, 1, 2],
                offset: (0.0, 0.0)
            })
        );
        if let ArrangeDialog::Duplicate { all, from, to, .. } = &mut d {
            (*all, *from, *to) = (false, 3, 2);
        }
        assert!(matches!(d.op(3), Some(PrepareOp::Duplicate { pages, .. }) if pages == [1, 2]));
        let c = ArrangeDialog::copies(0, 7, PdfRect::new(0.0, 0.0, 10.0, 5.0));
        assert_eq!(
            c.preview(),
            vec![PdfRect::new(0.0, 5.0, 10.0, 10.0)],
            "padrão: 2 para baixo"
        );
    }
}
