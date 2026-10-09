//! Única camada que conhece o MuPDF.
pub mod sample;

mod doc;
mod error;
mod types;
mod worker;

pub use doc::{max_render_scale, PdfDoc};
pub use error::{PdfError, Result};
pub use types::*;
pub use worker::{CancelToken, Request, Response, Worker};
