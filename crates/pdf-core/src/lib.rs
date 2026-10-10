//! Única camada que conhece o MuPDF.
pub mod sample;

mod annot_edit;
mod compare;
mod compare_report;
mod compress;
mod create_export;
mod digital_id;
mod doc;
mod edit_images;
mod edit_text;
mod error;
mod forms;
mod links;
mod ocr;
mod owner_password;
mod page_content;
mod pages;
mod pagination;
mod prepare;
mod redact;
mod security;
mod sign;
pub mod signature;
pub(crate) mod split;
mod stamp;
mod text_fit;
mod types;
mod worker;

pub use compare::{
    compare, Change, ChangeKind, CompareInput, CompareOptions, CompareStage, Comparison,
};
pub use compare_report::{compare_files, COMPARE_AUTHOR};
pub use compress::{CompressLevel, CompressReport};
pub use create_export::{create_pdf_from_images, ExportFormat};
pub use digital_id::{fingerprint, DigitalId};
pub use doc::{max_render_scale, PdfDoc};
pub use edit_images::{ImageEdit, PageImage};
pub use edit_text::{PageTextBlock, MIN_TEXT_WIDTH, NEW_TEXT_SIZE, TEXT_MARGIN};
pub use error::{PdfError, Result};
pub use forms::{FieldInfo, FieldKind, FormNotes};
pub use ocr::{tessdata_dir, OcrLanguage, OcrReport};
pub use pagination::{
    parse_ranges, Align, Bates, DateFormat, HeaderFooter, PageSet, Pagination, PaginationConfig,
    StdFont, TextLook, Watermark, WatermarkSource,
};
pub use prepare::{FieldProps, NewFieldKind, PrepareOp};
pub use redact::{RedactHit, RedactLook, RedactPattern, RedactQuery, SanitizeOptions};
pub use security::{Permissions, Protection, SecurityInfo};
pub use sign::{display_date, Integrity, SignPlace, SignatureInfo};
pub use split::{combine, safe_file_name, SplitRule};
pub use stamp::{HEIGHT as STAMP_HEIGHT, WIDTH as STAMP_WIDTH};
pub use types::*;
pub use worker::{CancelToken, Request, Response, Worker};
