//! Assinatura digital (ferramenta Certificados do Acrobat): `/Adobe.PPKLite` +
//! `/adbe.pkcs7.detached`, CMS destacado sobre o `/ByteRange`, gravação incremental; e a validação
//! das assinaturas do documento.
use crate::digital_id::{fingerprint, subject_of, DigitalId, SigningKey};
use crate::doc::{annot_id, pdf_now, PdfDoc};
use crate::error::{PdfError, Result};
use crate::prepare::NewFieldKind;
use crate::types::PdfRect;
use cms::builder::{create_signing_time_attribute, SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::ContentInfo;
use cms::signed_data::{EncapsulatedContentInfo, SignedData, SignerIdentifier};
use der::{Decode, Encode, SliceReader};
use mupdf::pdf::{PdfObject, PdfWriteOptions};
use mupdf::{Buffer, Font};
use sha2::Digest;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

/// Onde assinar: um campo de assinatura vazio ou um retângulo novo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignPlace {
    Field { page: i32, id: i32 },
    New { page: i32, rect: PdfRect },
}

/// Integridade de uma assinatura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    /// O intervalo assinado não mudou e a assinatura confere com o certificado.
    Intact,
    /// Documento alterado no intervalo assinado, assinatura que não confere ou malformada.
    Broken(String),
    /// Algoritmo que este programa não confere (ou fraco, como SHA-1): nem válida nem inválida.
    Unverifiable(String),
}

/// Uma assinatura do documento, como o painel Assinaturas mostra.
#[derive(Debug, Clone, PartialEq)]
pub struct SignatureInfo {
    /// Nome completo do campo.
    pub field: String,
    pub page: i32,
    /// Id do widget do campo.
    pub id: i32,
    pub signer: String,
    pub email: String,
    /// Data da assinatura (/M, formato PDF "D:…").
    pub signed_at: String,
    pub reason: String,
    pub location: String,
    pub integrity: Integrity,
    /// O arquivo tem revisões depois desta assinatura.
    pub modified_after: bool,
    /// Impressões digitais SHA-256 do certificado do titular e da cadeia (titular primeiro).
    pub fingerprints: Vec<String>,
    /// As revisões posteriores só preenchem campos ou assinam (sem comentários nem mudar páginas):
    /// o conteúdo das páginas é o assinado, como o Acrobat permite num documento não certificado.
    pub changes_permitted: bool,
    /// O certificado (e a cadeia) estava dentro da validade na hora da assinatura.
    pub cert_valid_at_signing: bool,
}

/// Bytes reservados para o CMS em `/Contents` (o Acrobat reserva parecido).
const CONTENTS_BYTES: usize = 16 * 1024;
/// `/ByteRange` provisório: dez dígitos por número, corrigido depois sem mudar o tamanho.
const BYTE_RANGE_PLACEHOLDER: &str = "[0 1000000000 1000000000 1000000000]";
/// /SigFlags: o documento tem assinaturas e só aceita gravação incremental.
const SIG_FLAGS: i32 = 3;

