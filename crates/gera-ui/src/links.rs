//! Links das páginas: o que fica sob o ponteiro, a dica e o aviso de segurança antes de sair do
//! documento (como no Acrobat).
use app_state::link_policy::{self, LinkDecision};
use pdf_core::{LinkInfo, LinkTarget};
use std::collections::BTreeMap;

/// Link da página `page` sob o ponto `pt` (coordenadas do MuPDF). `links` vem em ordem de página
/// (como no resumo do documento); com links sobrepostos vale o de cima, o último.
pub fn link_at(links: &[LinkInfo], page: i32, pt: (f32, f32)) -> Option<&LinkInfo> {
    let start = links.partition_point(|l| l.page < page);
    let end = start + links[start..].partition_point(|l| l.page == page);
    links[start..end]
        .iter()
        .rev()
        .find(|l| l.rect.contains(pt.0, pt.1))
}

/// Tamanho máximo do endereço na dica e no aviso (o resto vira "…").
const HINT_CHARS: usize = 200;

/// Começo de `text` com no máximo `max` caracteres, com "…" se foi cortado.
fn shortened(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if out.len() < text.len() {
        out.push('…');
    }
    out
}

/// Dica mostrada sobre o link.
pub fn hint(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Page { page, .. } => format!("Página {}", page + 1),
        LinkTarget::Uri(url) => shortened(url, HINT_CHARS),
        LinkTarget::File { path, .. } => app_state::tabs::tab_title(path, 80),
        LinkTarget::Blocked(_) => "Link bloqueado".into(),
    }
}

/// O que a casca faz com um link de fora do documento.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Abre o endereço no navegador (já permitido para este site).
    OpenUrl(String),
    /// Pergunta antes (aviso de segurança).
    Ask(LinkWarning),
    /// Não faz nada além da mensagem na barra de status.
    Refuse(String),
}

pub const BLOCKED_MESSAGE: &str =
    "Este link tenta abrir um programa ou endereço não permitido e foi bloqueado.";

/// Decide pelo alvo e pelas escolhas lembradas. Links para página não passam por aqui.
pub fn outcome(target: &LinkTarget, sites: &BTreeMap<String, LinkDecision>) -> Option<Outcome> {
    Some(match target {
        LinkTarget::Page { .. } => return None,
        LinkTarget::Uri(url) => match link_policy::remembered(sites, url) {
            Some(LinkDecision::Allow) => Outcome::OpenUrl(url.clone()),
            Some(LinkDecision::Block) => Outcome::Refuse(format!("Link bloqueado: {url}")),
            None => Outcome::Ask(LinkWarning::new(target.clone())),
        },
        LinkTarget::File { .. } => Outcome::Ask(LinkWarning::new(target.clone())),
        LinkTarget::Blocked(_) => Outcome::Refuse(BLOCKED_MESSAGE.into()),
    })
}

/// Janela "Aviso de segurança" (endereço ou outro arquivo).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkWarning {
    pub target: LinkTarget,
    /// "Lembrar desta ação para este site" (só para endereços).
    pub remember: bool,
    /// Quando a janela apareceu (os botões só valem depois de `ARM_DELAY`).
    opened_at: Option<f64>,
}

/// Segundos até os botões aceitarem clique: o clique que abriu o aviso não pode cair em Permitir.
pub const ARM_DELAY: f64 = 0.6;

pub(crate) fn armed(opened_at: f64, now: f64) -> bool {
    now - opened_at >= ARM_DELAY - 1e-9
}

impl LinkWarning {
    fn new(target: LinkTarget) -> Self {
        Self {
            target,
            remember: false,
            opened_at: None,
        }
    }

