use crate::error::{PdfError, Result};
use crate::types::PageSize;
use mupdf::pdf::PdfDocument;
use std::path::{Path, PathBuf};

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
        let b = self.load(page)?.bounds()?;
        Ok(PageSize {
            width: b.x1 - b.x0,
            height: b.y1 - b.y0,
        })
    }
}
