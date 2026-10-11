//! Formatar, Validar e Calcular dos campos de formulário, como o Acrobat: as chamadas AF que ele
//! grava em /AA (`AFNumber_Format`, `AFRange_Validate`, `AFSimple_Calculate`, o bloco BVCALC da
//! notação simplificada) são reconhecidas e executadas aqui, sem interpretador JavaScript. Script
//! que não seja uma dessas chamadas é `Custom` (não executa).

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};

/// Maior script reconhecido; casas decimais; símbolos de uma expressão de cálculo; tamanho de
/// máscara e de formato de data.
const MAX_SCRIPT: usize = 64 * 1024;
const MAX_DEC: u8 = 10;
const MAX_TOKENS: usize = 256;
const MAX_MASK: usize = 64;

/// Formatar (aba Formatar das propriedades).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FieldFormat {
    #[default]
    None,
    /// `sep`: 0 `1,234.56`, 1 `1234.56`, 2 `1.234,56`, 3 `1234,56`, 4 `1'234.56`.
    /// `neg`: 0 sinal, 1 vermelho, 2 parênteses, 3 parênteses e vermelho.
    Number {
        dec: u8,
        sep: u8,
        neg: u8,
        currency: String,
        prepend: bool,
    },
    Percent {
        dec: u8,
        sep: u8,
    },
    /// Máscara de data do Acrobat (`dd/mm/yyyy`, `d mmmm yyyy`…).
    Date(String),
    /// 0 `HH:MM`, 1 `h:MM tt`, 2 `HH:MM:ss`, 3 `h:MM:ss tt`.
    Time(u8),
    /// Máscara: `9` dígito, `A` letra, `O` letra ou dígito, `X` qualquer; o resto é literal.
    Special(String),
    /// Script que não é uma chamada AF conhecida (não executa).
    Custom,
}

/// Valor formatado para mostrar.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Formatted {
    pub text: String,
    /// Negativo em vermelho (estilos 1 e 3).
    pub red: bool,
}

/// Validar (aba Validar).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FieldValidate {
    #[default]
    None,
    Range {
        min: Option<f64>,
        max: Option<f64>,
    },
    Custom,
}

/// Operação do Calcular simples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcOp {
    Sum,
    Avg,
    Prd,
    Min,
    Max,
}

impl CalcOp {
    pub const ALL: [Self; 5] = [Self::Sum, Self::Prd, Self::Avg, Self::Min, Self::Max];

    fn code(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Prd => "PRD",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }

    /// Como o Acrobat mostra: "soma (+)", "produto (x)"…
    pub fn label(self) -> &'static str {
        match self {
            Self::Sum => "soma (+)",
            Self::Prd => "produto (x)",
            Self::Avg => "média",
            Self::Min => "mínimo",
            Self::Max => "máximo",
        }
    }
}

/// Calcular (aba Calcular).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FieldCalc {
    #[default]
    None,
    Simple {
        op: CalcOp,
        fields: Vec<String>,
    },
    /// Notação simplificada de campos (`Preco * Quantidade`).
    Sfn(String),
    Custom,
}

/// Máscaras de AFSpecial_Format (0 a 3) e formatos de AFDate_Format (antigo, por número).
const SPECIAL_MASKS: [&str; 4] = ["99999", "99999-9999", "(999) 999-9999", "999-99-9999"];
const OLD_DATES: [&str; 14] = [
    "m/d",
    "m/d/yy",
    "mm/dd/yy",
    "mm/yy",
    "d-mmm",
    "d-mmm-yy",
    "dd-mmm-yy",
    "yy-mm-dd",
    "mmm-yy",
    "mmmm-yy",
    "mmm d, yyyy",
    "mmmm d, yyyy",
    "m/d/yy h:MM tt",
    "m/d/yy HH:MM",
];
const MONTHS: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];
const MONTHS_EN: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const WEEKDAYS: [&str; 7] = [
    "segunda-feira",
    "terça-feira",
    "quarta-feira",
    "quinta-feira",
    "sexta-feira",
    "sábado",
    "domingo",
];

/// Argumento de uma chamada AF.
#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Num(f64),
    Str(String),
    Bool(bool),
    Array(Vec<String>),
}

/// Lê `Nome(arg, …);` (uma chamada só, nada depois). `None` se não for isso.
fn parse_call(js: &str) -> Option<(String, Vec<Arg>)> {
    if js.len() > MAX_SCRIPT {
        return None;
    }
    let mut p = Cursor::new(js.trim());
    let name = p.ident()?;
    p.ws();
    p.eat('(')?;
    let mut args = Vec::new();
    p.ws();
    if p.peek() != Some(')') {
        loop {
            args.push(p.arg()?);
            p.ws();
            match p.next()? {
                ',' => continue,
                ')' => break,
                _ => return None,
            }
        }
    } else {
        p.next();
    }
    p.ws();
    if p.peek() == Some(';') {
        p.next();
    }
    p.ws();
    p.done().then_some((name, args))
}