    /// Mostra a janela; devolve a escolha (Permitir ou Bloquear) quando o usuário escolhe.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<LinkDecision> {
        let mut answer = None;
        let now = ctx.input(|i| i.time);
        let ready = armed(*self.opened_at.get_or_insert(now), now);
        if !ready {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(ARM_DELAY));
        }
        egui::Window::new("Aviso de segurança")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                let (what, detail, rememberable) = match &self.target {
                    // endereço ambíguo (sem site reconhecível): nada a lembrar, o aviso volta sempre
                    LinkTarget::Uri(url) => (
                        "se conectar a:",
                        shortened(url, 2 * HINT_CHARS),
                        link_policy::site_key(url).is_some(),
                    ),
                    LinkTarget::File { path, .. } => {
                        let path = path.display().to_string();
                        ("abrir o arquivo:", shortened(&path, 2 * HINT_CHARS), false)
                    }
                    _ => return,
                };
                ui.label(format!("O documento está tentando {what}"));
                ui.add(egui::Label::new(egui::RichText::new(detail).monospace()).wrap());
                ui.add_space(4.0);
                ui.label(
                    "Se você confia neste destino, escolha Permitir. Se não confia, escolha \
                     Bloquear.",
                );
                if rememberable {
                    ui.checkbox(
                        &mut self.remember,
                        "Lembrar desta ação para este site em todos os documentos PDF",
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(ready, egui::Button::new("Permitir"))
                        .clicked()
                    {
                        answer = Some(LinkDecision::Allow);
                    }
                    let block = ui.add_enabled(ready, egui::Button::new("Bloquear"));
                    if block.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        answer = Some(LinkDecision::Block);
                    }
                });
            });
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    fn link(page: i32, x0: f32, target: LinkTarget) -> LinkInfo {
        LinkInfo {
            page,
            rect: PdfRect {
                x0,
                y0: 0.0,
                x1: x0 + 10.0,
                y1: 10.0,
            },
            target,
        }
    }

    #[test]
    fn link_at_finds_topmost_link_under_point() {
        let links = [
            link(0, 0.0, LinkTarget::Uri("https://a".into())),
            link(0, 5.0, LinkTarget::Uri("https://b".into())),
            link(1, 0.0, LinkTarget::Uri("https://c".into())),
        ];
        let hit = |page, x| link_at(&links, page, (x, 5.0)).map(|l| hint(&l.target));
        assert_eq!(hit(0, 2.0).as_deref(), Some("https://a"));
        assert_eq!(hit(0, 7.0).as_deref(), Some("https://b"), "o de cima");
        assert_eq!(hit(1, 7.0).as_deref(), Some("https://c"));
        assert_eq!(hit(0, 50.0), None);
        assert_eq!(hit(2, 2.0), None);
    }

    #[test]
    fn buttons_wait_before_accepting_a_click() {
        // o clique (ou o duplo clique) que abriu o aviso não pode cair em Permitir
        assert!(!armed(10.0, 10.0));
        assert!(!armed(10.0, 10.0 + ARM_DELAY - 0.01));
        assert!(armed(10.0, 10.0 + ARM_DELAY));
    }

    #[test]
    fn long_addresses_are_shortened_on_screen() {
        let url = format!("https://example.com/{}", "a".repeat(5000));
        let shown = hint(&LinkTarget::Uri(url.clone()));
        assert!(shown.chars().count() <= HINT_CHARS + 1, "{}", shown.len());
        assert!(shown.starts_with("https://example.com/") && shown.ends_with('…'));
        assert_eq!(hint(&LinkTarget::Uri("https://a.b".into())), "https://a.b");
    }

    #[test]
    fn unknown_site_asks() {
        let o = outcome(&LinkTarget::Uri("https://ex.com".into()), &BTreeMap::new());
        assert!(matches!(
            o,
            Some(Outcome::Ask(LinkWarning {
                target: LinkTarget::Uri(_),
                ..
            }))
        ));
    }

    #[test]
    fn remembered_allow_opens_without_asking() {
        let mut sites = BTreeMap::new();
        link_policy::remember(&mut sites, "https://ex.com", LinkDecision::Allow);
        let o = outcome(&LinkTarget::Uri("https://EX.com/x".into()), &sites);
        assert_eq!(o, Some(Outcome::OpenUrl("https://EX.com/x".into())));
    }

    #[test]
    fn remembered_block_never_opens() {
        let mut sites = BTreeMap::new();
        link_policy::remember(&mut sites, "https://ex.com", LinkDecision::Block);
        let o = outcome(&LinkTarget::Uri("https://ex.com/x".into()), &sites);
        assert!(matches!(o, Some(Outcome::Refuse(_))));
    }

    #[test]
    fn files_always_ask_and_blocked_links_never_open() {
        let file = LinkTarget::File {
            path: "/tmp/a.pdf".into(),
            page: Some(2),
        };
        assert!(matches!(
            outcome(&file, &BTreeMap::new()),
            Some(Outcome::Ask(LinkWarning {
                target: LinkTarget::File { page: Some(2), .. },
                ..
            }))
        ));
        assert_eq!(
            outcome(&LinkTarget::Blocked("calc.exe".into()), &BTreeMap::new()),
            Some(Outcome::Refuse(BLOCKED_MESSAGE.into()))
        );
        let page = LinkTarget::Page { page: 0, at: None };
        assert_eq!(outcome(&page, &BTreeMap::new()), None);
        assert_eq!(hint(&page), "Página 1");
        assert_eq!(hint(&file), "a.pdf");
    }
}
