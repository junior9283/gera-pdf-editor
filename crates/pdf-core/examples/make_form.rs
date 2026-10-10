fn main() {
    let path = std::env::args().nth(1).expect("uso: make_form <saida.pdf>");
    std::fs::write(path, pdf_core::sample::form_pdf()).expect("falha ao gravar");
}
