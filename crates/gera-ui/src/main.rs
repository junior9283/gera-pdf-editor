// sem janela de console no Windows
#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod attachments_panel;
mod bench;
mod bookmarks;
mod canvas;
mod certificates;
mod clipboard;
mod comment_tools;
mod comments_panel;
mod create;
#[cfg(unix)]
mod cups;
mod desktop;
mod destinations_panel;
mod doc_tab;
mod edit;
mod edit_pdf;
mod fields;
mod fill_sign;
mod icon;
mod layers_panel;
mod links;
mod open_dialog;
mod organize;
mod page_labels_dialog;
mod pagination_tool;
mod prepare;
mod print_job;
#[cfg(windows)]
mod print_win;
mod redact_tool;
mod search;
mod shell;
mod sidebar;
mod stamp_dialogs;
mod submit;
mod summary_dialog;
mod thumbnails;
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
    desktop::integrate_appimage();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 800.0])
        .with_title("Gera PDF Editor")
        .with_app_id(desktop::APP_ID)
        // no Windows abre maximizada, como o Acrobat (e não passa da tela num notebook)
        .with_maximized(cfg!(windows));
    if let Some(icon) = icon::app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        // no Windows, o wgpu (DirectX 12, ou o WARP por software sem placa de vídeo): o OpenGL do
        // Windows sem driver não abre a janela
        #[cfg(windows)]
        renderer: eframe::Renderer::Wgpu,
        #[cfg(not(windows))]
        renderer: eframe::Renderer::Glow,
        viewport,
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