/// Leitor de texto caractere a caractere.
struct Cursor<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            chars: s.chars().peekable(),
        }
    }
    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }
    fn next(&mut self) -> Option<char> {
        self.chars.next()
    }
    fn done(&mut self) -> bool {
        self.peek().is_none()
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.next();
        }
    }
    fn eat(&mut self, c: char) -> Option<()> {
        (self.next()? == c).then_some(())
    }
    fn ident(&mut self) -> Option<String> {
        let mut out = String::new();
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            out.push(c);
            self.next();
        }
        (!out.is_empty()).then_some(out)
    }
    fn string(&mut self) -> Option<String> {
        let quote = self.next()?;
        let mut out = String::new();
        loop {
            match self.next()? {
                '\\' => out.push(self.next()?),
                c if c == quote => return Some(out),
                c => out.push(c),
            }
            if out.len() > MAX_SCRIPT {
                return None;
            }
        }
    }
    fn arg(&mut self) -> Option<Arg> {
        self.ws();
        match self.peek()? {
            '"' | '\'' => self.string().map(Arg::Str),
            c if c.is_ascii_digit() || c == '-' || c == '.' => {
                let mut s = String::new();
                while let Some(c) = self
                    .peek()
                    .filter(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | 'e' | 'E' | '+'))
                {
                    s.push(c);
                    self.next();
                }
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Arg::Num)
            }
            _ => {
                let word = self.ident()?;
                match word.as_str() {
                    "true" => Some(Arg::Bool(true)),
                    "false" => Some(Arg::Bool(false)),
                    "new" => {
                        self.ws();
                        if self.ident()? != "Array" {
                            return None;
                        }
                        self.ws();
                        self.eat('(')?;
                        let mut items = Vec::new();
                        loop {
                            self.ws();
                            match self.peek()? {
                                ')' => {
                                    self.next();
                                    return Some(Arg::Array(items));
                                }
                                ',' => {
                                    self.next();
                                }
                                '"' | '\'' => items.push(self.string()?),
                                _ => return None,
                            }
                        }
                    }
                    _ => None,
                }
            }
        }
    }
}

fn small(v: f64, max: f64) -> Option<u8> {
    (v >= 0.0 && v <= max && v.fract() == 0.0).then_some(v as u8)
}

/// Reconhece o Formatar (ou o Tecla, que tem os mesmos argumentos).
pub fn parse_format(js: &str) -> FieldFormat {
    if js.trim().is_empty() {
        return FieldFormat::None;
    }
    let Some((name, args)) = parse_call(js) else {
        return FieldFormat::Custom;
    };
    use Arg::*;
    let dec = |v: f64| small(v, 255.0).map(|d| d.min(MAX_DEC));
    let parsed = match (name.as_str(), args.as_slice()) {
        (
            "AFNumber_Format" | "AFNumber_Keystroke",
            [Num(d), Num(s), Num(n), Num(_), Str(c), Bool(pre)],
        ) => Some(FieldFormat::Number {
            dec: match dec(*d) {
                Some(d) => d,
                None => return FieldFormat::Custom,
            },
            sep: small(*s, 4.0).unwrap_or(0),
            neg: small(*n, 3.0).unwrap_or(0),
            currency: c.chars().take(16).collect(),
            prepend: *pre,
        }),
        ("AFPercent_Format" | "AFPercent_Keystroke", [Num(d), Num(s), ..]) => {
            dec(*d).map(|dec| FieldFormat::Percent {
                dec,
                sep: small(*s, 4.0).unwrap_or(0),
            })
        }
        ("AFDate_FormatEx" | "AFDate_KeystrokeEx", [Str(f)]) if f.len() <= MAX_MASK => {
            Some(FieldFormat::Date(f.clone()))
        }
        ("AFDate_Format" | "AFDate_Keystroke", [Num(n)]) => {
            small(*n, 13.0).map(|n| FieldFormat::Date(OLD_DATES[n as usize].to_string()))
        }
        ("AFTime_Format" | "AFTime_Keystroke", [Num(n)]) => small(*n, 3.0).map(FieldFormat::Time),
        ("AFSpecial_Format" | "AFSpecial_Keystroke", [Num(n)]) => {
            small(*n, 3.0).map(|n| FieldFormat::Special(SPECIAL_MASKS[n as usize].to_string()))
        }
        ("AFSpecial_KeystrokeEx", [Str(m)]) if m.chars().count() <= MAX_MASK => {
            Some(FieldFormat::Special(m.clone()))
        }
        _ => None,
    };
    parsed.unwrap_or(FieldFormat::Custom)
}

/// Texto de string JavaScript entre aspas.
fn js_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Os scripts de Formatar (/F) e Tecla (/K) que o Acrobat grava; vazios para Nenhum.
pub fn format_scripts(fmt: &FieldFormat) -> (String, String) {
    let both = |f: String, k: String| (f, k);
    match fmt {
        FieldFormat::None | FieldFormat::Custom => (String::new(), String::new()),
        FieldFormat::Number {
            dec,
            sep,
            neg,
            currency,
            prepend,
        } => {
            let args = format!("{dec}, {sep}, {neg}, 0, {}, {prepend}", js_str(currency));
            both(
                format!("AFNumber_Format({args});"),
                format!("AFNumber_Keystroke({args});"),
            )
        }
        FieldFormat::Percent { dec, sep } => both(
            format!("AFPercent_Format({dec}, {sep});"),
            format!("AFPercent_Keystroke({dec}, {sep});"),
        ),
        FieldFormat::Date(f) => both(
            format!("AFDate_FormatEx({});", js_str(f)),
            format!("AFDate_KeystrokeEx({});", js_str(f)),
        ),
        FieldFormat::Time(n) => both(
            format!("AFTime_Format({n});"),
            format!("AFTime_Keystroke({n});"),
        ),
        FieldFormat::Special(m) => match SPECIAL_MASKS.iter().position(|s| s == m) {
            Some(n) => both(
                format!("AFSpecial_Format({n});"),
                format!("AFSpecial_Keystroke({n});"),
            ),
            None => {
                let js = format!("AFSpecial_KeystrokeEx({});", js_str(m));
                both(js.clone(), js)
            }
        },
    }
}

