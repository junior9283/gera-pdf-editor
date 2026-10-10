//! Certificados: IDs digitais conhecidos, certificados de confiança e a situação de cada
//! assinatura (como o Acrobat mostra no painel Assinaturas e na barra azul).
use pdf_core::{Integrity, SignatureInfo};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const IDS_FILE: &str = "ids_digitais.json";
pub const TRUST_FILE: &str = "certificados_confiaveis.json";

/// ID digital conhecido (só o caminho e quem é; a senha nunca é guardada).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdEntry {
    pub path: PathBuf,
    pub name: String,
    pub email: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigitalIds {
    pub ids: Vec<IdEntry>,
}

impl DigitalIds {
    pub fn load(path: &Path) -> Self {
        crate::store::load_json(path)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::store::save_json(path, self)
    }

    /// Acrescenta (ou atualiza, se o mesmo certificado já está na lista).
    pub fn add(&mut self, entry: IdEntry) {
        self.ids.retain(|e| e.fingerprint != entry.fingerprint);
        self.ids.push(entry);
    }

    pub fn remove(&mut self, fingerprint: &str) {
        self.ids.retain(|e| e.fingerprint != fingerprint);
    }
}

/// Certificados em que o usuário confia (impressões digitais SHA-256).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedCerts {
    pub fingerprints: Vec<String>,
}

impl TrustedCerts {
    pub fn load(path: &Path) -> Self {
        crate::store::load_json(path)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::store::save_json(path, self)
    }

    pub fn trust(&mut self, fingerprint: &str) {
        if !self.trusts(fingerprint) {
            self.fingerprints.push(fingerprint.to_owned());
        }
    }

    pub fn trusts(&self, fingerprint: &str) -> bool {
        self.fingerprints.iter().any(|f| f == fingerprint)
    }
}

/// Situação de uma assinatura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigStatus {
    /// Íntegra e de um certificado (ou cadeia) confiável.
    Valid,
    /// Íntegra, mas a identidade do titular não foi confirmada.
    Unknown,
    /// Documento alterado no trecho assinado ou assinatura que não confere.
    Invalid,
}

pub fn status(sig: &SignatureInfo, trusted: &TrustedCerts) -> SigStatus {
    match sig.integrity {
        Integrity::Broken(_) => SigStatus::Invalid,
        Integrity::Intact if sig.fingerprints.iter().any(|f| trusted.trusts(f)) => SigStatus::Valid,
        Integrity::Intact => SigStatus::Unknown,
    }
}

/// Texto da barra azul dos documentos assinados (nenhum se não há assinaturas).
pub fn bar_text(
    sigs: &[SignatureInfo],
    trusted: &TrustedCerts,
) -> Option<(SigStatus, &'static str)> {
    if sigs.is_empty() {
        return None;
    }
    let all: Vec<SigStatus> = sigs.iter().map(|s| status(s, trusted)).collect();
    Some(if all.contains(&SigStatus::Invalid) {
        (
            SigStatus::Invalid,
            "Pelo menos uma assinatura tem problemas.",
        )
    } else if all.iter().all(|s| *s == SigStatus::Valid) {
        (
            SigStatus::Valid,
            "Assinado e todas as assinaturas são válidas.",
        )
    } else {
        (SigStatus::Unknown, "Validade da assinatura desconhecida: pelo menos uma assinatura não foi verificada como confiável.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(integrity: Integrity, fp: &str) -> SignatureInfo {
        SignatureInfo {
            field: "Assinatura1".into(),
            page: 0,
            id: 1,
            signer: "Ana".into(),
            email: String::new(),
            signed_at: String::new(),
            reason: String::new(),
            location: String::new(),
            integrity,
            modified_after: false,
            fingerprints: vec![fp.into()],
        }
    }

    #[test]
    fn status_follows_integrity_and_trust() {
        let mut t = TrustedCerts::default();
        let ok = sig(Integrity::Intact, "aa");
        assert_eq!(status(&ok, &t), SigStatus::Unknown);
        t.trust("aa");
        t.trust("aa");
        assert_eq!(t.fingerprints.len(), 1);
        assert_eq!(status(&ok, &t), SigStatus::Valid);
        assert_eq!(
            status(&sig(Integrity::Broken("x".into()), "aa"), &t),
            SigStatus::Invalid
        );
    }

    #[test]
    fn the_bar_summarizes_like_acrobat() {
        let mut t = TrustedCerts::default();
        assert!(bar_text(&[], &t).is_none());
        let a = sig(Integrity::Intact, "aa");
        assert_eq!(bar_text(&[a.clone()], &t).unwrap().0, SigStatus::Unknown);
        t.trust("aa");
        assert_eq!(bar_text(&[a.clone()], &t).unwrap().0, SigStatus::Valid);
        let bad = sig(Integrity::Broken("x".into()), "aa");
        assert_eq!(bar_text(&[a, bad], &t).unwrap().0, SigStatus::Invalid);
    }

    #[test]
    fn ids_are_listed_once_per_certificate_and_survive_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(IDS_FILE);
        let mut ids = DigitalIds::default();
        let e = IdEntry {
            path: "/x/ana.pfx".into(),
            name: "Ana".into(),
            email: String::new(),
            fingerprint: "aa".into(),
        };
        ids.add(e.clone());
        ids.add(IdEntry {
            path: "/y/ana.pfx".into(),
            ..e
        });
        assert_eq!(ids.ids.len(), 1);
        ids.save(&path).unwrap();
        assert_eq!(DigitalIds::load(&path), ids);
        std::fs::write(&path, b"{").unwrap();
        assert_eq!(DigitalIds::load(&path), DigitalIds::default());
        ids.remove("aa");
        assert!(ids.ids.is_empty());
    }
}
