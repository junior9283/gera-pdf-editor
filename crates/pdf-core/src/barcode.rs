//! Campo Código de barras (como o do Acrobat): um campo de texto com /PMD (Paper MetaData) que
//! mostra o próprio valor como PDF417, QR Code ou Data Matrix. O valor vem do Calcular (os campos
//! delimitados por tabulação); a aparência, com os módulos em preto, é desenhada aqui (o MuPDF
//! mostraria o texto).
use crate::doc::PdfDoc;
use crate::error::Result;
use crate::page_content::form_xobject;
use crate::stamp::literal;
use mupdf::pdf::{PdfDocument, PdfObject};
use rxing::{BarcodeFormat, EncodeHints, MultiFormatWriter, Writer};

/// Simbologia do código.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Symbology {
    Pdf417,
    #[default]
    QrCode,
    DataMatrix,
}

impl Symbology {
    pub const ALL: [Symbology; 3] = [Self::Pdf417, Self::QrCode, Self::DataMatrix];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf417 => "PDF417",
            Self::QrCode => "QR Code",
            Self::DataMatrix => "Data Matrix",
        }
    }

    fn pdf_name(self) -> &'static str {
        match self {
            Self::Pdf417 => "PDF417",
            Self::QrCode => "QRCode",
            Self::DataMatrix => "DataMatrix",
        }
    }

    fn format(self) -> BarcodeFormat {
        match self {
            Self::Pdf417 => BarcodeFormat::PDF_417,
            Self::QrCode => BarcodeFormat::QR_CODE,
            Self::DataMatrix => BarcodeFormat::DATA_MATRIX,
        }
    }

    /// Zona de silêncio (em módulos) que os leitores exigem em volta do código.
    fn quiet_zone(self) -> f32 {
        match self {
            Self::QrCode => 4.0,
            Self::Pdf417 => 2.0,
            Self::DataMatrix => 1.0,
        }
    }

    /// Níveis de correção de erros (PDF417 0 a 8; QR L, M, Q, H; Data Matrix não tem).
    pub fn ecc_levels(self) -> &'static [&'static str] {
        match self {
            Self::Pdf417 => &["0", "1", "2", "3", "4", "5", "6", "7", "8"],
            Self::QrCode => &["L (7%)", "M (15%)", "Q (25%)", "H (30%)"],
            Self::DataMatrix => &[],
        }
    }
}

/// Opções do código (aba Opções do Acrobat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarcodeOptions {
    pub symbology: Symbology,
    /// Índice em `ecc_levels`.
    pub ecc: u8,
}

impl BarcodeOptions {
    /// A correção de erros dentro dos níveis da simbologia.
    fn clamped(self) -> Self {
        let max = self.symbology.ecc_levels().len().saturating_sub(1) as u8;
        Self {
            ecc: self.ecc.min(max),
            ..self
        }
    }
}

impl Default for BarcodeOptions {
    fn default() -> Self {
        Self {
            symbology: Symbology::QrCode,
            ecc: 1,
        }
    }
}

/// Códigos desenhados por passada (abrir o arquivo, redesenhar páginas): codificar é caro.
pub(crate) const MAX_PER_PASS: usize = 50;
/// Maior valor desenhado (acima disso, o aviso no lugar do código).
const MAX_VALUE: usize = 2_000;
/// No Data Matrix, bem menos: o rxing entra em pânico perto de 1 558 palavras-código (o app
/// aborta com pânico) e codifica números em tempo quadrático.
const MAX_DATA_MATRIX: usize = 500;
/// Chave da aparência desenhada (valor, opções e tamanho): igual, não codifica de novo.
const KEY: &str = "GeraKey";
/// Largura do módulo gravada no /PMD (em milésimos de polegada, como o Acrobat).
const X_SYM_WIDTH: i32 = 9;

/// As opções do /PMD do campo, se ele é um código de barras.
pub(crate) fn read_options(obj: &PdfObject) -> Option<BarcodeOptions> {
    let pmd = obj.get_dict_inheritable("PMD").ok().flatten()?;
    let sym = crate::pdf_obj::get(&pmd, "Symbology").and_then(|s| crate::pdf_obj::name_of(&s));
    // desconhecida: PDF417, o padrão do Acrobat
    let symbology = Symbology::ALL
        .into_iter()
        .find(|s| sym.as_deref() == Some(s.pdf_name()))
        .unwrap_or(Symbology::Pdf417);
    let ecc = crate::pdf_obj::get(&pmd, "ECC")
        .and_then(|e| e.as_int().ok())
        .unwrap_or(1)
        .clamp(0, 255) as u8;
    Some(BarcodeOptions { symbology, ecc }.clamped())
}