/// Caractere de milhar e de decimal de cada estilo.
fn separators(sep: u8) -> (Option<char>, char) {
    match sep {
        1 => (None, '.'),
        2 => (Some('.'), ','),
        3 => (None, ','),
        4 => (Some('\''), '.'),
        _ => (Some(','), '.'),
    }
}

/// Número digitado no estilo `sep` (ou já no formato cru, `1234.5`).
fn parse_number(raw: &str, sep: u8) -> Option<f64> {
    let raw = raw.trim();
    if let Ok(v) = raw.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let negative = raw.contains('-') || raw.contains('(');
    let (_, decimal) = separators(sep);
    let mut s = String::new();
    for c in raw.chars() {
        if c.is_ascii_digit() {
            s.push(c);
        } else if c == decimal || (decimal == '.' && c == ',' && sep == 1) {
            s.push('.');
        }
    }
    if s.is_empty() || s.matches('.').count() > 1 {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    Some(if negative { -v } else { v })
}

/// `v` com `dec` casas no estilo `sep` (sem sinal).
fn number_text(v: f64, dec: u8, sep: u8) -> String {
    let (thousands, decimal) = separators(sep);
    let fixed = format!("{:.*}", dec as usize, v.abs());
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut grouped = String::new();
    for (k, c) in int.chars().enumerate() {
        if k > 0 && (int.len() - k) % 3 == 0 {
            if let Some(t) = thousands {
                grouped.push(t);
            }
        }
        grouped.push(c);
    }
    if frac.is_empty() {
        grouped
    } else {
        format!("{grouped}{decimal}{frac}")
    }
}

/// O valor como o Formatar o mostra. Erro: o valor não se encaixa (data inválida, máscara
/// incompleta).
pub fn format_value(fmt: &FieldFormat, raw: &str) -> Result<Formatted, String> {
    let plain = |text: String| Formatted { text, red: false };
    if raw.trim().is_empty() {
        return Ok(Formatted::default());
    }
    match fmt {
        FieldFormat::None | FieldFormat::Custom => Ok(plain(raw.to_string())),
        FieldFormat::Number {
            dec,
            sep,
            neg,
            currency,
            prepend,
        } => {
            let v = parse_number(raw, *sep).ok_or_else(mismatch)?;
            let body = number_text(v, *dec, *sep);
            let body = if *prepend {
                format!("{currency}{body}")
            } else {
                format!("{body}{currency}")
            };
            let negative = v < 0.0 && body.chars().any(|c| c.is_ascii_digit() && c != '0');
            Ok(match (negative, neg) {
                (false, _) => plain(body),
                (true, 1) => Formatted {
                    text: body,
                    red: true,
                },
                (true, 2) => plain(format!("({body})")),
                (true, 3) => Formatted {
                    text: format!("({body})"),
                    red: true,
                },
                (true, _) => plain(format!("-{body}")),
            })
        }
        FieldFormat::Percent { dec, sep } => {
            let v = parse_number(raw.trim_end_matches('%'), *sep).ok_or_else(mismatch)? * 100.0;
            let sign = if v < 0.0 { "-" } else { "" };
            Ok(plain(format!("{sign}{}%", number_text(v, *dec, *sep))))
        }
        FieldFormat::Date(f) => {
            let (date, time) = parse_date(raw, f).ok_or_else(mismatch)?;
            Ok(plain(print_date(f, date, time)))
        }
        FieldFormat::Time(n) => {
            let t = parse_time(raw).ok_or_else(mismatch)?;
            let f = ["HH:MM", "h:MM tt", "HH:MM:ss", "h:MM:ss tt"][(*n).min(3) as usize];
            Ok(plain(print_date(f, NaiveDate::default(), t)))
        }
        FieldFormat::Special(mask) => apply_mask(mask, raw, false).map(plain),
    }
}

/// O texto digitado pronto para gravar em /V (número cru; data, hora e máscara no formato).
pub fn commit_value(fmt: &FieldFormat, typed: &str) -> Result<String, String> {
    if typed.trim().is_empty() {
        return Ok(String::new());
    }
    match fmt {
        FieldFormat::Number { sep, .. } | FieldFormat::Percent { sep, .. } => {
            let v = parse_number(typed.trim_end_matches('%'), *sep).ok_or_else(mismatch)?;
            Ok(format!("{v}"))
        }
        FieldFormat::None | FieldFormat::Custom => Ok(typed.to_string()),
        _ => format_value(fmt, typed).map(|f| f.text),
    }
}

/// O aviso do Acrobat quando o valor não cabe no formato.
fn mismatch() -> String {
    "O valor inserido não corresponde ao formato do campo.".into()
}

/// Encaixa `input` na máscara. `partial`: aceita faltar o fim (o que está sendo digitado).
fn apply_mask(mask: &str, input: &str, partial: bool) -> Result<String, String> {
    let slot_ok = |m: char, c: char| match m {
        '9' => c.is_ascii_digit(),
        'A' => c.is_alphabetic(),
        'O' => c.is_alphanumeric(),
        'X' => !c.is_control(),
        _ => false,
    };
    let is_slot = |m: char| matches!(m, '9' | 'A' | 'O' | 'X');
    let literals: Vec<char> = mask.chars().filter(|m| !is_slot(*m)).collect();
    let mut chars = input.trim().chars().peekable();
    let mut out = String::new();
    for m in mask.chars() {
        if is_slot(m) {
            // literais da máscara digitados pelo usuário são pulados
            while chars
                .peek()
                .is_some_and(|c| !slot_ok(m, *c) && literals.contains(c))
            {
                chars.next();
            }
            match chars.next() {
                Some(c) if slot_ok(m, c) => out.push(c),
                Some(_) => return Err(mismatch()),
                None if partial => return Ok(out),
                None => return Err(mismatch()),
            }
        } else {
            if chars.peek().is_none() && partial {
                return Ok(out);
            }
            out.push(m);
            if chars.peek() == Some(&m) {
                chars.next();
            }
        }
    }
    if chars.next().is_some() {
        return Err(mismatch());
    }
    Ok(out)
}

/// O texto em digitação ainda pode virar um valor do formato?
pub fn keystroke_ok(fmt: &FieldFormat, text: &str) -> bool {
    match fmt {
        FieldFormat::None | FieldFormat::Custom => true,
        FieldFormat::Number { currency, .. } => text
            .chars()
            .all(|c| c.is_ascii_digit() || " .,-()'".contains(c) || currency.contains(c)),
        FieldFormat::Percent { .. } => text
            .chars()
            .all(|c| c.is_ascii_digit() || " .,-%'".contains(c)),
        FieldFormat::Date(_) => text
            .chars()
            .all(|c| c.is_alphanumeric() || " /-.,:".contains(c)),
        FieldFormat::Time(_) => text
            .chars()
            .all(|c| c.is_ascii_digit() || " :aApPmM".contains(c)),
        FieldFormat::Special(mask) => apply_mask(mask, text, true).is_ok(),
    }
}

/// Mês por nome (português ou inglês, inteiro ou começo de 3+ letras).
fn month_named(word: &str) -> Option<u32> {
    let w = word.to_lowercase();
    if w.chars().count() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .chain(MONTHS_EN.iter())
        .position(|m| m.starts_with(&w) || w.starts_with(&m.chars().take(3).collect::<String>()))
        .map(|k| (k % 12) as u32 + 1)
}

/// Lê data (e hora) digitada, na ordem dia/mês/ano da máscara. ISO `aaaa-mm-dd` também vale.
fn parse_date(raw: &str, fmt: &str) -> Option<(NaiveDate, NaiveTime)> {
    let raw = raw.trim();
    let mut nums: Vec<u32> = Vec::new();
    let mut month_word = None;
    let mut word = String::new();
    let mut digits = String::new();
    let flush = |digits: &mut String, nums: &mut Vec<u32>| -> Option<()> {
        if !digits.is_empty() {
            nums.push(digits.parse().ok()?);
            digits.clear();
        }
        Some(())
    };
    for c in raw.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush(&mut digits, &mut nums)?;
        }
        if c.is_alphabetic() {
            word.push(c);
        } else if !word.is_empty() {
            month_word = month_word.or(month_named(&word));
            word.clear();
        }
    }
    let pm = raw.to_lowercase().contains("pm");
    // ordem de dia, mês e ano na máscara
    let lower = fmt.replace("MM", "").replace("ss", "");
    let pos = |t: char| lower.find(t);
    let iso = raw.len() >= 8
        && raw.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && raw.as_bytes()[4] == b'-';
    let mut order: Vec<char> = if iso {
        vec!['y', 'm', 'd']
    } else {
        let mut o: Vec<(usize, char)> = ['d', 'm', 'y']
            .into_iter()
            .filter_map(|t| pos(t).map(|p| (p, t)))
            .collect();
        o.sort();
        o.into_iter().map(|(_, t)| t).collect()
    };
    if month_word.is_some() {
        order.retain(|t| *t != 'm');
    }
    let mut it = nums.into_iter();
    let (mut d, mut m, mut y) = (1, month_word.unwrap_or(1), chrono::Local::now().year());
    for t in order {
        let v = it.next()?;
        match t {
            'd' => d = v,
            'm' => m = v,
            _ => {
                y = match v {
                    0..=49 => 2000 + v as i32,
                    50..=99 => 1900 + v as i32,
                    _ => v as i32,
                }
            }
        }
    }
    if !(1..=9999).contains(&y) {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(y, m, d)?;
    let (mut h, min, s) = (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    );
    if pm && h < 12 {
        h += 12;
    }
    Some((date, NaiveTime::from_hms_opt(h, min, s)?))
}

