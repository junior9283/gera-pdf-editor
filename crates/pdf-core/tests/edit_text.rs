use pdf_core::{PdfDoc, PdfRect};

/// PDF de uma página Carta com o conteúdo dado; /F1 = Helvetica, /Im0 = imagem 1×1 e /Fm0 = formulário
/// com o texto "Dentro do formulario".
fn pdf(dir: &std::path::Path, content: &str, rotate: i32) -> std::path::PathBuf {
    let rot = if rotate != 0 {
        format!("/Rotate {rotate}")
    } else {
        String::new()
    };
    let form = "BT /F1 12 Tf 0 0 Td (Dentro do formulario) Tj ET";
    let objs = [
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        "2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n".into(),
        format!("3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]{rot}/Resources<</Font<</F1 6 0 R>>/XObject<</Im0 5 0 R/Fm0 7 0 R>>>>/Contents 4 0 R>>endobj\n"),
        format!("4 0 obj<</Length {}>>stream\n{content}\nendstream endobj\n", content.len() + 1),
        "5 0 obj<</Type/XObject/Subtype/Image/Width 1/Height 1/ColorSpace/DeviceGray/BitsPerComponent 8/Length 1>>stream\n0\nendstream endobj\n".into(),
        "6 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n".into(),
        format!("7 0 obj<</Type/XObject/Subtype/Form/BBox[0 0 612 792]/Resources<</Font<</F1 6 0 R>>>>/Length {}>>stream\n{form}\nendstream endobj\n", form.len() + 1),
    ];
    let path = dir.join(format!("texto{rotate}.pdf"));
    std::fs::write(
        &path,
        format!("%PDF-1.4\n{}trailer<</Root 1 0 R>>\n", objs.concat()),
    )
    .unwrap();
    path
}

/// Título, linha vermelha, imagem e texto de baixo.
const PAGE: &str = "BT /F1 18 Tf 72 720 Td (Texto acima) Tj ET \
    BT 1 0 0 rg /F1 14 Tf 72 600 Td (Segunda linha) Tj ET \
    q 200 0 0 150 72 400 cm /Im0 Do Q \
    BT /F1 18 Tf 72 350 Td (Texto abaixo) Tj ET";

fn open(content: &str, rotate: i32) -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&pdf(dir.path(), content, rotate), None).unwrap();
    (dir, d)
}

fn block_rect(d: &PdfDoc, text: &str) -> PdfRect {
    d.page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text.contains(text))
        .unwrap_or_else(|| panic!("bloco {text:?}"))
        .rect
}

#[test]
fn blocks_are_listed_with_style() {
    let (_dir, d) = open(PAGE, 0);
    let blocks = d.page_text_blocks(0).unwrap();
    let title = blocks.iter().find(|b| b.text == "Texto acima").unwrap();
    assert!((title.size - 18.0).abs() < 0.5, "{title:?}");
    // na tela (y para baixo) a linha de base está em 792 - 720 = 72
    assert!(title.rect.y0 < 72.0 && title.rect.y1 > 72.0 && (title.rect.x0 - 72.0).abs() < 2.0);
    let red = blocks.iter().find(|b| b.text == "Segunda linha").unwrap();
    assert_eq!(red.color, [255, 0, 0]);
    assert_eq!(title.color, [0, 0, 0]);
}

#[test]
fn editing_replaces_only_that_block() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Novo título").unwrap();
    let text = d.page_text(0).unwrap();
    assert!(text.contains("Novo título"), "{text}");
    assert!(!text.contains("Texto acima"), "{text}");
    assert!(text.contains("Texto abaixo") && text.contains("Segunda linha"));
    assert_eq!(d.page_images(0).unwrap().len(), 1);
    let new = block_rect(&d, "Novo título");
    assert!(
        (new.x0 - r.x0).abs() < 2.0 && (new.y0 - r.y0).abs() < 2.0,
        "{r:?} → {new:?}"
    );
}

#[test]
fn accents_and_symbols_round_trip() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "ação é útil — 10 € “sim”").unwrap();
    let text = d.page_text(0).unwrap();
    assert!(text.contains("ação é útil — 10 € “sim”"), "{text:?}");
}

#[test]
fn paragraphs_wrap_inside_their_width() {
    let (_dir, mut d) = open(
        "BT /F1 12 Tf 72 300 Td (Primeira linha do paragrafo) Tj 0 -14 Td (e a segunda dele) Tj ET",
        0,
    );
    let r = block_rect(&d, "Primeira linha");
    let long = "Este é um texto bem mais comprido que a largura original do parágrafo e quebra";
    d.edit_text_block(0, r, long).unwrap();
    let new = d
        .page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text.starts_with("Este é"))
        .unwrap();
    assert!(new.text.lines().count() >= 3, "{:?}", new.text);
    assert_eq!(new.text.replace('\n', " "), long);
    assert!(new.rect.x1 <= r.x1 + 1.0, "{r:?} → {:?}", new.rect);
}

#[test]
fn a_single_line_grows_to_the_right() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Um título bem mais comprido que o original")
        .unwrap();
    let new = block_rect(&d, "Um título");
    assert!(
        new.x1 > r.x1 + 50.0 && new.height() < r.height() * 1.5,
        "{new:?}"
    );
}

