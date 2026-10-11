mod app;
mod bench;
mod canvas;
mod certificates;
mod comments_panel;
mod create;
mod doc_tab;
mod edit;
mod edit_pdf;
mod fields;
mod fill_sign;
mod links;
mod open_dialog;
mod organize;
mod pagination_tool;
mod prepare;
mod print_job;
mod redact_tool;
mod search;
mod shell;
mod sidebar;
mod view;

use std::path::PathBuf;
use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    let mut paths = Vec::new();
    let (mut bench, mut scroll) = (false, false);
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--bench-open" => bench = true,
            "--bench-scroll" => (bench, scroll) = (true, true),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_title("Gera PDF Editor")
            .with_app_id("gera-pdf-editor"),
        ..Default::default()
    };
    eframe::run_native(
        "Gera PDF Editor",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::GeraApp::new(
                cc,
                paths,
                bench.then(|| bench::Bench::new(started, scroll)),
            )))
        }),
    )
}