fn err(msg: impl std::fmt::Display) -> PdfError {
    PdfError::Signature(msg.to_string())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Fonte que cabe em `width` pt para `text` em Helvetica, até `max`.
fn fit_size(font: &Font, text: &str, width: f32, max: f32) -> Result<f32> {
    let w = crate::text_fit::text_width(font, text, 1.0)?;
    Ok(if w <= 0.0 { max } else { (width / w).min(max) })
}

/// "D:20261010153000-03'00'" → "2026.10.10 15:30:00 -03'00'" (como o Acrobat mostra).
pub fn display_date(pdf: &str) -> String {
    let d = pdf.trim_start_matches("D:");
    // por bytes: um /M malicioso com acento na posição 14 não pode derrubar o app
    if !d
        .as_bytes()
        .get(..14)
        .is_some_and(|b| b.iter().all(u8::is_ascii_digit))
    {
        return pdf.to_owned();
    }
    format!(
        "{}.{}.{} {}:{}:{} {}",
        &d[0..4],
        &d[4..6],
        &d[6..8],
        &d[8..10],
        &d[10..12],
        &d[12..14],
        &d[14..]
    )
    .trim_end()
    .to_owned()
}

/// Operadores da aparência: nome grande à esquerda; à direita "Assinado digitalmente por",
/// nome, data, motivo e local (o leiaute do Acrobat). `w`×`h` é a caixa em pé.
fn appearance_ops(
    name: &str,
    date: &str,
    reason: &str,
    location: &str,
    w: f32,
    h: f32,
) -> Result<String> {
    let font = Font::new("Helvetica")?;
    let half = w / 2.0;
    let pad = 2.0;
    let big = fit_size(&font, name, half - 2.0 * pad, h * 0.45)?;
    let mut ops = format!(
        "q 0 g BT /Helv {big:.2} Tf {pad} {:.2} Td ({}) Tj ET\n",
        (h - big * 0.7) / 2.0,
        crate::stamp::literal(name)
    );
    let mut lines = vec![
        "Assinado digitalmente por".to_owned(),
        name.to_owned(),
        format!("Data: {}", display_date(date)),
    ];
    if !reason.is_empty() {
        lines.push(format!("Motivo: {reason}"));
    }
    if !location.is_empty() {
        lines.push(format!("Local: {location}"));
    }
    let longest = lines
        .iter()
        .map(|l| crate::text_fit::text_width(&font, l, 1.0))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(0.0_f32, f32::max);
    let size = (h / (lines.len() as f32 * 1.2))
        .min((half - 2.0 * pad) / longest.max(0.01))
        .min(10.0);
    let mut y = h - pad - size;
    for l in &lines {
        ops.push_str(&format!(
            "BT /Helv {size:.2} Tf {:.2} {y:.2} Td ({}) Tj ET\n",
            half + pad,
            crate::stamp::literal(l)
        ));
        y -= size * 1.2;
    }
    ops.push('Q');
    Ok(ops)
}

/// CMS SignedData destacado sobre `digest` (SHA-256 do intervalo), com o certificado e a cadeia.
fn build_cms(id: &DigitalId, digest: &[u8]) -> Result<Vec<u8>> {
    let cert = Certificate::from_der(&id.cert_der).map_err(err)?;
    let econtent = EncapsulatedContentInfo {
        econtent_type: const_oid::db::rfc5911::ID_DATA,
        econtent: None,
    };
    let sha256 = AlgorithmIdentifierOwned {
        oid: const_oid::db::rfc5912::ID_SHA_256,
        parameters: None,
    };
    let sid = || {
        SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: cert.tbs_certificate.issuer.clone(),
            serial_number: cert.tbs_certificate.serial_number.clone(),
        })
    };
    let mut builder = SignedDataBuilder::new(&econtent);
    builder.add_digest_algorithm(sha256.clone()).map_err(err)?;
    builder
        .add_certificate(CertificateChoices::Certificate(cert.clone()))
        .map_err(err)?;
    for c in &id.chain {
        let c = Certificate::from_der(c).map_err(err)?;
        builder
            .add_certificate(CertificateChoices::Certificate(c))
            .map_err(err)?;
    }
    match &id.key {
        SigningKey::Rsa(k) => {
            let signer = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new((**k).clone());
            let mut si = SignerInfoBuilder::new(&signer, sid(), sha256, &econtent, Some(digest))
                .map_err(err)?;
            si.add_signed_attribute(create_signing_time_attribute().map_err(err)?)
                .map_err(err)?;
            builder
                .add_signer_info::<_, rsa::pkcs1v15::Signature>(si)
                .map_err(err)?;
        }
        SigningKey::P256(k) => {
            let mut si =
                SignerInfoBuilder::new(k, sid(), sha256, &econtent, Some(digest)).map_err(err)?;
            si.add_signed_attribute(create_signing_time_attribute().map_err(err)?)
                .map_err(err)?;
            builder
                .add_signer_info::<_, p256::ecdsa::DerSignature>(si)
                .map_err(err)?;
        }
    }
    builder.build().map_err(err)?.to_der().map_err(err)
}

/// Resumo dos intervalos com o algoritmo `oid` do CMS.
fn digest_with(oid: &der::oid::ObjectIdentifier, parts: &[&[u8]]) -> Option<Vec<u8>> {
    use const_oid::db::rfc5912::{ID_SHA_1, ID_SHA_256, ID_SHA_384, ID_SHA_512};
    fn run<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
        let mut h = D::new();
        for p in parts {
            h.update(p);
        }
        h.finalize().to_vec()
    }
    Some(match *oid {
        ID_SHA_1 => run::<sha1::Sha1>(parts),
        ID_SHA_256 => run::<sha2::Sha256>(parts),
        ID_SHA_384 => run::<sha2::Sha384>(parts),
        ID_SHA_512 => run::<sha2::Sha512>(parts),
        _ => return None,
    })
}

/// Por que uma assinatura não foi aceita.
enum Fail {
    /// Não confere: a assinatura é inválida.
    Bad(String),
    /// Algoritmo ou chave que este programa não confere.
    Unsupported(String),
}

impl Fail {
    fn integrity(self) -> Integrity {
        match self {
            Fail::Bad(m) => Integrity::Broken(m),
            Fail::Unsupported(m) => Integrity::Unverifiable(m),
        }
    }
}

