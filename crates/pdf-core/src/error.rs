use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("não foi possível abrir o PDF: {0}")]
    Open(String),
    #[error("o PDF está protegido por senha")]
    PasswordRequired,
    #[error("senha incorreta")]
    WrongPassword,
    #[error("a página {0} não existe")]
    PageOutOfRange(i32),
    #[error("escala de renderização inválida: {0}")]
    InvalidScale(f32),
    #[error("anotação inválida: {0}")]
    InvalidAnnotation(String),
    #[error("operação de página inválida: {0}")]
    InvalidPages(String),
    #[error("erro do motor PDF: {0}")]
    Engine(String),
    #[error("assinatura digital: {0}")]
    Signature(String),
    #[error("erro de arquivo: {0}")]
    Io(#[from] std::io::Error),
}

impl From<mupdf::Error> for PdfError {
    fn from(e: mupdf::Error) -> Self {
        PdfError::Engine(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PdfError>;
