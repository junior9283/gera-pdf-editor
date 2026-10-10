//! IDs digitais (certificado X.509 + chave privada em PKCS#12, como os `.pfx` do Acrobat): abrir
//! com a senha e criar um ID autoassinado.
use crate::error::{PdfError, Result};
use der::{Decode, Encode};
use p12_keystore::{
    Certificate as P12Cert, KeyStore, KeyStoreEntry, Pkcs12ImportPolicy, PrivateKey,
    PrivateKeyChain,
};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;
use x509_cert::Certificate;

/// Chave privada do ID (as que o Acrobat usa: RSA e ECDSA P-256).
#[derive(Clone)]
pub(crate) enum SigningKey {
    Rsa(Box<rsa::RsaPrivateKey>),
    P256(p256::ecdsa::SigningKey),
}

/// ID digital aberto (a chave fica só em memória).
#[derive(Clone)]
pub struct DigitalId {
    /// Nome do titular (CN do certificado).
    pub name: String,
    pub email: String,
    /// Certificado do titular (DER) e o resto da cadeia, se houver.
    pub cert_der: Vec<u8>,
    pub chain: Vec<Vec<u8>>,
    pub(crate) key: SigningKey,
}

impl std::fmt::Debug for DigitalId {
    // nunca mostra a chave
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DigitalId")
            .field("name", &self.name)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

fn err(msg: impl std::fmt::Display) -> PdfError {
    PdfError::Signature(msg.to_string())
}

/// Texto de um atributo do nome X.509 (UTF8String, PrintableString, IA5String…).
fn attr_text(any: &der::Any) -> Option<String> {
    use der::asn1::{Ia5StringRef, PrintableStringRef, TeletexStringRef, Utf8StringRef};
    if let Ok(s) = any.decode_as::<Utf8StringRef<'_>>() {
        return Some(s.as_str().to_owned());
    }
    if let Ok(s) = any.decode_as::<PrintableStringRef<'_>>() {
        return Some(s.as_str().to_owned());
    }
    if let Ok(s) = any.decode_as::<Ia5StringRef<'_>>() {
        return Some(s.as_str().to_owned());
    }
    any.decode_as::<TeletexStringRef<'_>>()
        .ok()
        .map(|s| s.as_str().to_owned())
}

/// Valor do atributo `oid` no nome (o primeiro).
pub(crate) fn name_attr(name: &Name, oid: der::oid::ObjectIdentifier) -> Option<String> {
    name.0
        .iter()
        .flat_map(|rdn| rdn.0.iter())
        .find(|a| a.oid == oid)
        .and_then(|a| attr_text(&a.value))
}

const CN: der::oid::ObjectIdentifier = der::oid::db::rfc4519::CN;
const EMAIL: der::oid::ObjectIdentifier = der::oid::db::rfc3280::EMAIL_ADDRESS;

/// Titular (CN, ou o nome inteiro se não houver CN) e e-mail do certificado.
pub(crate) fn subject_of(cert: &Certificate) -> (String, String) {
    let subject = &cert.tbs_certificate.subject;
    let name = name_attr(subject, CN).unwrap_or_else(|| subject.to_string());
    let email = name_attr(subject, EMAIL).unwrap_or_default();
    (name, email)
}

