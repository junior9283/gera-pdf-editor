fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("uso: make_sample <saida.pdf> [paginas]");
    let pages = args
        .next()
        .map_or(100, |p| p.parse().expect("número de páginas inválido"));
    std::fs::write(path, pdf_core::sample::sample_pdf(pages, 0)).expect("falha ao gravar");
}