/// Confere `signature` sobre `signed` com a chave pública do certificado (RSA PKCS#1 v1.5 ou
/// ECDSA P-256), com o resumo `digest_oid`.
fn verify_with_cert(
    cert: &Certificate,
    digest_oid: &der::oid::ObjectIdentifier,
    signed: &[u8],
    signature: &[u8],
) -> std::result::Result<(), Fail> {
    use const_oid::db::rfc5912::{ID_SHA_1, ID_SHA_256, ID_SHA_384, ID_SHA_512};
    use rsa::pkcs8::DecodePublicKey;
    let unsupported = || Fail::Unsupported("algoritmo de resumo não suportado".into());
    let mismatch = || Fail::Bad("a assinatura não confere com o certificado".into());
    let spki = cert
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|e| Fail::Bad(e.to_string()))?;
    let hashed = digest_with(digest_oid, &[signed]).ok_or_else(unsupported)?;
    if let Ok(key) = rsa::RsaPublicKey::from_public_key_der(&spki) {
        let scheme = match *digest_oid {
            ID_SHA_1 => rsa::Pkcs1v15Sign::new::<sha1::Sha1>(),
            ID_SHA_256 => rsa::Pkcs1v15Sign::new::<sha2::Sha256>(),
            ID_SHA_384 => rsa::Pkcs1v15Sign::new::<sha2::Sha384>(),
            ID_SHA_512 => rsa::Pkcs1v15Sign::new::<sha2::Sha512>(),
            _ => return Err(unsupported()),
        };
        return key
            .verify(scheme, &hashed, signature)
            .map_err(|_| mismatch());
    }
    if let Ok(key) = p256::ecdsa::VerifyingKey::from_public_key_der(&spki) {
        use p256::ecdsa::signature::hazmat::PrehashVerifier;
        let sig = p256::ecdsa::DerSignature::try_from(signature)
            .map_err(|_| Fail::Bad("assinatura ECDSA malformada".into()))?;
        // o resumo é o que o CMS pede (SHA-256, SHA-384…), não sempre SHA-256
        return key.verify_prehash(&hashed, &sig).map_err(|_| mismatch());
    }
    Err(Fail::Unsupported(
        "tipo de chave não suportado (só RSA e ECDSA P-256)".into(),
    ))
}

/// Hora da assinatura (atributo assinado signing-time), se houver.
fn signing_time(si: &cms::signed_data::SignerInfo) -> Option<std::time::Duration> {
    si.signed_attrs
        .as_ref()?
        .iter()
        .find(|a| a.oid == const_oid::db::rfc5911::ID_SIGNING_TIME)
        .and_then(|a| a.values.iter().next())
        .and_then(|v| x509_cert::time::Time::from_der(&v.to_der().ok()?).ok())
        .map(|t| t.to_unix_duration())
}

/// Todos os certificados da cadeia valiam no instante `t`?
fn valid_at(chain: &[Certificate], t: std::time::Duration) -> bool {
    chain.iter().all(|c| {
        let v = &c.tbs_certificate.validity;
        v.not_before.to_unix_duration() <= t && t <= v.not_after.to_unix_duration()
    })
}

/// Valida uma assinatura: `/ByteRange` coerente com o arquivo, resumo e assinatura do CMS. Devolve
/// a integridade, o fim do intervalo assinado e os certificados (titular primeiro).
/// Resultado da validação de uma assinatura: integridade, fim do intervalo assinado, cadeia
/// verificada (titular primeiro) e se ela valia na hora da assinatura.
struct Checked {
    integrity: Integrity,
    end: usize,
    chain: Vec<Certificate>,
    valid_then: bool,
}

