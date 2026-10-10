//! Gera um PDF assinado (para conferir com validadores externos): sign_sample <saida.pdf> <id.pfx>
use pdf_core::{DigitalId, PdfDoc, PdfRect, SignPlace};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&args[1]);
    std::fs::write(&out, pdf_core::sample::sample_pdf(2, 0)).unwrap();
    let id = DigitalId::create_self_signed(
        "Ana Souza",
        "ana@exemplo.com",
        "Exemplo",
        "s3nha",
        std::path::Path::new(&args[2]),
    )
    .unwrap();
    let mut d = PdfDoc::open(&out, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 600.0, 320.0, 660.0),
        },
        &id,
        "Aprovado",
        "São Paulo",
    )
    .unwrap();
}
