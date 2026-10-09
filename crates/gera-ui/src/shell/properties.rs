//! Janela "Propriedades do documento" (Arquivo → Propriedades, Ctrl+D), no arranjo do Acrobat.
use app_state::pdf_date::format_pdf_date;
use pdf_core::{DocProperties, PageSize};

/// Número com vírgula decimal (pt-BR).
fn decimal(v: f64, places: usize) -> String {
    format!("{v:.places$}").replace('.', ",")
}

/// Tamanho de arquivo legível: "900 bytes", "1,5 KB", "1,11 MB".
fn size_text(bytes: u64) -> String {
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

pub fn window(ui: &mut egui::Ui, p: &DocProperties, open: &mut bool) {
    let rows = |ui: &mut egui::Ui, id: &str, items: &[(&str, String)]| {
        egui::Grid::new(id)
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (label, value) in items {
                    ui.strong(*label);
                    ui.label(value);
                    ui.end_row();
                }
            });
    };
    egui::Window::new("Propriedades do documento")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .show(ui, |ui| {
            ui.heading("Descrição");
            rows(
                ui,
                "descricao",
                &[
                    ("Arquivo:", p.path.display().to_string()),
                    ("Título:", or_dash(&p.title).into()),
                    ("Autor:", or_dash(&p.author).into()),
                    ("Assunto:", or_dash(&p.subject).into()),
                    ("Palavras-chave:", or_dash(&p.keywords).into()),
                    ("Criado:", format_pdf_date(&p.created)),
                    ("Modificado:", format_pdf_date(&p.modified)),
                    ("Aplicativo:", or_dash(&p.creator).into()),
                    ("Produtor do PDF:", or_dash(&p.producer).into()),
                ],
            );
            ui.add_space(8.0);
            ui.heading("Avançado");
            rows(
                ui,
                "avancado",
                &[
                    ("Versão do PDF:", or_dash(&p.format).into()),
                    ("Tamanho da página:", page_size_text(p.first_page)),
                    ("Número de páginas:", p.pages.to_string()),
                    ("Tamanho do arquivo:", size_text(p.file_size)),
                    (
                        "Protegido por senha:",
                        if p.encrypted { "Sim" } else { "Não" }.into(),
                    ),
                ],
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