/// Lê hora digitada (`14:05`, `2:05 pm`, `9:5`).
fn parse_time(raw: &str) -> Option<NaiveTime> {
    let lower = raw.to_lowercase();
    let nums: Vec<u32> = lower
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let (mut h, m, s) = match nums.as_slice() {
        [h] => (*h, 0, 0),
        [h, m] => (*h, *m, 0),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if lower.contains('p') && h < 12 {
        h += 12;
    } else if lower.contains('a') && h == 12 {
        h = 0;
    }
    NaiveTime::from_hms_opt(h, m, s)
}

/// Escreve a data com a máscara do Acrobat (`yyyy`, `mmmm`, `dd`, `HH`, `MM`, `tt`…).
fn print_date(fmt: &str, d: NaiveDate, t: NaiveTime) -> String {
    const TOKENS: [&str; 20] = [
        "yyyy", "mmmm", "dddd", "mmm", "ddd", "yy", "mm", "dd", "HH", "hh", "MM", "ss", "tt", "m",
        "d", "H", "h", "M", "s", "t",
    ];
    let h12 = match t.hour() % 12 {
        0 => 12,
        h => h,
    };
    let mut out = String::new();
    let mut rest = fmt;
    while !rest.is_empty() {
        let Some(tok) = TOKENS.iter().find(|t| rest.starts_with(**t)) else {
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
            continue;
        };
        let month = MONTHS[d.month0() as usize];
        let weekday = WEEKDAYS[d.weekday().num_days_from_monday() as usize];
        out.push_str(&match *tok {
            "yyyy" => format!("{:04}", d.year()),
            "yy" => format!("{:02}", d.year() % 100),
            "mmmm" => month.to_string(),
            "mmm" => month.chars().take(3).collect(),
            "mm" => format!("{:02}", d.month()),
            "m" => d.month().to_string(),
            "dddd" => weekday.to_string(),
            "ddd" => weekday.chars().take(3).collect(),
            "dd" => format!("{:02}", d.day()),
            "d" => d.day().to_string(),
            "HH" => format!("{:02}", t.hour()),
            "H" => t.hour().to_string(),
            "hh" => format!("{h12:02}"),
            "h" => h12.to_string(),
            "MM" => format!("{:02}", t.minute()),
            "M" => t.minute().to_string(),
            "ss" => format!("{:02}", t.second()),
            "s" => t.second().to_string(),
            "tt" => if t.hour() < 12 { "am" } else { "pm" }.to_string(),
            _ => if t.hour() < 12 { "a" } else { "p" }.to_string(),
        });
        rest = &rest[tok.len()..];
    }
    out
}

/// Reconhece o Validar.
pub fn parse_validate(js: &str) -> FieldValidate {
    if js.trim().is_empty() {
        return FieldValidate::None;
    }
    match parse_call(js) {
        Some((name, args)) if name == "AFRange_Validate" => match args.as_slice() {
            [Arg::Bool(bmin), Arg::Num(min), Arg::Bool(bmax), Arg::Num(max)] => {
                FieldValidate::Range {
                    min: bmin.then_some(*min),
                    max: bmax.then_some(*max),
                }
            }
            _ => FieldValidate::Custom,
        },
        _ => FieldValidate::Custom,
    }
}

/// O script de Validar que o Acrobat grava; vazio para Nenhum.
pub fn validate_script(v: &FieldValidate) -> String {
    match v {
        FieldValidate::Range { min, max } => format!(
            "AFRange_Validate({}, {}, {}, {});",
            min.is_some(),
            min.unwrap_or(0.0),
            max.is_some(),
            max.unwrap_or(0.0)
        ),
        _ => String::new(),
    }
}

/// Confere o valor (cru) no Validar; o erro é o aviso do Acrobat.
pub fn validate(v: &FieldValidate, raw: &str) -> Result<(), String> {
    let FieldValidate::Range { min, max } = v else {
        return Ok(());
    };
    let Some(x) = parse_number(raw, 0).or_else(|| parse_number(raw, 2)) else {
        return Ok(()); // vazio ou não numérico: o Acrobat não valida
    };
    if min.is_some_and(|m| x < m) || max.is_some_and(|m| x > m) {
        let mut msg = String::from("O valor inserido deve ser");
        if let Some(m) = min {
            msg.push_str(&format!(" maior ou igual a {m}"));
        }
        if let (Some(_), Some(_)) = (min, max) {
            msg.push_str(" e");
        }
        if let Some(m) = max {
            msg.push_str(&format!(" menor ou igual a {m}"));
        }
        msg.push('.');
        return Err(msg);
    }
    Ok(())
}

/// Símbolo de uma expressão da notação simplificada.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Field(String),
    Op(char),
}

