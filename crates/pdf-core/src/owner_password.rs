//! A senha confere como senha de dono (permissões)? Algoritmos do Standard Security Handler da
//! especificação PDF (ISO 32000): 7 (R2–R4, MD5 + RC4) e 2.B (R6, SHA-2 + AES). O MuPDF do binding
//! só diz se a senha foi aceita, não como qual.
use md5::Md5;
use sha2::{Digest, Sha256, Sha384, Sha512};

/// Preenchimento padrão das senhas (R2–R4).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Dados do dicionário /Encrypt que a verificação usa.
pub(crate) struct EncryptDict {
    pub r: i32,
    pub o: Vec<u8>,
    pub u: Vec<u8>,
    pub p: i32,
    /// Primeiro elemento do /ID do trailer.
    pub id0: Vec<u8>,
    /// Tamanho da chave em bytes (R2: 5).
    pub key_len: usize,
    pub encrypt_metadata: bool,
}

/// RC4 (só para conferir senhas de arquivos antigos; nunca para cifrar).
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    if key.is_empty() {
        return Vec::new();
    }
    let mut st: Vec<u8> = (0..=255).collect();
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(st[i]).wrapping_add(key[i % key.len()]);
        st.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(st[usize::from(i)]);
            st.swap(usize::from(i), usize::from(j));
            b ^ st[usize::from(st[usize::from(i)].wrapping_add(st[usize::from(j)]))]
        })
        .collect()
}

/// Senha em PDFDocEncoding (R2–R4): Latin-1 para o que couber, "?" no resto; 32 bytes, completada
/// com o preenchimento padrão.
fn padded(password: &str) -> [u8; 32] {
    let bytes: Vec<u8> = password
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .take(32)
        .collect();
    let mut out = PAD;
    out[..bytes.len()].copy_from_slice(&bytes);
    if bytes.len() < 32 {
        out[bytes.len()..].copy_from_slice(&PAD[..32 - bytes.len()]);
    }
    out
}

/// Algoritmo 2: chave do arquivo a partir da senha de usuário (já completada).
fn file_key(e: &EncryptDict, user_padded: &[u8]) -> Vec<u8> {
    let mut h = Md5::new();
    h.update(user_padded);
    h.update(&e.o[..32.min(e.o.len())]);
    h.update(e.p.to_le_bytes());
    h.update(&e.id0);
    if e.r >= 4 && !e.encrypt_metadata {
        h.update([0xff; 4]);
    }
    let mut key = h.finalize().to_vec();
    if e.r >= 3 {
        for _ in 0..50 {
            key = Md5::digest(&key[..e.key_len]).to_vec();
        }
    }
    key.truncate(e.key_len);
    key
}

/// Algoritmos 4/5: a senha de usuário (completada) confere com /U?
fn user_matches(e: &EncryptDict, user_padded: &[u8]) -> bool {
    let key = file_key(e, user_padded);
    if e.r == 2 {
        return rc4(&key, &PAD) == e.u.get(..32).unwrap_or_default();
    }
    let mut h = Md5::new();
    h.update(PAD);
    h.update(&e.id0);
    let mut x = rc4(&key, &h.finalize());
    for i in 1..=19u8 {
        let k: Vec<u8> = key.iter().map(|b| b ^ i).collect();
        x = rc4(&k, &x);
    }
    x.get(..16) == e.u.get(..16)
}

/// Algoritmo 7 (R2–R4): decifra a senha de usuário guardada em /O com a candidata a dono e
/// confere essa senha de usuário.
fn owner_matches_rc4(e: &EncryptDict, password: &str) -> bool {
    if e.o.len() < 32 || e.u.len() < 16 || !(2..=4).contains(&e.r) {
        return false;
    }
    let mut key = Md5::digest(padded(password)).to_vec();
    if e.r >= 3 {
        for _ in 0..50 {
            key = Md5::digest(&key).to_vec();
        }
    }
    key.truncate(e.key_len);
    let mut user = e.o[..32].to_vec();
    if e.r == 2 {
        user = rc4(&key, &user);
    } else {
        for i in (0..=19u8).rev() {
            let k: Vec<u8> = key.iter().map(|b| b ^ i).collect();
            user = rc4(&k, &user);
        }
    }
    user.len() == 32 && user_matches(e, &user)
}

/// Algoritmo 2.B (R6): resumo da senha com sal e, para o dono, os 48 bytes de /U.
fn hash_r6(password: &[u8], salt: &[u8], udata: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncryptMut, KeyIvInit};
    type Enc = cbc::Encryptor<aes::Aes128>;
    let mut k = Sha256::new()
        .chain_update(password)
        .chain_update(salt)
        .chain_update(udata)
        .finalize()
        .to_vec();
    let mut round = 0u32;
    loop {
        let unit: Vec<u8> = [password, &k, udata].concat();
        let mut e = unit.repeat(64);
        let enc = Enc::new_from_slices(&k[..16], &k[16..32]).expect("16 bytes");
        let blocks = e.len() / 16;
        enc.encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut e, blocks * 16)
            .expect("múltiplo de 16");
        let sum: u32 = e[..16].iter().map(|&b| u32::from(b)).sum();
        k = match sum % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last().expect("não vazio")) <= round - 32 {
            break;
        }
    }
    k.truncate(32);
    k
}

/// A senha confere como senha de dono?
pub(crate) fn is_owner_password(e: &EncryptDict, password: &str) -> bool {
    match e.r {
        2..=4 => owner_matches_rc4(e, password),
        5 | 6 => {
            if e.o.len() < 48 || e.u.len() < 48 {
                return false;
            }
            // R6: UTF-8 (SASLprep não aplicado), até 127 bytes
            let pw = &password.as_bytes()[..password.len().min(127)];
            let hash = if e.r == 5 {
                Sha256::new()
                    .chain_update(pw)
                    .chain_update(&e.o[32..40])
                    .chain_update(&e.u[..48])
                    .finalize()
                    .to_vec()
            } else {
                hash_r6(pw, &e.o[32..40], &e.u[..48])
            };
            hash == e.o[..32]
        }
        _ => false,
    }
}

/// /Encrypt do documento (com o /ID do trailer), se ele é protegido pelo Standard Security Handler.
pub(crate) fn encrypt_dict(doc: &mupdf::pdf::PdfDocument) -> crate::Result<Option<EncryptDict>> {
    let trailer = doc.trailer()?;
    let Some(enc) = trailer.get_dict("Encrypt")? else {
        return Ok(None);
    };
    let bytes = |k: &str| -> crate::Result<Vec<u8>> {
        Ok(enc
            .get_dict(k)?
            .map(|v| v.as_bytes())
            .transpose()?
            .unwrap_or_default())
    };
    let int = |k: &str, d: i32| -> crate::Result<i32> {
        Ok(enc
            .get_dict(k)?
            .map(|v| v.as_int())
            .transpose()?
            .unwrap_or(d))
    };
    let r = int("R", 0)?;
    let id0 = trailer
        .get_dict("ID")?
        .and_then(|id| id.get_array(0).ok().flatten())
        .map(|v| v.as_bytes())
        .transpose()?
        .unwrap_or_default();
    let key_len = if r == 2 {
        5
    } else {
        (int("Length", 40)? / 8).clamp(5, 16) as usize
    };
    Ok(Some(EncryptDict {
        r,
        o: bytes("O")?,
        u: bytes("U")?,
        p: int("P", 0)?,
        id0,
        key_len,
        encrypt_metadata: enc
            .get_dict("EncryptMetadata")?
            .map(|v| v.as_bool())
            .transpose()?
            .unwrap_or(true),
    }))
}
