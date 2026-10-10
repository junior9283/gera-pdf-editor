//! Reconhecer texto (OCR, como no Acrobat): páginas digitalizadas ganham uma camada de texto
//! invisível sobre as palavras — a aparência não muda, mas busca, seleção e cópia funcionam. O
//! reconhecimento é do Tesseract embutido no MuPDF.
use crate::annot_edit::SNAPSHOT_BUDGET;
use crate::doc::PdfDoc;
use crate::error::{PdfError, Result};
use mupdf::{Buffer, Colorspace, DocumentWriter, Font, Image, Matrix, Rect, TextPageFlags};
use std::path::{Path, PathBuf};

/// Idioma do reconhecimento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrLanguage {
    Portuguese,
    English,
    PortugueseEnglish,
}

impl OcrLanguage {
    fn code(self) -> &'static str {
        match self {
            OcrLanguage::Portuguese => "por",
            OcrLanguage::English => "eng",
            OcrLanguage::PortugueseEnglish => "por+eng",
        }
    }
}

/// Resultado: páginas reconhecidas, puladas (já tinham texto) e palavras acrescentadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OcrReport {
    pub recognized: usize,
    pub skipped: usize,
    pub words: usize,
}

/// Página com pelo menos isto de caracteres já tem texto (o Acrobat recusa página com texto
/// renderizável; uns poucos caracteres soltos ainda contam como digitalização).
const HAS_TEXT: usize = 3;

/// O Leptonica (usado pelo Tesseract) não aceita duas threads ao mesmo tempo: abas diferentes
/// reconhecem uma de cada vez.
static OCR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Leitura boa o bastante para não tentar as outras orientações.
const GOOD_READING: f32 = 20.0;

/// Qualidade da leitura: letras e números reconhecidos, pesados pela proporção deles (lixo de
/// texto deitado vem cheio de símbolos soltos).
fn reading_score(words: &[Word]) -> f32 {
    let (mut alnum, mut total) = (0usize, 0usize);
    for w in words {
        for c in w.text.chars() {
            total += 1;
            if c.is_alphanumeric() {
                alnum += 1;
            }
        }
    }
    if total == 0 {
        return 0.0;
    }
    let ratio = alnum as f32 / total as f32;
    alnum as f32 * ratio * ratio
}

/// Resolução do reconhecimento e o limite de pixels do render.
const OCR_DPI: f32 = 300.0;
const MAX_PIXELS: f32 = 40_000_000.0;

fn refused(msg: impl Into<String>) -> PdfError {
    PdfError::Refused(msg.into())
}

/// Pasta com os dados de idioma do Tesseract que têm todos os idiomas de `lang`.
pub fn tessdata_dir(lang: OcrLanguage) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("TESSDATA_PREFIX") {
        candidates.push(PathBuf::from(p));
    }
    // AppImage: os dados vão junto do programa
    if let Some(app) = std::env::var_os("APPDIR") {
        candidates.push(PathBuf::from(app).join("usr/share/tessdata"));
    }
    for p in [
        "/usr/share/tesseract/tessdata",
        "/usr/share/tessdata",
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
        "/usr/local/share/tessdata",
    ] {
        candidates.push(PathBuf::from(p));
    }
    candidates.into_iter().find(|dir| {
        lang.code()
            .split('+')
            .all(|l| dir.join(format!("{l}.traineddata")).is_file())
    })
}

/// Palavra reconhecida no espaço da página (como a tela a mostra): linha de base à esquerda,
/// largura e altura.
struct Word {
    text: String,
    origin: (f32, f32),
    width: f32,
    height: f32,
}

/// Arquivo temporário único para o resultado do Tesseract.
fn temp_pdf() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("gera-ocr-{}-{nanos}.pdf", std::process::id()))
}