/// Quebra a expressão em símbolos. Nomes aceitam letras, dígitos, `_`, `.` e qualquer caractere
/// escapado com `\` (espaço, por exemplo).
fn tokens(expr: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        if out.len() > MAX_TOKENS {
            return None;
        }
        if c.is_whitespace() {
            chars.next();
        } else if "+-*/()".contains(c) {
            out.push(Tok::Op(c));
            chars.next();
        } else if c.is_ascii_digit() || c == '.' {
            let mut s = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit() || **d == '.') {
                s.push(d);
                chars.next();
            }
            out.push(Tok::Num(s.parse().ok()?));
        } else if c.is_alphanumeric() || c == '_' || c == '\\' {
            let mut s = String::new();
            while let Some(&d) = chars.peek() {
                if d == '\\' {
                    chars.next();
                    s.push(chars.next()?);
                } else if d.is_alphanumeric() || d == '_' || d == '.' {
                    s.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(Tok::Field(s));
        } else {
            return None;
        }
    }
    Some(out)
}

/// Avalia a expressão (precedência normal, menos unário). `None`: malformada ou divisão por zero.
struct Expr<'a, F: Fn(&str) -> f64> {
    toks: &'a [Tok],
    at: usize,
    lookup: &'a F,
}

impl<F: Fn(&str) -> f64> Expr<'_, F> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            let r = self.product()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }
    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.at += 1;
            let r = self.unary()?;
            v = if op == '*' {
                v * r
            } else if r == 0.0 {
                return None;
            } else {
                v / r
            };
        }
        Some(v)
    }
    fn unary(&mut self) -> Option<f64> {
        let tok = self.peek()?.clone();
        self.at += 1;
        match tok {
            Tok::Op('-') => Some(-self.unary()?),
            Tok::Op('+') => self.unary(),
            Tok::Op('(') => {
                let v = self.sum()?;
                (self.peek()? == &Tok::Op(')')).then(|| self.at += 1)?;
                Some(v)
            }
            Tok::Num(v) => Some(v),
            Tok::Field(name) => Some((self.lookup)(&name)),
            Tok::Op(_) => None,
        }
    }
}

