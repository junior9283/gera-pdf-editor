//! Certificados (como no Acrobat): assinar com um ID digital arrastando o retângulo ou clicando
//! num campo de assinatura vazio, o painel Assinaturas e a barra azul dos documentos assinados.
use crate::edit::screen_rect;
use crate::shell::sign_dialog::SignDialog;
use app_state::certificates::{self as certs, DigitalIds, SigStatus, TrustedCerts};
use app_state::store;
use app_state::transform::PageMap;
use pdf_core::{FieldInfo, FieldKind, PdfRect, SignPlace, SignatureInfo};
use std::sync::Mutex;

/// IDs digitais e certificados de confiança, lidos do disco uma vez e compartilhados pelas abas.
static IDS: Mutex<Option<DigitalIds>> = Mutex::new(None);
static TRUST: Mutex<Option<TrustedCerts>> = Mutex::new(None);

fn cached<T: Clone, R>(
    cell: &Mutex<Option<T>>,
    load: impl FnOnce() -> T,
    f: impl FnOnce(&mut T) -> R,
) -> R {
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(load))
}

pub fn with_ids<R>(f: impl FnOnce(&mut DigitalIds) -> R) -> R {
    cached(
        &IDS,
        || {
            store::config_file(certs::IDS_FILE)
                .map_or_else(Default::default, |p| DigitalIds::load(&p))
        },
        f,
    )
}

pub fn with_trust<R>(f: impl FnOnce(&mut TrustedCerts) -> R) -> R {
    cached(
        &TRUST,
        || {
            store::config_file(certs::TRUST_FILE)
                .map_or_else(Default::default, |p| TrustedCerts::load(&p))
        },
        f,
    )
}

/// Grava a lista de IDs depois de mudar.
pub fn save_ids() -> std::io::Result<()> {
    match store::config_file(certs::IDS_FILE) {
        Some(p) => with_ids(|ids| ids.save(&p)),
        None => Ok(()),
    }
}

/// Passa a confiar no certificado (impressão digital) e grava.
pub fn trust(fingerprint: &str) -> std::io::Result<()> {
    with_trust(|t| t.trust(fingerprint));
    match store::config_file(certs::TRUST_FILE) {
        Some(p) => with_trust(|t| t.save(&p)),
        None => Ok(()),
    }
}

/// Arrasto do retângulo da assinatura: (página, de, até).
type Drag = (i32, (f32, f32), (f32, f32));

/// Estado da ferramenta Certificados numa aba.
#[derive(Default)]
pub struct CertState {
    /// "Assinar digitalmente" ativo: arrastar o retângulo ou clicar num campo vazio.
    pub placing: bool,
    drag: Option<Drag>,
    pub dialog: Option<SignDialog>,
    /// Assinaturas do documento (validadas pelo worker).
    pub signatures: Vec<SignatureInfo>,
}

impl CertState {
    /// Abre a janela de assinatura para `place`.
    pub fn sign(&mut self, place: SignPlace, author: Option<&str>) {
        self.placing = false;
        self.dialog = Some(SignDialog::new(place, author));
    }
}

/// Campo de assinatura ainda não assinado sob `p`.
fn empty_sig_field(
    fields: &[FieldInfo],
    signed: &[SignatureInfo],
    page: i32,
    p: (f32, f32),
) -> Option<i32> {
    fields
        .iter()
        .filter(|f| f.page == page && matches!(f.kind, FieldKind::Signature))
        .filter(|f| !signed.iter().any(|s| s.page == page && s.id == f.id))
        .find(|f| f.rect.x0 <= p.0 && p.0 <= f.rect.x1 && f.rect.y0 <= p.1 && p.1 <= f.rect.y1)
        .map(|f| f.id)
}

/// Gestos de "Assinar digitalmente" na página: devolve onde assinar quando o usuário escolheu.
pub fn on_page(
    ui: &mut egui::Ui,
    st: &mut CertState,
    fields: &[FieldInfo],
    page: i32,
    rect: egui::Rect,
    map: &PageMap,
) -> Option<SignPlace> {
    let response = ui
        .interact(
            rect,
            ui.id().with(("assinar", page)),
            egui::Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    let pointer = response
        .interact_pointer_pos()
        .map(|p| map.to_page(p.into()));
    let mut out = None;
    if response.drag_started() {
        if let Some(p) = crate::edit::press_point(&response, map, pointer) {
            st.drag = Some((page, p, p));
        }
    }
    if let (Some(d), Some(p)) = (st.drag.as_mut().filter(|d| d.0 == page), pointer) {
        d.2 = p;
    }
    if response.drag_stopped() {
        if let Some((_, a, b)) = st.drag.take().filter(|d| d.0 == page) {
            let r = PdfRect::from_corners(a, b);
            if r.width() >= 20.0 && r.height() >= 10.0 {
                out = Some(SignPlace::New { page, rect: r });
            }
        }
    } else if response.clicked() {
        out = pointer
            .and_then(|p| empty_sig_field(fields, &st.signatures, page, p))
            .map(|id| SignPlace::Field { page, id });
    }
    if let Some((_, a, b)) = st.drag.filter(|d| d.0 == page) {
        ui.painter().rect_stroke(
            screen_rect(&PdfRect::from_corners(a, b), map),
            0.0,
            egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Middle,
        );
    }
    out
}

fn status_color(s: SigStatus) -> egui::Color32 {
    match s {
        SigStatus::Valid => egui::Color32::from_rgb(30, 140, 60),
        SigStatus::Unknown => egui::Color32::from_rgb(200, 140, 0),
        SigStatus::Invalid => egui::Color32::from_rgb(200, 40, 40),
    }
}

fn status_icon(s: SigStatus) -> &'static str {
    match s {
        SigStatus::Valid => "✔",
        SigStatus::Unknown => "⚠",
        SigStatus::Invalid => "✖",
    }
}

