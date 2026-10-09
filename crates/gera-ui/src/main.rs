mod app;
mod canvas;
mod view;

use std::path::PathBuf;
use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    let mut path = None;
    let mut bench = false;
    for arg in std::env::args().skip(1) {
        if arg == "--bench-open" {
            bench = true;
        } else {
            path = Some(PathBuf::from(arg));
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
                path,
                bench.then_some(started),
            )))
        }),
    )
}
