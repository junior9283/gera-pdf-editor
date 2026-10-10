//! Peças comuns para mexer no conteúdo e nos recursos da página (Editar PDF, marca d'água,
//! cabeçalho e rodapé): leitura do conteúdo, tokenizador, matrizes e recursos próprios.
use crate::error::{refused, Result};
use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Matrix;

/// Os seis números da matriz, para o operador `cm` ou `Tm`.
pub(crate) fn matrix_ops(m: &Matrix) -> String {
    [m.a, m.b, m.c, m.d, m.e, m.f].map(num).join(" ")
}

/// Cor de preenchimento RGB (`r g b rg`).
pub(crate) fn rgb_ops(color: [u8; 3]) -> String {
    let [r, g, b] = color.map(|c| num(c as f32 / 255.0));
    format!("{r} {g} {b} rg")
}

/// Primeiro nome `prefixN` livre no dicionário (de recursos).
pub(crate) fn free_name(dict: &PdfObject, prefix: &str) -> Result<String> {
    for n in 0.. {
        let name = format!("{prefix}{n}");
        if dict.get_dict(name.as_str())?.is_none() {
            return Ok(name);
        }
    }
    unreachable!("sempre há um nome livre")
}

/// Inversa da matriz (recusa as degeneradas, de PDF malformado).
pub(crate) fn invert(m: &Matrix) -> Result<Matrix> {
    m.invert()
        .ok_or_else(|| refused("transformação inválida na página"))
}

/// Número para o conteúdo da página: até 6 casas, sem zeros sobrando.
pub(crate) fn num(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// O dicionário `kind` (/XObject, /Font) próprio dos recursos da página, para acrescentar ou tirar
/// itens: os recursos (herdados ou de outras páginas) são copiados, para não mudar as outras páginas.
pub(crate) fn own_resource(
    doc: &PdfDocument,
    page_obj: &mut PdfObject,
    kind: &str,
) -> Result<PdfObject> {
    let copy = |o: Option<PdfObject>| -> Result<PdfObject> {
        Ok(match o {
            Some(d) if d.is_dict()? => d.copy_dict()?,
            _ => doc.new_dict()?,
        })
    };
    let mut res = copy(page_obj.get_dict_inheritable("Resources")?)?;
    let dict = copy(res.get_dict(kind)?)?;
    res.dict_put(kind, dict)?;
    page_obj.dict_put("Resources", res)?;
    Ok(page_obj
        .get_dict("Resources")?
        .and_then(|r| r.get_dict(kind).ok().flatten())
        .expect("acabou de ser posto"))
}

pub(crate) fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = a.clone();
    m.concat(b.clone());
    m
}

/// Conteúdo da página (os fluxos de /Contents, um depois do outro).
pub(crate) fn page_content(page_obj: &PdfObject) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let Some(contents) = page_obj.get_dict("Contents")? else {
        return Ok(out);
    };
    if contents.is_array()? {
        for i in 0..contents.len()? as i32 {
            if let Some(s) = contents.get_array(i)? {
                out.extend(s.read_stream()?);
                out.push(b'\n');
            }
        }
    } else {
        out.extend(contents.read_stream()?);
    }
    Ok(out)
}

pub(crate) enum Tok<'a> {
    Num(f32),
    /// Nome ainda com as sequências `#xx`.
    Name(&'a [u8]),
    Op(&'a [u8]),
    Other,
}

pub(crate) fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0)
}

pub(crate) fn is_delim(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Tokenizador do conteúdo: pula comentários, strings e dados de imagem embutida, para que um
/// "Do" dentro deles não conte.
pub(crate) struct Lexer<'a> {
    pub(crate) s: &'a [u8],
    /// Posição depois do último token lido.
    pub(crate) i: usize,
    /// Achou imagem embutida (BI…ID…EI).
    pub(crate) inline_image: bool,
}

impl<'a> Lexer<'a> {
    fn word(&mut self) -> &'a [u8] {
        let s = self.s;
        let start = self.i;
        while self.i < s.len() && !is_ws(s[self.i]) && !is_delim(s[self.i]) {
            self.i += 1;
        }
        &s[start..self.i]
    }

    /// Pula os dados depois de ID até o EI (cercado de espaço) que fecha a imagem embutida.
    fn skip_inline_data(&mut self) {
        self.inline_image = true;
        let s = self.s;
        let mut j = self.i + 1;
        while j + 2 <= s.len() {
            if is_ws(s[j - 1])
                && &s[j..j + 2] == b"EI"
                && s.get(j + 2).is_none_or(|&c| is_ws(c) || is_delim(c))
            {
                self.i = j + 2;
                return;
            }
            j += 1;
        }
        self.i = s.len();
    }

    /// Próximo token e onde começa.
    pub(crate) fn next(&mut self) -> Option<(usize, Tok<'a>)> {
        let s = self.s;
        loop {
            while self.i < s.len() && is_ws(s[self.i]) {
                self.i += 1;
            }
            if s.get(self.i) == Some(&b'%') {
                while self.i < s.len() && !matches!(s[self.i], b'\r' | b'\n') {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
        let start = self.i;
        let c = *s.get(self.i)?;
        let tok = match c {
            b'(' => {
                let mut depth = 0usize;
                while self.i < s.len() {
                    match s[self.i] {
                        b'\\' => self.i += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                self.i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                Tok::Other
            }
            b'<' if s.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                Tok::Other
            }
            b'<' => {
                while self.i < s.len() && s[self.i] != b'>' {
                    self.i += 1;
                }
                self.i += 1;
                Tok::Other
            }
            b'>' if s.get(self.i + 1) == Some(&b'>') => {
                self.i += 2;
                Tok::Other
            }
            b'/' => {
                self.i += 1;
                Tok::Name(self.word())
            }
            _ if is_delim(c) => {
                self.i += 1;
                Tok::Other
            }
            _ => {
                let w = self.word();
                if w == b"ID" {
                    self.skip_inline_data();
                    Tok::Other
                } else {
                    match std::str::from_utf8(w).ok().and_then(|t| t.parse().ok()) {
                        Some(n) => Tok::Num(n),
                        None => Tok::Op(w),
                    }
                }
            }
        };
        Some((start, tok))
    }
}

/// Nome do PDF sem as sequências `#xx`.
pub(crate) fn decode_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = raw
            .get(i + 1..i + 3)
            .filter(|_| raw[i] == b'#')
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(raw[i]);
                i += 1;
            }
        }
    }
    out
}