/// Barra azul dos documentos assinados; devolve `true` se o usuário pediu o painel.
pub fn bar(ui: &mut egui::Ui, signatures: &[SignatureInfo]) -> bool {
    let Some((status, text)) = with_trust(|t| certs::bar_text(signatures, t)) else {
        return false;
    };
    let mut open = false;
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(222, 235, 252))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.colored_label(status_color(status), status_icon(status));
                ui.label(egui::RichText::new(text).color(egui::Color32::from_rgb(20, 50, 100)));
                if ui.button("Painel de assinaturas").clicked() {
                    open = true;
                }
            });
        });
    open
}

/// O que o usuário pediu no painel Assinaturas.
pub enum PanelAction {
    GoTo(i32, PdfRect),
    Validate,
}

/// Painel Assinaturas (trilho à esquerda).
pub fn panel(
    ui: &mut egui::Ui,
    signatures: &[SignatureInfo],
    fields: &[FieldInfo],
) -> Option<PanelAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.strong("Assinaturas");
        if ui.button("Validar todas").clicked() {
            action = Some(PanelAction::Validate);
        }
    });
    ui.separator();
    if signatures.is_empty() {
        ui.label("Este documento não tem assinaturas digitais.");
        return action;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for s in signatures {
            let status = with_trust(|t| certs::status(s, t));
            ui.horizontal(|ui| {
                ui.colored_label(status_color(status), status_icon(status));
                let who = if s.signer.is_empty() { "Titular desconhecido" } else { &s.signer };
                let title = format!("Assinado por {who}");
                if ui.link(title).clicked() {
                    if let Some(f) = fields.iter().find(|f| f.page == s.page && f.id == s.id) {
                        action = Some(PanelAction::GoTo(f.page, f.rect));
                    }
                }
            });
            ui.indent(("assinatura", s.page, s.id), |ui| {
                ui.label(match (&s.integrity, status) {
                    (pdf_core::Integrity::Broken(why), _) => format!("Assinatura inválida: {why}."),
                    (pdf_core::Integrity::Unverifiable(why), _) => {
                        format!("Não foi possível verificar a assinatura: {why}.")
                    }
                    (_, SigStatus::Valid) => "A assinatura é válida.".to_owned(),
                    _ => "Validade desconhecida: a identidade do titular não foi verificada como confiável.".to_owned(),
                });
                if s.modified_after && s.changes_permitted {
                    ui.label(
                        "Depois desta assinatura só houve alterações permitidas (preencher campos ou assinar).",
                    );
                } else if s.modified_after {
                    ui.label("O conteúdo das páginas foi alterado depois desta assinatura.");
                }
                if s.integrity == pdf_core::Integrity::Intact && !s.cert_valid_at_signing {
                    ui.label("O certificado não estava dentro da validade na hora da assinatura.");
                } else if s.integrity == pdf_core::Integrity::Intact {
                    ui.label("O documento não foi alterado desde esta assinatura.");
                }
                ui.label(format!("Data: {}", pdf_core::display_date(&s.signed_at)));
                if !s.reason.is_empty() {
                    ui.label(format!("Motivo: {}", s.reason));
                }
                if !s.location.is_empty() {
                    ui.label(format!("Local: {}", s.location));
                }
                ui.label(format!("Campo: {} (página {})", s.field, s.page + 1));
                let leaf = s.fingerprints.first();
                if let (Some(fp), false) = (leaf, status == SigStatus::Valid) {
                    if s.integrity == pdf_core::Integrity::Intact
                        && ui
                            .button("Confiar neste certificado")
                            .on_hover_text(format!("Impressão digital SHA-256: {fp}"))
                            .clicked()
                    {
                        let _ = trust(fp);
                    }
                }
            });
            ui.separator();
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unsigned_signature_fields_are_clickable() {
        let field = |id, kind| FieldInfo {
            id,
            page: 0,
            name: format!("f{id}"),
            kind,
            rect: PdfRect::new(0.0, 0.0, 100.0, 40.0),
            value: String::new(),
            readonly: false,
            required: false,
            tooltip: String::new(),
            keystroke: Default::default(),
            format: Default::default(),
            validate: Default::default(),
            calc: Default::default(),
        };
        let fields = [field(1, FieldKind::Signature)];
        assert_eq!(empty_sig_field(&fields, &[], 0, (10.0, 10.0)), Some(1));
        assert_eq!(empty_sig_field(&fields, &[], 0, (200.0, 10.0)), None);
        let text = [field(2, FieldKind::Button { reset: false })];
        assert_eq!(empty_sig_field(&text, &[], 0, (10.0, 10.0)), None);
    }
}