#[test]
fn line_breaks_are_kept() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Um\nDois").unwrap();
    let blocks = d.page_text_blocks(0).unwrap();
    assert!(blocks.iter().any(|b| b.text == "Um\nDois"), "{blocks:?}");
}

#[test]
fn style_is_kept() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Segunda linha");
    d.edit_text_block(0, r, "Outra linha").unwrap();
    let b = d
        .page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text == "Outra linha")
        .unwrap();
    assert_eq!(b.color, [255, 0, 0]);
    assert!((b.size - 14.0).abs() < 0.5, "{b:?}");
    // a mesma família (a Helvetica embutida pelo MuPDF chama-se Nimbus Sans)
    assert!(
        b.font.contains("Helvetica") || b.font.contains("NimbusSans"),
        "{b:?}"
    );
}

#[test]
fn close_neighbours_survive() {
    let (_dir, mut d) = open(
        "BT /F1 12 Tf 72 700 Td (Primeiro bloco) Tj ET BT /F1 12 Tf 300 700 Td (Vizinho do lado) Tj ET \
         BT /F1 30 Tf 72 664 Td (Grande) Tj ET",
        0,
    );
    let r = block_rect(&d, "Primeiro");
    d.edit_text_block(0, r, "Trocado").unwrap();
    let text = d.page_text(0).unwrap();
    assert!(
        text.contains("Vizinho do lado") && text.contains("Grande"),
        "{text}"
    );
    assert!(text.contains("Trocado") && !text.contains("Primeiro"));
}

#[test]
fn rotated_page_edits_in_screen_space() {
    let (_dir, mut d) = open(PAGE, 90);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Girado").unwrap();
    let b = d
        .page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text == "Girado")
        .unwrap();
    assert!(
        (b.rect.x0 - r.x0).abs() < 2.0 && (b.rect.y0 - r.y0).abs() < 2.0,
        "{r:?} → {:?}",
        b.rect
    );
}

#[test]
fn undo_brings_the_old_text_back() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Outro").unwrap();
    d.undo().unwrap();
    let text = d.page_text(0).unwrap();
    assert!(text.contains("Texto acima") && !text.contains("Outro"));
}

#[test]
fn empty_text_deletes_the_block() {
    let (_dir, mut d) = open(PAGE, 0);
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "  ").unwrap();
    let text = d.page_text(0).unwrap();
    assert!(!text.contains("Texto acima") && text.contains("Texto abaixo"));
}

#[test]
fn adding_text_places_it_at_the_click() {
    let (_dir, mut d) = open(PAGE, 0);
    d.add_text(0, (300.0, 500.0), "Texto novo").unwrap();
    let r = block_rect(&d, "Texto novo");
    assert!(
        (r.x0 - 300.0).abs() < 2.0 && (r.y0 - 500.0).abs() < 4.0,
        "{r:?}"
    );
}

#[test]
fn edited_text_survives_saving() {
    let dir = tempfile::tempdir().unwrap();
    let path = pdf(dir.path(), PAGE, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Salvo ação").unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let text = d.page_text(0).unwrap();
    assert!(
        text.contains("Salvo ação") && !text.contains("Texto acima"),
        "{text}"
    );
}

#[test]
fn text_that_cannot_be_removed_is_refused() {
    let (_dir, mut d) = open(
        "BT /F1 12 Tf 72 700 Td (Fora) Tj ET q 1 0 0 1 72 500 cm /Fm0 Do Q",
        0,
    );
    let Some(b) = d
        .page_text_blocks(0)
        .unwrap()
        .into_iter()
        .find(|b| b.text.contains("formulario"))
    else {
        return; // não listado: nada a editar
    };
    if d.edit_text_block(0, b.rect, "Trocado").is_err() {
        let text = d.page_text(0).unwrap();
        assert!(text.contains("Dentro do formulario") && !text.contains("Trocado"));
    } else {
        let text = d.page_text(0).unwrap();
        assert!(
            !text.contains("Dentro do formulario") && text.contains("Trocado"),
            "{text}"
        );
    }
}

#[test]
fn editing_twice_reuses_the_embedded_font() {
    let dir = tempfile::tempdir().unwrap();
    let path = pdf(dir.path(), PAGE, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Primeira").unwrap();
    let r = block_rect(&d, "Primeira");
    d.edit_text_block(0, r, "Segunda").unwrap();
    d.save(&path).unwrap();
    let raw = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    assert!(raw.contains("/GeraFn0") && !raw.contains("/GeraFn1"));
}

/// Um PDF que já traz uma fonte `GeraFn0` marcada como se fosse do app (de outro programa ou
/// maliciosa) não pode ser usada para escrever o texto novo: os glifos dela seriam outros.
#[test]
fn a_font_planted_in_the_file_is_not_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = pdf(dir.path(), PAGE, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = block_rect(&d, "Texto acima");
    d.edit_text_block(0, r, "Primeira").unwrap();
    d.save(&path).unwrap();
    // reaberto: a fonte do arquivo não é mais "do app" desta sessão
    let mut d = PdfDoc::open(&path, None).unwrap();
    let r = block_rect(&d, "Primeira");
    d.edit_text_block(0, r, "Segunda").unwrap();
    d.save(&path).unwrap();
    let raw = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    assert!(
        raw.contains("/GeraFn1"),
        "a segunda edição embute a própria fonte"
    );
    assert!(
        !raw.contains("/GeraFonte"),
        "nenhuma marca confiável vem do arquivo"
    );
}
