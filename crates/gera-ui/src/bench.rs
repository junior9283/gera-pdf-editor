//! Modo de medição (`--bench-open`, `--bench-scroll`): imprime métricas e fecha o app.
//!
//! Fica no binário de produção de propósito: `scripts/measure.sh` mede o mesmo executável que
//! o usuário recebe (tamanho e tempos). Sem as flags, o custo é um `Option` vazio.
use crate::view::DocView;
use std::time::Instant;

/// Memória residente do processo, em kB, lida do texto de `/proc/self/status`.
fn rss_kb_from(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Memória residente atual, ou `?` se não foi possível medir (o script de medição trata como falha).
fn rss_kb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| rss_kb_from(&s))
        .map_or_else(|| "?".into(), |kb| kb.to_string())
}

pub struct Bench {
    started: Instant,
    /// Percorrer todas as páginas depois de abrir?
    scroll: bool,
    /// A abertura já foi medida?
    opened: bool,
    /// Percurso em andamento: (início, próxima página).
    run: Option<(Instant, usize)>,
}

impl Bench {
    pub fn new(started: Instant, scroll: bool) -> Self {
        Self {
            started,
            scroll,
            opened: false,
            run: None,
        }
    }

    /// A primeira página do canvas ficou pronta: imprime a abertura e fecha (ou começa o percurso).
    pub fn on_first_page(&mut self, ctx: &egui::Context) {
        if std::mem::replace(&mut self.opened, true) {
            return;
        }
        println!(
            "BENCH open_ms={} rss_kb={}",
            self.started.elapsed().as_millis(),
            rss_kb()
        );
        if self.scroll {
            self.run = Some((Instant::now(), 0));
            ctx.request_repaint();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Um passo do percurso: vai para a próxima página quando a anterior já está na tela.
    /// Devolve a posição de rolagem a aplicar, se houver.
    pub fn scroll_step(&mut self, ctx: &egui::Context, view: &mut DocView) -> Option<f32> {
        let (started, next) = self.run.as_mut()?;
        ctx.request_repaint();
        if *next > 0
            && view
                .texture_at(*next as i32 - 1, view.page_scale())
                .is_none()
        {
            return None; // espera a página atual renderizar
        }
        if *next >= view.layout.page_count() {
            println!(
                "BENCH scroll_ms={} pages={} rss_kb={}",
                started.elapsed().as_millis(),
                next,
                rss_kb()
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.run = None;
            return None;
        }
        let target = view.scroll_target(*next as i32, 0.0);
        *next += 1;
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_is_read_from_proc_status_text() {
        let status = "Name:\tgera\nVmPeak:\t 999 kB\nVmRSS:\t  123456 kB\nThreads:\t4\n";
        assert_eq!(rss_kb_from(status), Some(123456));
        assert_eq!(rss_kb_from("Name:\tgera\n"), None);
        assert_eq!(rss_kb_from("VmRSS:\tabc kB\n"), None);
    }
}