/// Escapa um valor para o nome no formato RFC 4514 ("CN=…,O=…").
fn rfc4514(v: &str) -> String {
    let mut out = String::new();
    for (i, c) in v.trim().chars().enumerate() {
        if matches!(c, ',' | '+' | '"' | '\\' | '<' | '>' | ';' | '=') || (i == 0 && c == '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl DigitalId {
    /// Abre um `.pfx`/`.p12` com a senha.
    pub fn open(path: &Path, password: &str) -> Result<Self> {
        let data = std::fs::read(path)?;
        let ks = KeyStore::from_pkcs12(&data, password, Pkcs12ImportPolicy::Relaxed)
            .map_err(|_| err("senha incorreta ou arquivo que não é um ID digital"))?;
        let (_, chain) = ks
            .private_key_chain()
            .ok_or_else(|| err("o arquivo não tem chave privada"))?;
        let key_der = chain.key().as_der();
        let key = if let Ok(k) = rsa::RsaPrivateKey::from_pkcs8_der(key_der) {
            SigningKey::Rsa(Box::new(k))
        } else if let Ok(k) = p256::SecretKey::from_pkcs8_der(key_der) {
            SigningKey::P256(k.into())
        } else {
            return Err(err("tipo de chave não suportado (só RSA e ECDSA P-256)"));
        };
        let certs: Vec<Vec<u8>> = chain.certs().iter().map(|c| c.as_der().to_vec()).collect();
        // o certificado do titular é o da chave (a ordem no arquivo varia)
        let public = key.public_spki()?;
        let leaf = certs
            .iter()
            .position(|c| {
                Certificate::from_der(c)
                    .is_ok_and(|c| c.tbs_certificate.subject_public_key_info == public)
            })
            .ok_or_else(|| err("o certificado do titular não está no arquivo"))?;
        let mut chain = certs;
        let cert_der = chain.remove(leaf);
        let (name, email) = subject_of(&Certificate::from_der(&cert_der).map_err(err)?);
        Ok(Self {
            name,
            email,
            cert_der,
            chain,
            key,
        })
    }

    /// Cria um ID digital autoassinado (RSA 2048, válido por 5 anos) e grava em `path` como
    /// PKCS#12 protegido por `password` (só o dono lê o arquivo).
    pub fn create_self_signed(
        name: &str,
        email: &str,
        org: &str,
        password: &str,
        path: &Path,
    ) -> Result<Self> {
        if name.trim().is_empty() {
            return Err(err("o ID digital precisa de um nome"));
        }
        if password.is_empty() {
            return Err(err("o ID digital precisa de uma senha"));
        }
        let mut rng = rand::thread_rng();
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048).map_err(err)?;
        let signer = rsa::pkcs1v15::SigningKey::<Sha256>::new(key.clone());
        let mut dn = format!("CN={}", rfc4514(name));
        if !org.trim().is_empty() {
            dn.push_str(&format!(",O={}", rfc4514(org)));
        }
        if !email.trim().is_empty() {
            dn.push_str(&format!(",{EMAIL}={}", rfc4514(email)));
        }
        let subject = Name::from_str(&dn).map_err(err)?;
        let mut serial = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rng, &mut serial);
        serial[0] &= 0x7f; // positivo
        let serial = SerialNumber::new(&serial).map_err(err)?;
        let validity = Validity::from_now(Duration::from_secs(5 * 365 * 24 * 3600)).map_err(err)?;
        let spki = SubjectPublicKeyInfoOwned::from_key(key.to_public_key()).map_err(err)?;
        let profile = Profile::Leaf {
            issuer: subject.clone(),
            enable_key_agreement: false,
            enable_key_encipherment: false,
        };
        let builder = CertificateBuilder::new(profile, serial, validity, subject, spki, &signer)
            .map_err(err)?;
        let cert = builder.build::<rsa::pkcs1v15::Signature>().map_err(err)?;
        let cert_der = cert.to_der().map_err(err)?;
        let key_der = key.to_pkcs8_der().map_err(err)?;
        let mut ks = KeyStore::new();
        let chain = PrivateKeyChain::new(
            Sha256::digest(&cert_der).to_vec(),
            PrivateKey::from_der(key_der.as_bytes()).map_err(err)?,
            [P12Cert::from_der(&cert_der).map_err(err)?],
        );
        ks.add_entry(name.trim(), KeyStoreEntry::PrivateKeyChain(chain));
        let bytes = ks.writer(password).write().map_err(err)?;
        write_private(path, &bytes)?;
        Ok(Self {
            name: name.trim().to_owned(),
            email: email.trim().to_owned(),
            cert_der,
            chain: Vec::new(),
            key: SigningKey::Rsa(Box::new(key)),
        })
    }

    /// Impressão digital SHA-256 do certificado (hexadecimal minúsculo), para a lista de confiança.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert_der)
    }
}

impl SigningKey {
    fn public_spki(&self) -> Result<SubjectPublicKeyInfoOwned> {
        match self {
            SigningKey::Rsa(k) => SubjectPublicKeyInfoOwned::from_key(k.to_public_key()),
            SigningKey::P256(k) => SubjectPublicKeyInfoOwned::from_key(*k.verifying_key()),
        }
        .map_err(err)
    }
}

/// Impressão digital SHA-256 de um certificado DER.
pub fn fingerprint(cert_der: &[u8]) -> String {
    Sha256::digest(cert_der)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Grava o arquivo com permissão só do dono (é uma chave privada).
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
