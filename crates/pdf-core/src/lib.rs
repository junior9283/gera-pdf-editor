//! Única camada que conhece o MuPDF.
pub mod sample;

mod annot_edit;
mod doc;
mod error;
mod forms;
mod pages;
pub(crate) mod split;
mod stamp;
mod text_fit;
mod types;
mod worker;

pub use doc::{max_render_scale, PdfDoc};
pub use error::{PdfError, Result};
pub use forms::{FieldInfo, FieldKind};
pub use split::{combine, safe_file_name, SplitRule};
pub use stamp::{HEIGHT as STAMP_HEIGHT, WIDTH as STAMP_WIDTH};
pub use types::*;
pub use worker::{CancelToken, Request, Response, Worker};
