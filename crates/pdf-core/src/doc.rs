use crate::error::{PdfError, Result};
use crate::types::{PageSize, RenderedPage};
use mupdf::pdf::PdfDocument;
use mupdf::{Colorspace, Matrix};
use std::path::{Path, PathBuf};

/// Limite de pixels por imagem renderizada (evita estourar a memória).
const MAX_PIXELS: f64 = 40_000_000.0;

pub struct PdfDoc {
    pub(crate) inner: PdfDocument,
    pub(crate) path: PathBuf,
    /// Guardada só em memória, para reabrir o arquivo (ex.: descartar mudanças após falha ao salvar).
    pub(crate) password: Option<String>,
}

/// Abre o arquivo e, se for protegido, autentica com `password`.
pub(crate) fn open_authenticated(path: &Path, password: Option<&str>) -> Result<PdfDocument> {
    let mut inner = PdfDocument::open(path).map_err(|e| PdfError::Open(e.to_string()))?;
    if inner.needs_password()? {
        match password {
            None => return Err(PdfError::PasswordRequired),
            Some(p) if !inner.authenticate(p)? => return Err(PdfError::WrongPassword),
            Some(_) => {}
        }
    }
    Ok(inner)
}

impl PdfDoc {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let inner = open_authenticated(path, password)?;
        Ok(Self {
            inner,
            path: path.to_path_buf(),
            password: password.map(str::to_owned),
        })
    }

    pub fn page_count(&self) -> Result<i32> {
        Ok(self.inner.page_count()?)
    }

    pub(crate) fn load(&self, page: i32) -> Result<mupdf::Page> {
        if page < 0 || page >= self.page_count()? {
            return Err(PdfError::PageOutOfRange(page));
        }
        Ok(self.inner.load_page(page)?)
    }

    pub fn page_size(&self, page: i32) -> Result<PageSize> {
        size_of(&self.load(page)?)
    }

    pub fn render(&self, page: i32, scale: f32, invert: bool) -> Result<RenderedPage> {
        let page = self.load(page)?;
        let size = size_of(&page)?;
        let pixels = f64::from(size.width) * f64::from(size.height) * f64::from(scale).powi(2);
        if !scale.is_finite() || scale <= 0.0 || pixels > MAX_PIXELS {
            return Err(PdfError::InvalidScale(scale));
        }
        let ctm = Matrix::new_scale(scale, scale);
        let pix = page.to_pixmap(&ctm, &Colorspace::device_rgb(), false, true)?;
        let (w, h, stride) = (
            pix.width() as usize,
            pix.height() as usize,
            pix.stride() as usize,
        );
        debug_assert_eq!(pix.n(), 3, "device_rgb sem alfa tem 3 canais");
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in pix.samples().chunks(stride).take(h) {
            let row = &row[..w * 3];
            if invert {
                rgb.extend(row.iter().map(|b| !b));
            } else {
                rgb.extend_from_slice(row);
            }
        }
        Ok(RenderedPage {
            width: w as u32,
            height: h as u32,
            rgb,
        })
    }
}

fn size_of(page: &mupdf::Page) -> Result<PageSize> {
    let b = page.bounds()?;
    Ok(PageSize {
        width: b.x1 - b.x0,
        height: b.y1 - b.y0,
    })
}
