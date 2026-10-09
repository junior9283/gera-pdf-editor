use crate::bench::Bench;
use crate::doc_tab::DocTab;
use std::path::PathBuf;

pub struct GeraApp {
    doc: DocTab,
    /// Janela "descartar alterações?" aberta (o usuário tentou fechar com anotações pendentes).
    confirm_close: bool,
    /// Modo de medição (`--bench-open` / `--bench-scroll`).
    bench: Option<Bench>,
}

impl GeraApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        bench: Option<Bench>,
    ) -> Self {
        let mut doc = DocTab::new(&cc.egui_ctx);
        if let Some(path) = path {
            doc.open(path);
        }
        Self {
            doc,
            confirm_close: false,
            bench,
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped.iter().map(|f| f.path().to_path_buf()) {
            if self.doc.view.is_none() {
                self.doc.open(file);
            } else if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(file).spawn(); // uma janela por documento
            }
        }
    }

    /// Confirmação de saída com anotações não salvas.
    fn confirm_close_window(&mut self, ui: &mut egui::Ui) {
        if !self.confirm_close {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Window::new("Alterações não salvas")
            .collapsible(false)
            .show(ui, |ui| {
                ui.label(format!(
                    "Há {} anotação(ões) que ainda não foram salvas no PDF.",
                    self.doc.edit.pending.items().len()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Sair sem salvar").clicked() {
                        self.doc.edit.reset(); // sem pendentes, o fechamento não pede confirmação de novo
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Voltar").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
    }
}

impl eframe::App for GeraApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.doc.poll(ctx, self.bench.as_mut());
        self.handle_drops(ctx);
        if let (Some(bench), Some(view)) = (self.bench.as_mut(), self.doc.view.as_mut()) {
            if let Some(y) = bench.scroll_step(ctx, view) {
                self.doc.jump_to = Some(y);
            }
        }
        // Fechar com anotações pendentes pede confirmação, em vez de perdê-las em silêncio.
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        self.doc.handle_input(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.doc.show(ui);
        self.confirm_close_window(ui);
    }
}