/// Reconhece as palavras de uma imagem da página (`size` = tamanho da página na tela).
fn recognize(img: &Image, size: (f32, f32), lang: OcrLanguage, data: &Path) -> Result<Vec<Word>> {
    let _one_at_a_time = OCR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = temp_pdf();
    let utf8 = |p: &Path| -> Result<String> {
        p.to_str()
            .map(str::to_owned)
            .ok_or_else(|| refused("caminho não é UTF-8 válido"))
    };
    let result = (|| -> Result<Vec<Word>> {
        {
            let options = format!("ocr-language={},ocr-datadir={}", lang.code(), utf8(data)?);
            let mut w = DocumentWriter::with_ocr(utf8(&tmp)?.as_str(), &options)?;
            let rect = Rect::new(0.0, 0.0, size.0, size.1);
            let dev = w.begin_page(rect)?;
            dev.fill_image(
                img,
                &Matrix::new(size.0, 0.0, 0.0, size.1, 0.0, 0.0),
                1.0,
                Default::default(),
            )?;
            w.end_page(dev)?;
        }
        let doc = mupdf::Document::open(utf8(&tmp)?.as_str())?;
        let page = doc.load_page(0)?;
        let text = page.to_text_page(TextPageFlags::empty())?;
        let mut words = Vec::new();
        for block in text.blocks() {
            for line in block.lines() {
                let mut cur: Option<Word> = None;
                for ch in line.chars() {
                    let c = ch.char().unwrap_or(' ');
                    let q = ch.quad();
                    if c.is_whitespace() {
                        words.extend(cur.take());
                        continue;
                    }
                    let height = (q.ll.y - q.ul.y).abs().max((q.lr.y - q.ur.y).abs());
                    let right = q.ur.x.max(q.lr.x);
                    match cur.as_mut() {
                        Some(w) => {
                            w.text.push(c);
                            w.width = right - w.origin.0;
                            w.height = w.height.max(height);
                        }
                        None => {
                            let o = ch.origin();
                            cur = Some(Word {
                                text: c.to_string(),
                                origin: (o.x, o.y),
                                width: right - o.x,
                                height,
                            });
                        }
                    }
                }
                words.extend(cur);
            }
        }
        Ok(words)
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

impl PdfDoc {
    /// Reconhece o texto das `pages` (as que já têm texto são puladas). `progress(feitas, total)`
    /// é chamado antes de cada página; devolver `false` cancela. Desfazer volta tudo.
    pub fn recognize_text(
        &mut self,
        pages: &[i32],
        lang: OcrLanguage,
        mut progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<OcrReport> {
        if self.signed_on_disk || self.has_signatures()? {
            return Err(refused(
                "o documento está assinado: reconhecer texto alteraria as assinaturas",
            ));
        }
        if !self.security()?.permissions.modify {
            return Err(refused("o documento não permite alterações"));
        }
        let data = tessdata_dir(lang).ok_or_else(|| {
            refused("faltam os dados de idioma do OCR (tesseract-langpack-por e -eng)")
        })?;
        let before = self.snapshot()?;
        let mut report = OcrReport::default();
        let run = (|| -> Result<()> {
            for (i, &p) in pages.iter().enumerate() {
                if !progress(i, pages.len()) {
                    break;
                }
                self.check_page(p)?;
                let existing = self.page_text(p)?;
                if existing.chars().filter(|c| !c.is_whitespace()).count() >= HAS_TEXT {
                    report.skipped += 1;
                    continue;
                }
                report.words += self.ocr_page(p, lang, &data)?;
                report.recognized += 1;
            }
            Ok(())
        })();
        if let Err(e) = run {
            self.restore(&before)?;
            return Err(e);
        }
        if report.recognized > 0 && self.history.record_snapshot(before, SNAPSHOT_BUDGET) {
            self.mark_unsaved();
        }
        Ok(report)
    }

    /// Reconhece uma página e acrescenta a camada de texto invisível; devolve quantas palavras.
    /// Digitalização deitada ou de cabeça para baixo: tenta as outras orientações e fica com a de
    /// melhor leitura (o Tesseract só lê texto em pé).
    fn ocr_page(&mut self, page: i32, lang: OcrLanguage, data: &Path) -> Result<usize> {
        let fitz = self.load(page)?;
        let b = fitz.bounds()?;
        let (w, h) = (b.x1 - b.x0, b.y1 - b.y0);
        let scale = (OCR_DPI / 72.0).min((MAX_PIXELS / (w * h).max(1.0)).sqrt());
        let mut best: Option<(f32, Vec<Word>, Matrix)> = None;
        for angle in [0.0f32, 90.0, 270.0, 180.0] {
            // render girado: a imagem fica com a origem no canto da caixa girada
            let rot = Matrix::new_rotate(angle);
            let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
                .map(|(x, y)| (x * rot.a + y * rot.c, x * rot.b + y * rot.d));
            let mx = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min);
            let my = corners.iter().map(|c| c.1).fold(f32::MAX, f32::min);
            let (rw, rh) = if angle == 90.0 || angle == 270.0 {
                (h, w)
            } else {
                (w, h)
            };
            let mut ctm = Matrix::new(1.0, 0.0, 0.0, 1.0, -b.x0, -b.y0);
            ctm.concat(rot.clone());
            ctm.concat(Matrix::new(1.0, 0.0, 0.0, 1.0, -mx, -my));
            let mut render = ctm.clone();
            render.concat(Matrix::new_scale(scale, scale));
            let pix = fitz.to_pixmap(&render, &Colorspace::device_rgb(), false, false)?;
            let img = Image::from_pixmap(&pix)?;
            let words = recognize(&img, (rw, rh), lang, data)?;
            let score = reading_score(&words);
            // de volta: do espaço girado do OCR para a tela
            let back = ctm
                .invert()
                .ok_or_else(|| refused("página com transformação inválida"))?;
            if best.as_ref().is_none_or(|(s, ..)| score > *s) {
                best = Some((score, words, back));
            }
            if score >= GOOD_READING {
                break;
            }
        }
        let Some((_, words, back)) = best else {
            return Ok(0);
        };
        if words.is_empty() {
            return Ok(0);
        }
        let mut page_obj = self.inner.find_page(page)?;
        // da tela (com /Rotate e a origem da MediaBox) para o espaço do PDF
        let inv = page_obj
            .page_ctm()?
            .invert()
            .ok_or_else(|| refused("página com transformação inválida"))?;
        let font = Font::new("Helvetica")?;
        let mut ops = String::from("BT 3 Tr /GeraOCR 1 Tf\n");
        for wd in &words {
            let natural = crate::text_fit::text_width(&font, &wd.text, wd.height.max(1.0))?;
            if natural <= 0.0 || wd.width <= 0.0 {
                continue;
            }
            let tz = 100.0 * wd.width / natural;
            // matriz do texto no espaço do OCR (y para baixo), depois tela, depois PDF
            let mut tm = Matrix::new(wd.height, 0.0, 0.0, -wd.height, wd.origin.0, wd.origin.1);
            tm.concat(back.clone());
            tm.concat(inv.clone());
            ops.push_str(&format!(
                "{tz:.2} Tz {:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm ({}) Tj\n",
                tm.a,
                tm.b,
                tm.c,
                tm.d,
                tm.e,
                tm.f,
                crate::stamp::literal(&wd.text)
            ));
        }
        ops.push_str("ET");
        // recursos: a fonte da camada (os herdados de /Pages passam a ser da página)
        let mut resources = match page_obj.get_dict("Resources")? {
            Some(r) => r,
            None => {
                let r = page_obj
                    .get_dict_inheritable("Resources")?
                    .map(|r| r.copy_dict())
                    .transpose()?
                    .map_or_else(|| self.inner.new_dict(), Ok)?;
                page_obj.dict_put("Resources", r)?;
                page_obj
                    .get_dict("Resources")?
                    .expect("acabou de ser posto")
            }
        };
        let mut fonts = match resources.get_dict("Font")? {
            Some(f) => f,
            None => {
                resources.dict_put("Font", self.inner.new_dict()?)?;
                resources.get_dict("Font")?.expect("acabou de ser posto")
            }
        };
        let helv = self.inner.add_object(&self.inner.new_object_from_str(
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>",
        )?)?;
        fonts.dict_put("GeraOCR", helv)?;
        // conteúdo: o original entre q/Q (estado gráfico limpo) e a camada por cima
        let stream = |doc: &mut mupdf::pdf::PdfDocument, s: &str| -> Result<_> {
            Ok(doc.add_stream(&Buffer::from_bytes(s.as_bytes())?, None, false)?)
        };
        let mut contents = self.inner.new_array()?;
        if let Some(old) = page_obj.get_dict("Contents")? {
            contents.array_push(stream(&mut self.inner, "q")?)?;
            if old.is_array()? {
                for i in 0..old.len()? as i32 {
                    if let Some(s) = old.get_array(i)? {
                        contents.array_push(s)?;
                    }
                }
            } else {
                contents.array_push(old)?;
            }
            contents.array_push(stream(&mut self.inner, "Q")?)?;
        }
        contents.array_push(stream(&mut self.inner, &ops)?)?;
        page_obj.dict_put("Contents", contents)?;
        Ok(words.len())
    }
}