/// Reconhece o Calcular.
pub fn parse_calc(js: &str) -> FieldCalc {
    let js_t = js.trim();
    if js_t.is_empty() {
        return FieldCalc::None;
    }
    if js_t.len() > MAX_SCRIPT {
        return FieldCalc::Custom;
    }
    if let (Some(a), Some(b)) = (js_t.find("BVCALC"), js_t.find("EVCALC")) {
        let expr = js_t.get(a + 6..b).unwrap_or("").trim();
        let ok = tokens(expr).is_some_and(|t| {
            let mut e = Expr {
                toks: &t,
                at: 0,
                lookup: &|_: &str| 1.0,
            };
            !t.is_empty() && e.sum().is_some() && e.at == t.len()
        });
        // só vale se o código depois do comentário é a tradução que o Acrobat grava para a
        // expressão: senão o comentário poderia dizer uma conta e o código fazer outra
        let squash = |s: &str| -> String {
            s.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .trim_end_matches(';')
                .to_string()
        };
        let sfn = FieldCalc::Sfn(expr.to_string());
        return if ok && squash(js_t) == squash(&calc_script(&sfn)) {
            sfn
        } else {
            FieldCalc::Custom
        };
    }
    let Some((name, args)) = parse_call(js_t) else {
        return FieldCalc::Custom;
    };
    let op = |s: &str| CalcOp::ALL.into_iter().find(|o| o.code() == s);
    match (name.as_str(), args.as_slice()) {
        ("AFSimple_Calculate", [Arg::Str(o), list]) => {
            let fields: Vec<String> = match list {
                Arg::Array(items) => items.clone(),
                Arg::Str(s) => s.split(',').map(|f| f.trim().to_string()).collect(),
                _ => return FieldCalc::Custom,
            };
            match op(o) {
                Some(op) if !fields.is_empty() && fields.len() <= MAX_TOKENS => {
                    FieldCalc::Simple { op, fields }
                }
                _ => FieldCalc::Custom,
            }
        }
        _ => FieldCalc::Custom,
    }
}