fn check(file: &[u8], byte_range: &[i64]) -> Checked {
    let broken = |m: &str| Checked {
        integrity: Integrity::Broken(m.into()),
        end: 0,
        chain: Vec::new(),
        valid_then: false,
    };
    let [0, a, b, c] = byte_range else {
        return broken("/ByteRange malformado");
    };
    let (a, b, c) = (*a, *b, *c);
    if a < 2 || c < 0 || b < a + 2 || (b as u64).saturating_add(c as u64) > file.len() as u64 {
        return broken("/ByteRange fora do arquivo");
    }
    let (a, b, c) = (a as usize, b as usize, c as usize);
    let gap = &file[a..b];
    if gap.first() != Some(&b'<') || gap.last() != Some(&b'>') {
        return broken("/Contents fora do lugar esperado");
    }
    let hex: Vec<u8> = gap[1..gap.len() - 1]
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let Some(der_bytes) = hex
        .chunks(2)
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
    else {
        return broken("/Contents não é hexadecimal");
    };
    // o CMS vem seguido de zeros de preenchimento: lê só o primeiro objeto DER
    let Ok(ci) = SliceReader::new(&der_bytes).and_then(|mut r| ContentInfo::decode(&mut r)) else {
        return broken("assinatura malformada");
    };
    let Ok(sd) = ci.content.decode_as::<SignedData>() else {
        return broken("assinatura malformada");
    };
    let certs: Vec<Certificate> = sd
        .certificates
        .iter()
        .flat_map(|set| set.0.iter())
        .filter_map(|c| match c {
            CertificateChoices::Certificate(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let Some(si) = sd.signer_infos.0.iter().next() else {
        return broken("assinatura sem assinante");
    };
    let leaf = certs.iter().find(|cert| match &si.sid {
        SignerIdentifier::IssuerAndSerialNumber(is) => {
            cert.tbs_certificate.issuer == is.issuer
                && cert.tbs_certificate.serial_number == is.serial_number
        }
        SignerIdentifier::SubjectKeyIdentifier(ski) => {
            ski_of(cert).as_deref() == Some(ski.0.as_bytes())
        }
    });
    let Some(leaf) = leaf else {
        return broken("certificado do assinante ausente");
    };
    // só a cadeia de emissão verificada conta (um certificado qualquer embutido não)
    let ordered = verified_chain(leaf.clone(), &certs);
    let parts = [&file[..a], &file[b..b + c]];
    let Some(digest) = digest_with(&si.digest_alg.oid, &parts) else {
        return Checked {
            integrity: Integrity::Unverifiable("algoritmo de resumo não suportado".into()),
            end: b + c,
            chain: ordered,
            valid_then: false,
        };
    };
    let integrity = match &si.signed_attrs {
        Some(attrs) => {
            let md = attrs
                .iter()
                .find(|a| a.oid == const_oid::db::rfc5911::ID_MESSAGE_DIGEST)
                .and_then(|a| a.values.iter().next())
                .and_then(|v| v.decode_as::<der::asn1::OctetStringRef<'_>>().ok());
            match md {
                Some(md) if md.as_bytes() == digest.as_slice() => match attrs.to_der() {
                    Ok(signed) => verify_with_cert(
                        &ordered[0],
                        &si.digest_alg.oid,
                        &signed,
                        si.signature.as_bytes(),
                    )
                    .map_or_else(Fail::integrity, |()| Integrity::Intact),
                    Err(_) => Integrity::Broken("assinatura malformada".into()),
                },
                Some(_) => Integrity::Broken("o documento foi alterado depois de assinado".into()),
                None => Integrity::Broken("assinatura sem resumo do documento".into()),
            }
        }
        // sem atributos: assina direto o resumo (raro)
        None => verify_with_cert(
            &ordered[0],
            &si.digest_alg.oid,
            &[&file[..a], &file[b..b + c]].concat(),
            si.signature.as_bytes(),
        )
        .map_or_else(Fail::integrity, |()| Integrity::Intact),
    };
    // SHA-1 já não é seguro: o Acrobat também não dá essas assinaturas como válidas
    let integrity = match integrity {
        Integrity::Intact if si.digest_alg.oid == const_oid::db::rfc5912::ID_SHA_1 => {
            Integrity::Unverifiable("resumo SHA-1, que não é mais considerado seguro".into())
        }
        other => other,
    };
    let when = signing_time(si).unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
    });
    Checked {
        valid_then: valid_at(&ordered, when),
        integrity,
        end: b + c,
        chain: ordered,
    }
}

/// SubjectKeyIdentifier (extensão 2.5.29.14) do certificado.
fn ski_of(cert: &Certificate) -> Option<Vec<u8>> {
    cert.tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        .find(|e| e.extn_id == const_oid::db::rfc5280::ID_CE_SUBJECT_KEY_IDENTIFIER)
        .and_then(|e| {
            der::asn1::OctetStringRef::from_der(e.extn_value.as_bytes())
                .ok()
                .map(|o| o.as_bytes().to_vec())
        })
}

/// `child` foi emitido (assinado) por `issuer`?
fn issued_by(child: &Certificate, issuer: &Certificate) -> bool {
    use const_oid::db::rfc5912::*;
    if child.tbs_certificate.issuer != issuer.tbs_certificate.subject {
        return false;
    }
    let digest = match child.signature_algorithm.oid {
        SHA_1_WITH_RSA_ENCRYPTION => ID_SHA_1,
        SHA_256_WITH_RSA_ENCRYPTION | ECDSA_WITH_SHA_256 => ID_SHA_256,
        SHA_384_WITH_RSA_ENCRYPTION | ECDSA_WITH_SHA_384 => ID_SHA_384,
        SHA_512_WITH_RSA_ENCRYPTION | ECDSA_WITH_SHA_512 => ID_SHA_512,
        _ => return false,
    };
    let (Ok(tbs), Some(sig)) = (child.tbs_certificate.to_der(), child.signature.as_bytes()) else {
        return false;
    };
    verify_with_cert(issuer, &digest, &tbs, sig).is_ok()
}

/// O certificado é de uma autoridade certificadora (extensão basicConstraints com cA)?
fn is_ca(cert: &Certificate) -> bool {
    use x509_cert::ext::pkix::BasicConstraints;
    cert.tbs_certificate
        .extensions
        .as_ref()
        .is_some_and(|exts| {
            exts.iter().any(|e| {
                e.extn_id == const_oid::db::rfc5280::ID_CE_BASIC_CONSTRAINTS
                    && BasicConstraints::from_der(e.extn_value.as_bytes()).is_ok_and(|b| b.ca)
            })
        })
}

/// Cadeia a partir do titular, só com emissores cuja assinatura confere (até 8 níveis).
fn verified_chain(leaf: Certificate, pool: &[Certificate]) -> Vec<Certificate> {
    let mut chain = vec![leaf];
    for _ in 0..8 {
        let cur = chain.last().expect("começa com o titular");
        if cur.tbs_certificate.issuer == cur.tbs_certificate.subject {
            break; // raiz (autoassinado)
        }
        // emissor só se for autoridade certificadora (basicConstraints cA) e a assinatura conferir
        match pool
            .iter()
            .find(|p| !chain.contains(p) && is_ca(p) && issued_by(cur, p))
        {
            Some(issuer) => chain.push(issuer.clone()),
            None => break,
        }
    }
    chain
}

/// Motivo, local e data da versão assinada (uma revisão posterior pode tê-los trocado).
fn signed_details(
    file: &[u8],
    end: usize,
    password: Option<&str>,
    page: i32,
    field: &str,
) -> Option<(String, String, String)> {
    let mut doc = mupdf::pdf::PdfDocument::from_bytes(file.get(..end)?).ok()?;
    if let Some(p) = password {
        doc.authenticate(p).ok()?;
    }
    let page = doc.load_pdf_page(page).ok()?;
    let w = page
        .widgets()
        .find(|w| w.name().ok().flatten().as_deref() == Some(field))?;
    let v = w.annotation().object().get_dict_inheritable("V").ok()??;
    let get = |k: &str| text_of(v.get_dict(k).ok().flatten()).ok();
    Some((get("M")?, get("Reason")?, get("Location")?))
}

/// O que a página mostra, sem anotações nem campos: a imagem renderizada (meia escala). Comparar a
/// imagem pega também a troca de um recurso (fonte, imagem, formulário) com o /Contents igual.
fn page_fingerprint(doc: &mupdf::pdf::PdfDocument, page: i32) -> Option<Vec<u8>> {
    let page = doc.load_page(page).ok()?;
    let pix = page
        .to_pixmap(
            &mupdf::Matrix::new_scale(0.5, 0.5),
            &mupdf::Colorspace::device_gray(),
            false,
            false, // sem anotações: elas são alterações permitidas
        )
        .ok()?;
    let mut out = format!("{}x{}", pix.width(), pix.height()).into_bytes();
    out.extend(sha2::Sha256::digest(pix.samples()));
    Some(out)
}

/// Anotações da página que não são campos: tipo, retângulo, flags e a aparência (resumo). Elas
/// também mudam o que se vê (uma anotação opaca por cima do texto assinado).
fn annots_fingerprint(doc: &mupdf::pdf::PdfDocument, page: i32) -> Option<Vec<u8>> {
    let page = doc.load_pdf_page(page).ok()?;
    let mut h = sha2::Sha256::new();
    for a in page.annotations() {
        let obj = a.object();
        for key in ["Subtype", "Rect", "F", "C", "IC", "CA"] {
            h.update(
                obj.get_dict(key)
                    .ok()?
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            h.update(b"|");
        }
        if let Some(n) = obj
            .get_dict("AP")
            .ok()?
            .and_then(|ap| ap.get_dict("N").ok().flatten())
        {
            if n.is_stream().ok()? {
                h.update(n.read_stream().ok()?);
            } else {
                h.update(n.to_string());
            }
        }
        h.update(b";");
    }
    Some(h.finalize().to_vec())
}

/// As revisões depois de `end` só preencheram campos ou assinaram (o nível "preenchimento e
/// assinatura" do Acrobat)?
/// O conteúdo de cada página e o número de páginas são os da versão assinada.
fn only_fields_changed(file: &[u8], end: usize, password: Option<&str>) -> bool {
    let open = |bytes: &[u8]| -> Option<mupdf::pdf::PdfDocument> {
        let mut d = mupdf::pdf::PdfDocument::from_bytes(bytes).ok()?;
        if let Some(p) = password {
            d.authenticate(p).ok()?;
        }
        Some(d)
    };
    let (Some(signed), Some(now)) = (file.get(..end).and_then(open), open(file)) else {
        return false;
    };
    let (Ok(n), Ok(m)) = (signed.page_count(), now.page_count()) else {
        return false;
    };
    n == m
        && (0..n).all(|p| {
            let a = page_fingerprint(&signed, p);
            let b = annots_fingerprint(&signed, p);
            a.is_some()
                && b.is_some()
                && a == page_fingerprint(&now, p)
                && b == annots_fingerprint(&now, p)
        })
}

fn text_of(o: Option<PdfObject>) -> Result<String> {
    Ok(match o {
        Some(o) if o.is_string()? => o.as_string()?,
        _ => String::new(),
    })
}

impl PdfDoc {
    /// O documento tem alguma assinatura (Salvar passa a ser incremental).
    pub fn has_signatures(&self) -> Result<bool> {
        for p in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(p)?.widgets() {
                let obj = w.annotation().object();
                if obj
                    .get_dict_inheritable("V")?
                    .is_some_and(|v| v.get_dict("ByteRange").ok().flatten().is_some())
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Assinaturas do documento, validadas contra os bytes do arquivo no disco.
    pub fn signatures(&self) -> Result<Vec<SignatureInfo>> {
        // sem assinaturas, nada de ler o arquivo inteiro (PDFs grandes)
        if !self.signed_on_disk && !self.has_signatures()? {
            return Ok(Vec::new());
        }
        let file = std::fs::read(&self.path)?;
        let mut out = Vec::new();
        for page in 0..self.page_count()? {
            for w in self.inner.load_pdf_page(page)?.widgets() {
                let obj = w.annotation().object();
                let Some(v) = obj.get_dict_inheritable("V")? else {
                    continue;
                };
                let Some(range) = v.get_dict("ByteRange")? else {
                    continue;
                };
                let mut br = Vec::new();
                for i in 0..range.len()?.min(8) as i32 {
                    if let Some(n) = range.get_array(i)? {
                        br.push(n.as_int().map(i64::from).unwrap_or(-1));
                    }
                }
                let Checked {
                    integrity,
                    end,
                    chain: certs,
                    valid_then,
                } = check(&file, &br);
                // titular só do certificado (o /Name do dicionário não é verificado)
                let (signer, email) = certs.first().map(subject_of).unwrap_or_default();
                let field = w.name().ok().flatten().unwrap_or_default();
                let modified_after = integrity == Integrity::Intact && end < trimmed_len(&file);
                let current = || -> Result<(String, String, String)> {
                    Ok((
                        text_of(v.get_dict("M")?)?,
                        text_of(v.get_dict("Reason")?)?,
                        text_of(v.get_dict("Location")?)?,
                    ))
                };
                let (signed_at, reason, location) = match modified_after {
                    // sem a versão assinada, nada de mostrar os detalhes atuais (podem ser forjados)
                    true => signed_details(&file, end, self.password.as_deref(), page, &field)
                        .unwrap_or_default(),
                    false => current()?,
                };
                out.push(SignatureInfo {
                    field,
                    page,
                    id: annot_id(w.annotation())?,
                    signer,
                    email,
                    signed_at,
                    reason,
                    location,
                    modified_after,
                    integrity,
                    changes_permitted: modified_after
                        && only_fields_changed(&file, end, self.password.as_deref()),
                    cert_valid_at_signing: valid_then,
                    fingerprints: certs
                        .iter()
                        .filter_map(|c| c.to_der().ok())
                        .map(|d| fingerprint(&d))
                        .collect(),
                });
            }
        }
        Ok(out)
    }

    /// Assina com o ID digital e grava o arquivo (pendências antes; sempre incremental).
    pub fn sign_digitally(
        &mut self,
        place: SignPlace,
        id: &DigitalId,
        reason: &str,
        location: &str,
    ) -> Result<()> {
        let path = self.path.clone();
        // pendências primeiro: a assinatura cobre o arquivo como está no disco
        if self.history_state().dirty || self.history.can_undo() {
            self.save(&path)?;
        }
        // a gravação incremental parte do arquivo que o MuPDF abriu: reabre o do disco (depois de
        // salvar, ou de um "Salvar como", o aberto é outro)
        self.discard_changes()?;
        // arquivo reparado pelo MuPDF (tabela xref quebrada) não aceita gravação incremental:
        // sem assinaturas ainda, grava uma vez por inteiro (consertado) e reabre
        let mut probe = PdfWriteOptions::default();
        probe.set_incremental(true);
        if self
            .inner
            .write_to_with_options(&mut Vec::new(), probe)
            .is_err()
        {
            if self.signed_on_disk {
                return Err(err(
                    "o arquivo está danificado e já tem assinaturas: não dá para acrescentar outra",
                ));
            }
            let mut keep = PdfWriteOptions::default();
            keep.set_encryption(mupdf::pdf::Encryption::Keep);
            crate::doc::write_atomic(&self.inner, &path, keep)?;
            self.discard_changes()?;
        }
        let (page, widget_id) = match place {
            SignPlace::Field { page, id } => (page, id),
            SignPlace::New { page, rect } => {
                self.check_page(page)?;
                let name = self.free_signature_name()?;
                let id = self.add_field(page, rect, NewFieldKind::Signature, &name)?;
                (page, id)
            }
        };
        let result = self.write_signature(page, widget_id, id, reason, location);
        // o documento em memória volta a ser o do disco (assinado, ou como estava se falhou)
        self.discard_changes()?;
        result
    }

    fn free_signature_name(&self) -> Result<String> {
        let taken: Vec<String> = (0..self.page_count()?)
            .map(|p| self.fields(p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .map(|f| f.name)
            .collect();
        Ok((1..)
            .map(|n| format!("Assinatura{n}"))
            .find(|n| !taken.contains(n))
            .expect("sequência infinita"))
    }

    fn write_signature(
        &mut self,
        page: i32,
        widget_id: i32,
        id: &DigitalId,
        reason: &str,
        location: &str,
    ) -> Result<()> {
        let page_ref = self.inner.load_pdf_page(page)?;
        let rotation = page_ref.rotation()?.rem_euclid(360);
        let w = page_ref
            .widgets()
            .find(|w| annot_id(w.annotation()).ok() == Some(widget_id))
            .ok_or_else(|| err("campo de assinatura não encontrado"))?;
        let mut obj = w.annotation().object();
        // oculto (/F 2 ou 32): a assinatura ficaria invisível para quem lê
        if crate::forms::hidden(&obj)? {
            return Err(err("o campo de assinatura está oculto"));
        }
        let is_sig = obj
            .get_dict_inheritable("FT")?
            .map(|t| t.as_name())
            .transpose()?
            .as_deref()
            == Some(b"Sig");
        if !is_sig {
            return Err(err("o campo não é de assinatura"));
        }
        if obj.get_dict_inheritable("V")?.is_some() {
            return Err(err("o campo já está assinado"));
        }
        let now = pdf_now();
        let zeros = "0".repeat(CONTENTS_BYTES * 2);
        let mut sig = self.inner.new_object_from_str(&format!(
            "<</Type/Sig/Filter/Adobe.PPKLite/SubFilter/adbe.pkcs7.detached\
             /ByteRange{BYTE_RANGE_PLACEHOLDER}/Contents<{zeros}>>>"
        ))?;
        sig.dict_put("Name", PdfObject::new_string(&id.name)?)?;
        sig.dict_put("M", PdfObject::new_string(&now)?)?;
        if !reason.is_empty() {
            sig.dict_put("Reason", PdfObject::new_string(reason)?)?;
        }
        if !location.is_empty() {
            sig.dict_put("Location", PdfObject::new_string(location)?)?;
        }
        let sig = self.inner.add_object(&sig)?;
        crate::forms::field_head(&obj)?.dict_put("V", sig)?;
        // aparência: o leiaute do Acrobat, em pé na página girada
        let rect: PdfRect = w.annotation().rect()?.into();
        let (bw, bh) = if rotation % 180 == 0 {
            (rect.width(), rect.height())
        } else {
            (rect.height(), rect.width())
        };
        let ops = appearance_ops(&id.name, &now, reason, location, bw, bh)?;
        let m = mupdf::Matrix::new_rotate(rotation as f32);
        let form = self.inner.new_object_from_str(&format!(
            "<</Type/XObject/Subtype/Form/BBox[0 0 {bw} {bh}]/Matrix[{} {} {} {} 0 0]\
             /Resources<</Font<</Helv<</Type/Font/Subtype/Type1/BaseFont/Helvetica\
             /Encoding/WinAnsiEncoding>>>>>>>>",
            m.a, m.b, m.c, m.d
        ))?;
        let stream =
            self.inner
                .add_stream(&Buffer::from_bytes(ops.as_bytes())?, Some(&form), false)?;
        let mut ap = self.inner.new_dict()?;
        ap.dict_put("N", stream)?;
        obj.dict_put("AP", ap)?;
        if let Some(mut acro) = self.inner.catalog()?.get_dict("AcroForm")? {
            acro.dict_put("SigFlags", PdfObject::new_int(SIG_FLAGS)?)?;
        }
        // gravação incremental: o arquivo do disco + a revisão com a assinatura
        let mut opts = PdfWriteOptions::default();
        opts.set_incremental(true);
        let mut bytes = self.with_file_layers(|d| {
            let mut bytes = Vec::new();
            d.inner.write_to_with_options(&mut bytes, opts)?;
            Ok(bytes)
        })?;
        let original = std::fs::read(&self.path)?;
        if !bytes.starts_with(&original) {
            return Err(err(
                "não foi possível gravar a assinatura de forma incremental",
            ));
        }
        // a reserva está na revisão nova (a última ocorrência)
        let tail = original.len();
        let br_at = tail
            + find(&bytes[tail..], BYTE_RANGE_PLACEHOLDER.as_bytes())
                .ok_or_else(|| err("reserva do /ByteRange não encontrada"))?;
        let contents = format!("<{zeros}>");
        let a = tail
            + find(&bytes[tail..], contents.as_bytes())
                .ok_or_else(|| err("reserva do /Contents não encontrada"))?;
        let b = a + contents.len();
        let c = bytes.len() - b;
        let range = format!("[0 {a} {b} {c}]");
        let range = format!("{range:<width$}", width = BYTE_RANGE_PLACEHOLDER.len());
        bytes[br_at..br_at + range.len()].copy_from_slice(range.as_bytes());
        let digest = sha2::Sha256::digest([&bytes[..a], &bytes[b..]].concat());
        let cms = build_cms(id, &digest)?;
        if cms.len() > CONTENTS_BYTES {
            return Err(err("a assinatura não coube no espaço reservado"));
        }
        let hex: String = cms.iter().map(|x| format!("{x:02X}")).collect();
        bytes[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
        crate::doc::write_bytes_atomic(&self.path, &bytes)
    }
}

/// Tamanho do arquivo sem espaços/quebras no fim (o fim de uma revisão é "%%EOF\n").
fn trimmed_len(file: &[u8]) -> usize {
    file.iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(0, |i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digital_id::SigningKey;

    #[test]
    fn pdf_dates_are_shown_like_acrobat() {
        assert_eq!(
            display_date("D:20261010153000-03'00'"),
            "2026.10.10 15:30:00 -03'00'"
        );
        assert_eq!(display_date("lixo"), "lixo");
        // acento na posição 14 (PDF malicioso): sem pânico
        assert_eq!(display_date("D:1234567890123é"), "D:1234567890123é");
    }

    #[test]
    fn a_certificate_that_is_not_a_ca_cannot_issue_others() {
        use x509_cert::builder::{Builder, CertificateBuilder, Profile};
        use x509_cert::spki::SubjectPublicKeyInfoOwned;
        let dir = tempfile::tempdir().unwrap();
        // A: ID comum (folha, sem cA); B: certificado "emitido" pela chave de A
        let a = DigitalId::create_self_signed("A", "", "", "x", &dir.path().join("a.pfx")).unwrap();
        let a_cert = Certificate::from_der(&a.cert_der).unwrap();
        let SigningKey::Rsa(a_key) = &a.key else {
            panic!()
        };
        let signer = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new((**a_key).clone());
        let b_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
        let spki = SubjectPublicKeyInfoOwned::from_key(b_key.to_public_key()).unwrap();
        let subject: x509_cert::name::Name = "CN=B".parse().unwrap();
        let validity =
            x509_cert::time::Validity::from_now(std::time::Duration::from_secs(3600)).unwrap();
        let serial = x509_cert::serial_number::SerialNumber::new(&[7]).unwrap();
        let profile = Profile::Leaf {
            issuer: a_cert.tbs_certificate.subject.clone(),
            enable_key_agreement: false,
            enable_key_encipherment: false,
        };
        let b = CertificateBuilder::new(profile, serial, validity, subject, spki, &signer)
            .unwrap()
            .build::<rsa::pkcs1v15::Signature>()
            .unwrap();
        assert!(issued_by(&b, &a_cert), "a assinatura em si confere");
        assert_eq!(
            verified_chain(b.clone(), &[a_cert]).len(),
            1,
            "A não é autoridade certificadora"
        );
    }

    #[test]
    fn byte_ranges_outside_the_file_are_broken() {
        let file = b"%PDF-1.7 <00> fim";
        for br in [
            vec![0, 9, 13, 999],
            vec![0, -1, 3, 1],
            vec![1, 2, 3, 4],
            vec![0, 9],
        ] {
            assert!(
                matches!(check(file, &br).integrity, Integrity::Broken(_)),
                "{br:?}"
            );
        }
    }
}