/// Grava o /PMD como o Acrobat (/Width e /Height em polegadas; /DataPrep e /Resolution, que o
/// Acrobat exige na prática).
pub(crate) fn write_options(
    doc: &mut PdfDocument,
    head: &mut PdfObject,
    o: BarcodeOptions,
    size: (f32, f32),
) -> Result<()> {
    let o = o.clamped();
    let extra = match o.symbology {
        Symbology::Pdf417 => format!("/XSymHeight {}/ECC {}", X_SYM_WIDTH * 3, o.ecc),
        Symbology::QrCode => format!("/ECC {}", o.ecc),
        Symbology::DataMatrix => String::new(),
    };
    let pmd = doc.new_object_from_str(&format!(
        "<</Type/PaperMetaData/Version 1/Width {:.3}/Height {:.3}/XSymWidth {X_SYM_WIDTH}\
         /Symbology/{}{extra}/DataPrep 0/Resolution 300.0>>",
        size.0 / 72.0,
        size.1 / 72.0,
        o.symbology.pdf_name()
    ))?;
    head.dict_put("PMD", doc.add_object(&pmd)?)?;
    Ok(())
}

/// O aviso no lugar do código quando os dados não cabem.
const TOO_MUCH: &str = "Dados demais para o código de barras";

/// Os módulos do código (linhas de `bool`, `true` = preto), ou o motivo de não caber.
pub fn modules(o: BarcodeOptions, text: &str) -> std::result::Result<Vec<Vec<bool>>, String> {
    let limit = match o.symbology {
        Symbology::DataMatrix => MAX_DATA_MATRIX,
        _ => MAX_VALUE,
    };
    if text.chars().count() > limit {
        return Err(TOO_MUCH.into());
    }
    let o = o.clamped();
    let hints = EncodeHints {
        CharacterSet: Some("UTF-8".into()),
        Margin: Some("0".into()),
        ErrorCorrection: match o.symbology {
            // a letra do nível ("L (7%)" → "L")
            Symbology::QrCode => Some(o.symbology.ecc_levels()[usize::from(o.ecc)][..1].into()),
            Symbology::Pdf417 => Some(o.ecc.to_string()),
            Symbology::DataMatrix => None,
        },
        ..Default::default()
    };
    let format = o.symbology.format();
    // qualquer falha do codificador (dados demais, caractere que não cabe, até pânico) vira o aviso
    let m = std::panic::catch_unwind(|| {
        MultiFormatWriter.encode_with_hints(text, &format, 0, 0, &hints)
    })
    .ok()
    .and_then(|r| r.ok())
    .ok_or_else(|| TOO_MUCH.to_string())?;
    Ok((0..m.height())
        .map(|y| (0..m.width()).map(|x| m.get(x, y)).collect())
        .collect())
}