/// O script de Calcular que o Acrobat grava; vazio para Nenhum.
pub fn calc_script(c: &FieldCalc) -> String {
    match c {
        FieldCalc::Simple { op, fields } => {
            let list: Vec<String> = fields.iter().map(|f| js_str(f)).collect();
            format!(
                "AFSimple_Calculate(\"{}\", new Array ({}));",
                op.code(),
                list.join(", ")
            )
        }
        FieldCalc::Sfn(expr) => {
            let js: String = tokens(expr)
                .unwrap_or_default()
                .into_iter()
                .map(|t| match t {
                    Tok::Num(v) => format!("{v}"),
                    Tok::Op(c) => c.to_string(),
                    Tok::Field(f) => format!("AFMakeNumber(getField({}).value)", js_str(&f)),
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("/** BVCALC {expr} EVCALC **/ event.value = {js};")
        }
        _ => String::new(),
    }
}

/// Os campos de que o cálculo depende.
pub fn calc_fields(c: &FieldCalc) -> Vec<String> {
    match c {
        FieldCalc::Simple { fields, .. } => fields.clone(),
        FieldCalc::Sfn(expr) => tokens(expr)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| match t {
                Tok::Field(f) => Some(f),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Calcula com os valores dos campos (`lookup`: o número de cada campo, 0 se vazio).
pub fn evaluate(c: &FieldCalc, lookup: &impl Fn(&str) -> f64) -> Option<f64> {
    let v = match c {
        FieldCalc::Simple { op, fields } => {
            let vals: Vec<f64> = fields.iter().map(|f| lookup(f)).collect();
            if vals.is_empty() {
                return None;
            }
            match op {
                CalcOp::Sum => vals.iter().sum(),
                CalcOp::Avg => vals.iter().sum::<f64>() / vals.len() as f64,
                CalcOp::Prd => vals.iter().product(),
                CalcOp::Min => vals.iter().copied().fold(f64::INFINITY, f64::min),
                CalcOp::Max => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            }
        }
        FieldCalc::Sfn(expr) => {
            let toks = tokens(expr)?;
            let mut e = Expr {
                toks: &toks,
                at: 0,
                lookup,
            };
            let v = e.sum()?;
            if e.at != toks.len() {
                return None;
            }
            v
        }
        _ => return None,
    };
    v.is_finite().then_some(v)
}

/// O número de um valor cru (para Calcular): vazio ou não numérico vale 0.
pub fn number_of(raw: &str) -> f64 {
    parse_number(raw, 0)
        .or_else(|| parse_number(raw, 2))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(dec: u8, sep: u8, neg: u8, currency: &str, prepend: bool) -> FieldFormat {
        FieldFormat::Number {
            dec,
            sep,
            neg,
            currency: currency.into(),
            prepend,
        }
    }

    #[test]
    fn number_styles_and_negatives() {
        let f = |fmt: &FieldFormat, raw: &str| format_value(fmt, raw).unwrap().text;
        assert_eq!(f(&num(2, 0, 0, "", true), "1234.5"), "1,234.50");
        assert_eq!(f(&num(2, 1, 0, "", true), "1234.5"), "1234.50");
        assert_eq!(f(&num(2, 2, 0, "", true), "1234.5"), "1.234,50");
        assert_eq!(f(&num(2, 3, 0, "", true), "1234.5"), "1234,50");
        assert_eq!(f(&num(0, 2, 0, "", true), "1234567.6"), "1.234.568");
        assert_eq!(f(&num(2, 2, 0, "", true), "-1234.5"), "-1.234,50");
        assert_eq!(f(&num(2, 2, 2, "", true), "-1234.5"), "(1.234,50)");
        let red = format_value(&num(2, 2, 1, "", true), "-3").unwrap();
        assert_eq!((red.text.as_str(), red.red), ("3,00", true));
        assert_eq!(f(&num(2, 2, 0, "", true), ""), "", "vazio continua vazio");
        assert_eq!(
            f(&num(2, 2, 0, "", true), "1.234,5"),
            "1.234,50",
            "aceita o que foi digitado"
        );
    }

    #[test]
    fn percent_and_currency() {
        let f = |fmt: &FieldFormat, raw: &str| format_value(fmt, raw).unwrap().text;
        assert_eq!(f(&num(2, 2, 0, "R$ ", true), "1234.5"), "R$ 1.234,50");
        assert_eq!(f(&num(2, 2, 0, "R$ ", true), "-5"), "-R$ 5,00");
        assert_eq!(f(&num(2, 0, 0, " €", false), "7"), "7.00 €");
        let pct = FieldFormat::Percent { dec: 1, sep: 2 };
        assert_eq!(f(&pct, "0.155"), "15,5%");
    }

    #[test]
    fn dates_and_times() {
        let d = FieldFormat::Date("dd/mm/yyyy".into());
        assert_eq!(format_value(&d, "5/3/2026").unwrap().text, "05/03/2026");
        assert_eq!(format_value(&d, "2026-03-05").unwrap().text, "05/03/2026");
        assert!(format_value(&d, "31/02/2026").is_err(), "não existe");
        let long = FieldFormat::Date("d mmmm yyyy".into());
        assert_eq!(
            format_value(&long, "05/03/2026").unwrap().text,
            "5 março 2026"
        );
        let t = FieldFormat::Time(1);
        assert_eq!(format_value(&t, "14:05").unwrap().text, "2:05 pm");
        assert_eq!(
            format_value(&FieldFormat::Time(0), "9:5").unwrap().text,
            "09:05"
        );
        assert!(format_value(&FieldFormat::Time(0), "25:00").is_err());
    }

    #[test]
    fn special_masks() {
        let cep = FieldFormat::Special("99999-999".into());
        assert_eq!(format_value(&cep, "01310100").unwrap().text, "01310-100");
        assert_eq!(format_value(&cep, "01310-100").unwrap().text, "01310-100");
        assert!(format_value(&cep, "0131").is_err(), "incompleto");
        let cpf = FieldFormat::Special("999.999.999-99".into());
        assert_eq!(
            format_value(&cpf, "12345678901").unwrap().text,
            "123.456.789-01"
        );
        let placa = FieldFormat::Special("AAA-9O99".into());
        assert_eq!(format_value(&placa, "abc1D23").unwrap().text, "abc-1D23");
    }

    #[test]
    fn keystrokes() {
        let n = num(2, 2, 0, "R$ ", true);
        assert!(keystroke_ok(&n, "-1.234,5"));
        assert!(!keystroke_ok(&n, "12a"));
        assert!(keystroke_ok(&FieldFormat::Percent { dec: 0, sep: 0 }, "15"));
        let cep = FieldFormat::Special("99999-999".into());
        assert!(keystroke_ok(&cep, "01310-1"));
        assert!(!keystroke_ok(&cep, "0131a"));
        assert!(!keystroke_ok(&cep, "01310-1000"), "passou do tamanho");
        assert!(keystroke_ok(&FieldFormat::None, "qualquer coisa"));
    }

    #[test]
    fn acrobat_scripts_round_trip() {
        let fmt = num(2, 2, 0, "R$ ", true);
        let (f, k) = format_scripts(&fmt);
        assert_eq!(f, r#"AFNumber_Format(2, 2, 0, 0, "R$ ", true);"#);
        assert_eq!(k, r#"AFNumber_Keystroke(2, 2, 0, 0, "R$ ", true);"#);
        assert_eq!(parse_format(&f), fmt);
        // como o Acrobat grava (espaços, sem ponto e vírgula)
        assert_eq!(
            parse_format("AFPercent_Format(1,0)"),
            FieldFormat::Percent { dec: 1, sep: 0 }
        );
        assert_eq!(
            parse_format(r#"AFDate_FormatEx("dd/mm/yyyy");"#),
            FieldFormat::Date("dd/mm/yyyy".into())
        );
        assert_eq!(parse_format("AFTime_Format(1);"), FieldFormat::Time(1));
        assert_eq!(
            parse_format("AFSpecial_Format(1);"),
            FieldFormat::Special("99999-9999".into())
        );
        assert_eq!(
            parse_format(r#"AFSpecial_KeystrokeEx("99999-999");"#),
            FieldFormat::Special("99999-999".into())
        );
        let v = FieldValidate::Range {
            min: Some(0.0),
            max: Some(100.0),
        };
        assert_eq!(validate_script(&v), "AFRange_Validate(true, 0, true, 100);");
        assert_eq!(parse_validate(&validate_script(&v)), v);
        let c = FieldCalc::Simple {
            op: CalcOp::Sum,
            fields: vec!["Preço".into(), "Frete".into()],
        };
        let js = calc_script(&c);
        assert_eq!(
            js,
            r#"AFSimple_Calculate("SUM", new Array ("Preço", "Frete"));"#
        );
        assert_eq!(parse_calc(&js), c);
        assert_eq!(
            parse_calc(r#"AFSimple_Calculate("AVG", "a, b");"#),
            FieldCalc::Simple {
                op: CalcOp::Avg,
                fields: vec!["a".into(), "b".into()]
            }
        );
        let sfn = FieldCalc::Sfn("Preco * Quantidade".into());
        assert_eq!(parse_calc(&calc_script(&sfn)), sfn);
    }

    #[test]
    fn sfn_expressions() {
        let vals = |name: &str| match name {
            "Preco" => 10.0,
            "Quantidade" => 3.0,
            "Taxa.1" => 0.5,
            "Nome com espaço" => 2.0,
            _ => 0.0,
        };
        let ev = |e: &str| evaluate(&FieldCalc::Sfn(e.into()), &vals);
        assert_eq!(ev("Preco * Quantidade"), Some(30.0));
        assert_eq!(ev("(Preco + 2) * -Quantidade / 4"), Some(-9.0));
        assert_eq!(ev("Preco * Taxa.1"), Some(5.0));
        assert_eq!(ev(r"Nome\ com\ espaço * 3"), Some(6.0));
        assert_eq!(ev("Preco / 0"), None, "divisão por zero fica vazia");
        assert_eq!(ev("Preco * ("), None, "malformada");
        let sum = FieldCalc::Simple {
            op: CalcOp::Max,
            fields: vec!["Preco".into(), "Quantidade".into()],
        };
        assert_eq!(evaluate(&sum, &vals), Some(10.0));
    }

    #[test]
    fn range() {
        let v = FieldValidate::Range {
            min: Some(1.0),
            max: Some(10.0),
        };
        assert!(validate(&v, "5").is_ok());
        assert!(validate(&v, "").is_ok(), "vazio não é validado");
        let e = validate(&v, "11").unwrap_err();
        assert!(
            e.contains("maior ou igual a 1") && e.contains("menor ou igual a 10"),
            "{e}"
        );
        let only_min = FieldValidate::Range {
            min: Some(0.0),
            max: None,
        };
        assert!(validate(&only_min, "-1").is_err());
    }

    #[test]
    fn hostile_scripts_are_custom() {
        for js in [
            "AFNumber_Format(2, 2, 0, 0, \"R$ \", true); app.alert(1);",
            "AFNumber_Format(99999999999999999999, 2, 0, 0, \"x\", true);",
            "AFNumber_Format(2, 2, 0, 0, \"R$ , true);",
            "AFNumber_Format(AFNumber_Format(1));",
            "event.value = 1",
            "",
        ] {
            assert!(
                matches!(parse_format(js), FieldFormat::Custom | FieldFormat::None),
                "{js}"
            );
        }
        let huge = format!("/** BVCALC {} EVCALC **/", "a + ".repeat(5000));
        assert_eq!(parse_calc(&huge), FieldCalc::Custom);
        assert_eq!(
            parse_calc("this.getField('a').value = 1"),
            FieldCalc::Custom
        );
        // o comentário BVCALC diz uma coisa e o código faz outra: não confiar no comentário
        assert_eq!(
            parse_calc("/** BVCALC a + b EVCALC **/ event.value = 999;"),
            FieldCalc::Custom
        );
        let sfn = calc_script(&FieldCalc::Sfn("a + b".into()));
        assert_eq!(
            parse_calc(&format!("{sfn} app.alert(1);")),
            FieldCalc::Custom
        );
        // o mesmo texto com outros espaços continua sendo a notação simplificada
        assert_eq!(
            parse_calc(&sfn.replace(" = ", "=")),
            FieldCalc::Sfn("a + b".into())
        );
    }
}
