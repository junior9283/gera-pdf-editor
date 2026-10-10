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

/// Texto PDF literal (WinAnsi), para a aparência.
fn literal(s: &str) -> String {
    crate::stamp::literal(s)
}

/// Fonte que cabe em `width` pt para `text` em Helvetica, até `max`.
fn fit_size(font: &Font, text: &str, width: f32, max: f32) -> Result<f32> {
    let w = crate::text_fit::text_width(font, text, 1.0)?;
    Ok(if w <= 0.0 { max } else { (width / w).min(max) })
}

/// "D:20261010153000-03'00'" → "2026.10.10 15:30:00 -03'00'" (como o Acrobat mostra).
pub fn display_date(pdf: &str) -> String {
    let d = pdf.trim_start_matches("D:");
    if d.len() < 14 || !d[..14].bytes().all(|b| b.is_ascii_digit()) {
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
        literal(name)
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
            literal(l)
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

/// Confere a assinatura do `signed` (atributos assinados) com a chave pública do certificado.
fn verify_with_cert(
    cert: &Certificate,
    digest_oid: &der::oid::ObjectIdentifier,
    signed: &[u8],
    signature: &[u8],
) -> std::result::Result<(), String> {
    use rsa::pkcs8::DecodePublicKey;
    let spki = cert
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|e| e.to_string())?;
    if let Ok(key) = rsa::RsaPublicKey::from_public_key_der(&spki) {
        use const_oid::db::rfc5912::{ID_SHA_1, ID_SHA_256, ID_SHA_384, ID_SHA_512};
        let hashed =
            digest_with(digest_oid, &[signed]).ok_or("algoritmo de resumo desconhecido")?;
        let scheme = match *digest_oid {
            ID_SHA_1 => rsa::Pkcs1v15Sign::new::<sha1::Sha1>(),
            ID_SHA_256 => rsa::Pkcs1v15Sign::new::<sha2::Sha256>(),
            ID_SHA_384 => rsa::Pkcs1v15Sign::new::<sha2::Sha384>(),
            ID_SHA_512 => rsa::Pkcs1v15Sign::new::<sha2::Sha512>(),
            _ => return Err("algoritmo de resumo desconhecido".into()),
        };
        return key
            .verify(scheme, &hashed, signature)
            .map_err(|_| "a assinatura não confere com o certificado".to_owned());
    }
    if let Ok(key) = p256::ecdsa::VerifyingKey::from_public_key_der(&spki) {
        use p256::ecdsa::signature::Verifier;
        let sig = p256::ecdsa::DerSignature::try_from(signature)
            .map_err(|_| "assinatura ECDSA malformada")?;
        return key
            .verify(signed, &sig)
            .map_err(|_| "a assinatura não confere com o certificado".to_owned());
    }
    Err("tipo de chave do certificado não suportado".into())
}

/// Valida uma assinatura: `/ByteRange` coerente com o arquivo, resumo e assinatura do CMS. Devolve
/// a integridade, o fim do intervalo assinado e os certificados (titular primeiro).
fn check(file: &[u8], byte_range: &[i64]) -> (Integrity, usize, Vec<Certificate>) {
    let broken = |m: &str| (Integrity::Broken(m.into()), 0, Vec::new());
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
    let leaf = certs.iter().position(|cert| match &si.sid {
        SignerIdentifier::IssuerAndSerialNumber(is) => {
            cert.tbs_certificate.issuer == is.issuer
                && cert.tbs_certificate.serial_number == is.serial_number
        }
        SignerIdentifier::SubjectKeyIdentifier(_) => true,
    });
    let Some(leaf) = leaf else {
        return broken("certificado do assinante ausente");
    };
    let mut ordered = certs.clone();
    let leaf_cert = ordered.remove(leaf);
    ordered.insert(0, leaf_cert);
    let parts = [&file[..a], &file[b..b + c]];
    let Some(digest) = digest_with(&si.digest_alg.oid, &parts) else {
        return (
            Integrity::Broken("algoritmo de resumo desconhecido".into()),
            b + c,
            ordered,
        );
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
                    .map_or_else(Integrity::Broken, |()| Integrity::Intact),
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
        .map_or_else(Integrity::Broken, |()| Integrity::Intact),
    };
    (integrity, b + c, ordered)
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
                let (integrity, end, certs) = check(&file, &br);
                let (signer, email) = certs.first().map(subject_of).unwrap_or_else(|| {
                    (
                        text_of(v.get_dict("Name").ok().flatten()).unwrap_or_default(),
                        String::new(),
                    )
                });
                out.push(SignatureInfo {
                    field: w.name().ok().flatten().unwrap_or_default(),
                    page,
                    id: annot_id(w.annotation())?,
                    signer,
                    email,
                    signed_at: text_of(v.get_dict("M")?)?,
                    reason: text_of(v.get_dict("Reason")?)?,
                    location: text_of(v.get_dict("Location")?)?,
                    modified_after: integrity == Integrity::Intact && end < trimmed_len(&file),
                    integrity,
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
            // a gravação incremental parte do arquivo que o MuPDF abriu: reabre o recém-salvo
            self.reload()?;
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
        self.reload()?;
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
        let mut bytes = Vec::new();
        self.inner.write_to_with_options(&mut bytes, opts)?;
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

    #[test]
    fn pdf_dates_are_shown_like_acrobat() {
        assert_eq!(
            display_date("D:20261010153000-03'00'"),
            "2026.10.10 15:30:00 -03'00'"
        );
        assert_eq!(display_date("lixo"), "lixo");
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
            assert!(matches!(check(file, &br).0, Integrity::Broken(_)), "{br:?}");
        }
    }
}