impl PdfDoc {
    /// Desenha o código de barras do widget a partir do valor (/V): os módulos em preto,
    /// proporcionais e centrados; sem valor, só a borda; dados demais, o aviso. Se a aparência já
    /// é a deste valor (mesmas opções e tamanho), não codifica de novo.
    pub(crate) fn draw_barcode(&mut self, obj: &mut PdfObject, o: BarcodeOptions) -> Result<()> {
        use std::fmt::Write;
        let (w, h) = crate::pdf_obj::rect_size(obj);
        let value = obj
            .get_dict_inheritable("V")?
            .and_then(|v| v.as_string().ok())
            .unwrap_or_default();
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (
                &value,
                o.symbology.pdf_name(),
                o.ecc,
                w.to_bits(),
                h.to_bits(),
            )
                .hash(&mut hasher);
            format!("{:x}", hasher.finish())
        };
        let drawn = crate::pdf_obj::get(obj, "AP")
            .and_then(|ap| crate::pdf_obj::get(&ap, "N"))
            .is_some_and(|n| crate::pdf_obj::text(&n, KEY) == key);
        if drawn {
            return Ok(());
        }
        // sem orçamento nesta passada: fica como está até o valor mudar; sem aparência nenhuma,
        // só a borda (senão o MuPDF montaria o texto do valor a cada carga da página)
        if self.barcode_budget == 0 {
            if crate::pdf_obj::get(obj, "AP").is_none() {
                let ops = format!("q 0.6 G 0.5 w 0.25 0.25 {} {} re S Q", w - 0.5, h - 0.5);
                let res = self.inner.new_dict()?;
                let stream = form_xobject(&mut self.inner, ops.as_bytes(), res, [0.0, 0.0, w, h])?;
                let mut ap = self.inner.new_dict()?;
                ap.dict_put("N", stream)?;
                obj.dict_put("AP", ap)?;
            }
            return Ok(());
        }
        self.barcode_budget -= 1;
        // o /PMD acompanha o tamanho do campo (em polegadas), como o Acrobat
        if let Some(mut pmd) = obj.get_dict_inheritable("PMD")? {
            pmd.dict_put("Width", PdfObject::new_real(w / 72.0)?)?;
            pmd.dict_put("Height", PdfObject::new_real(h / 72.0)?)?;
        }
        let mut ops = format!("q 0.6 G 0.5 w 0.25 0.25 {} {} re S ", w - 0.5, h - 0.5);
        let mut res = self.inner.new_dict()?;
        match (value.is_empty(), modules(o, &value)) {
            (true, _) => {}
            (false, Ok(rows)) => {
                let (mh, mw) = (rows.len().max(1) as f32, rows[0].len().max(1) as f32);
                let quiet = o.symbology.quiet_zone();
                let s = ((w - 2.0) / (mw + 2.0 * quiet))
                    .min((h - 2.0) / (mh + 2.0 * quiet))
                    .max(0.01);
                let (ox, oy) = ((w - mw * s) / 2.0, (h - mh * s) / 2.0);
                ops += "0 g ";
                for (y, row) in rows.iter().enumerate() {
                    let top = h - oy - (y as f32 + 1.0) * s;
                    let mut x = 0;
                    while x < row.len() {
                        if !row[x] {
                            x += 1;
                            continue;
                        }
                        let start = x;
                        while x < row.len() && row[x] {
                            x += 1;
                        }
                        let _ = write!(
                            ops,
                            "{:.3} {:.3} {:.3} {:.3} re ",
                            ox + start as f32 * s,
                            top,
                            (x - start) as f32 * s,
                            s
                        );
                    }
                }
                ops += "f ";
            }
            (false, Err(why)) => {
                let _ = write!(
                    ops,
                    "BT /Helv 7 Tf 0.7 0 0 rg 3 {:.2} Td ({}) Tj ET ",
                    h / 2.0 - 3.0,
                    literal(&why)
                );
                let mut fonts = self.inner.new_dict()?;
                fonts.dict_put("Helv", self.helvetica()?)?;
                res.dict_put("Font", fonts)?;
            }
        }
        ops += "Q";
        let mut stream = form_xobject(&mut self.inner, ops.as_bytes(), res, [0.0, 0.0, w, h])?;
        stream.dict_put(KEY, PdfObject::new_string(&key)?)?;
        let mut ap = self.inner.new_dict()?;
        ap.dict_put("N", stream)?;
        obj.dict_put("AP", ap)?;
        Ok(())
    }

    /// Desenha os códigos de barras do documento (ao abrir: o MuPDF os mostraria como texto).
    /// Num documento assinado, nada muda (a aparência alterada apareceria como mudança depois da
    /// assinatura).
    pub(crate) fn draw_barcodes(&mut self) -> Result<()> {
        // sem formulário, nada a fazer (não carrega as páginas)
        let Some(mut acro) = crate::pdf_obj::get(&self.inner.catalog()?, "AcroForm") else {
            return Ok(());
        };
        let has_fields =
            crate::pdf_obj::get(&acro, "Fields").is_some_and(|f| f.len().unwrap_or(0) > 0);
        if !has_fields || self.signed_on_disk {
            return Ok(());
        }
        // /NeedAppearances: o MuPDF refaria as aparências (os códigos viram texto) a cada carga de
        // página; como o Acrobat ao salvar, gera as aparências e tira a marca
        if crate::pdf_obj::get(&acro, "NeedAppearances")
            .is_some_and(|n| n.as_bool().unwrap_or(false))
        {
            for p in 0..self.page_count()? {
                for mut w in self.inner.load_pdf_page(p)?.widgets() {
                    // os códigos são desenhados pelo app (o texto do MuPDF seria jogado fora)
                    if read_options(&w.annotation().object()).is_none() {
                        let _ = w.update();
                    }
                }
            }
            acro.dict_delete("NeedAppearances")?;
        }
        self.with_barcode_budget(|d| {
            for p in 0..d.page_count()? {
                for w in d.inner.load_pdf_page(p)?.widgets() {
                    let mut obj = w.annotation().object();
                    if let Some(o) = read_options(&obj) {
                        d.draw_barcode(&mut obj, o)?;
                    }
                }
            }
            Ok(())
        })
    }

    /// Roda `f` com o orçamento de uma passada (no máximo `MAX_PER_PASS` códigos codificados).
    /// Dentro de outra passada, usa o que sobrou dela: uma operação que redesenha muitas páginas
    /// não ganha um orçamento novo a cada página.
    pub(crate) fn with_barcode_budget<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let saved = self.barcode_budget;
        self.barcode_budget = saved.min(MAX_PER_PASS);
        let out = f(self);
        // devolve o que a passada de dentro não gastou só se esta era a de fora
        self.barcode_budget = if saved > MAX_PER_PASS {
            saved
        } else {
            self.barcode_budget
        };
        out
    }
}
