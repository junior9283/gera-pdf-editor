use pdf_core::{
    compare, compare_files, AnnotKind, ChangeKind, CompareInput, CompareOptions, CompareStage,
    Comparison, PdfDoc, PdfError,
};
use std::path::{Path, PathBuf};

/// Uma página de teste: linhas de texto (começando com `*`: em negrito) e, opcionalmente, uma
/// imagem 2×2 cinza (o valor do cinza) de 100×100 pt em x=300, y=300.
struct Pg<'a> {
    lines: &'a [&'a str],
    image: Option<u8>,
}

fn pg<'a>(lines: &'a [&'a str]) -> Pg<'a> {
    Pg { lines, image: None }
}

/// Escreve um PDF com as páginas (612×792; `rotate` em todas) e devolve o caminho.
fn text_pdf(dir: &Path, name: &str, pages: &[Pg], rotate: i32) -> PathBuf {
    let n = pages.len();
    // objetos: 1 catálogo, 2 páginas, 3 Helvetica, 4 Helvetica-Bold, depois 3 por página
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 3 * i)).collect();
    let mut objs = vec![
        "1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_string(),
        format!("2 0 obj<</Type/Pages/Kids[{}]/Count {n}>>endobj\n", kids.join(" ")),
        "3 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>endobj\n"
            .into(),
        "4 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica-Bold/Encoding/WinAnsiEncoding>>endobj\n"
            .into(),
    ];
    for (i, p) in pages.iter().enumerate() {
        let (page, contents, img) = (5 + 3 * i, 6 + 3 * i, 7 + 3 * i);
        let mut ops = String::new();
        for (k, line) in p.lines.iter().enumerate() {
            let (font, text) = match line.strip_prefix('*') {
                Some(t) => ("F2", t),
                None => ("F1", *line),
            };
            ops.push_str(&format!(
                "BT /{font} 12 Tf 72 {} Td ({text}) Tj ET\n",
                700 - 20 * k as i32
            ));
        }
        if p.image.is_some() {
            ops.push_str("q 100 0 0 100 300 300 cm /Im0 Do Q\n");
        }
        objs.push(format!(
            "{page} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Rotate {rotate}\
             /Resources<</Font<</F1 3 0 R/F2 4 0 R>>/XObject<</Im0 {img} 0 R>>>>/Contents {contents} 0 R>>endobj\n"
        ));
        objs.push(format!(
            "{contents} 0 obj<</Length {}>>stream\n{ops}\nendstream endobj\n",
            ops.len() + 1
        ));
        let g = p.image.unwrap_or(128) as char;
        objs.push(format!(
            "{img} 0 obj<</Type/XObject/Subtype/Image/Width 2/Height 2/ColorSpace/DeviceGray\
             /BitsPerComponent 8/Length 4>>stream\n{g}{g}{g}{g}\nendstream endobj\n"
        ));
    }
    let path = dir.join(name);
    let mut bytes = b"%PDF-1.4\n".to_vec();
    for o in objs {
        // o cinza pode passar de 127: grava o byte, não o UTF-8 do char
        bytes.extend(o.chars().map(|c| c as u32 as u8));
    }
    bytes.extend_from_slice(b"trailer<</Root 1 0 R>>\n");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn opts(old: &Path, new: &Path) -> CompareOptions {
    let input = |p: &Path| CompareInput {
        path: p.to_path_buf(),
        password: None,
        pages: None,
    };
    CompareOptions {
        old: input(old),
        new: input(new),
        text_only: false,
        formatting: false,
    }
}

fn run(o: &CompareOptions) -> Comparison {
    compare(o, &mut |_| true).unwrap()
}

fn kinds(c: &Comparison) -> Vec<ChangeKind> {
    c.changes.iter().map(|x| x.kind).collect()
}

#[test]
fn identical_documents_have_no_changes() {
    let dir = tempfile::tempdir().unwrap();
    let pages = [
        pg(&["um texto qualquer", "outra linha"]),
        pg(&["segunda pagina"]),
    ];
    let a = text_pdf(dir.path(), "a.pdf", &pages, 0);
    let b = text_pdf(dir.path(), "b.pdf", &pages, 0);
    let c = run(&opts(&a, &b));
    assert!(c.changes.is_empty(), "{:?}", kinds(&c));
    assert_eq!(c.pairs, vec![(Some(0), Some(0)), (Some(1), Some(1))]);
    assert_eq!((c.old_pages, c.new_pages), (2, 2));
}

#[test]
fn replaced_inserted_deleted_words() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(
        dir.path(),
        "a.pdf",
        &[pg(&["o gato preto", "linha fixa aqui"])],
        0,
    );
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&["o cao preto dormiu", "linha fixa aqui"])],
        0,
    );
    let c = run(&opts(&a, &b));
    assert_eq!(kinds(&c), vec![ChangeKind::Replaced, ChangeKind::Inserted]);
    let rep = &c.changes[0];
    assert!(rep.description.contains("gato") && rep.description.contains("cao"));
    assert!(!rep.old_rects.is_empty() && !rep.new_rects.is_empty());
    let ins = &c.changes[1];
    assert!(ins.description.contains("dormiu"));
    assert!(ins.old_rects.is_empty() && !ins.new_rects.is_empty());
}

#[test]
fn deleted_word_is_on_the_old_side() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["nada muito importante"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["nada importante"])], 0);
    let c = run(&opts(&a, &b));
    assert_eq!(kinds(&c), vec![ChangeKind::Deleted]);
    let d = &c.changes[0];
    assert!(d.description.contains("muito"));
    assert_eq!(d.old_rects.len(), 1);
    assert!(d.new_rects.is_empty());
    // "muito" fica depois de "nada" na linha (x maior que 72 + largura de "nada")
    assert!(d.old_rects[0].x0 > 90.0);
}

#[test]
fn inserted_page_stands_alone() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = [
        "Capitulo um",
        "texto do primeiro capitulo com varias palavras",
    ];
    let p2 = [
        "Capitulo dois",
        "texto do segundo capitulo com outras palavras",
    ];
    let novo = ["Pagina nova", "conteudo inteiramente diferente e inedito"];
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&p1), pg(&p2)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&p1), pg(&novo), pg(&p2)], 0);
    let c = run(&opts(&a, &b));
    assert_eq!(
        c.pairs,
        vec![(Some(0), Some(0)), (None, Some(1)), (Some(1), Some(2))]
    );
    assert!(c
        .changes
        .iter()
        .all(|x| x.kind == ChangeKind::Inserted && x.pair == 1));
}

#[test]
fn page_ranges_limit_what_is_compared() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["igual"]), pg(&["antigo"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["igual"]), pg(&["novo"])], 0);
    let mut o = opts(&a, &b);
    o.old.pages = Some(vec![0]);
    o.new.pages = Some(vec![0]);
    let c = run(&o);
    assert!(c.changes.is_empty());
    assert_eq!(c.pairs, vec![(Some(0), Some(0))]);
}

#[test]
fn cancelling_stops_the_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["a"])], 0);
    let r = compare(&opts(&a, &a), &mut |_| false);
    assert!(matches!(r, Err(PdfError::Cancelled)), "{r:?}");
}

#[test]
fn restricted_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["texto"])], 0);
    let locked = dir.path().join("restrito.pdf");
    let doc = mupdf::pdf::PdfDocument::open(a.to_str().unwrap()).unwrap();
    let mut w = mupdf::pdf::PdfWriteOptions::default();
    w.set_encryption(mupdf::pdf::Encryption::Aes256)
        .set_owner_password("dono")
        .set_permissions(mupdf::pdf::Permission::PRINT);
    doc.save_with_options(locked.to_str().unwrap(), w).unwrap();
    let r = compare(&opts(&a, &locked), &mut |_| true);
    assert!(matches!(r, Err(PdfError::InvalidPages(m)) if m.contains("protegido")));
}

#[test]
fn wrong_password_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["texto"])], 0);
    let locked = dir.path().join("senha.pdf");
    let doc = mupdf::pdf::PdfDocument::open(a.to_str().unwrap()).unwrap();
    let mut w = mupdf::pdf::PdfWriteOptions::default();
    w.set_encryption(mupdf::pdf::Encryption::Aes256)
        .set_owner_password("dono")
        .set_user_password("segredo");
    doc.save_with_options(locked.to_str().unwrap(), w).unwrap();
    let r = compare(&opts(&a, &locked), &mut |_| true);
    assert!(matches!(r, Err(PdfError::PasswordRequired)), "{r:?}");
    let mut o = opts(&a, &locked);
    o.new.password = Some("segredo".into());
    assert!(run(&o).changes.is_empty());
}

fn with_image<'a>(lines: &'a [&'a str], gray: u8) -> Pg<'a> {
    Pg {
        lines,
        image: Some(gray),
    }
}

#[test]
fn changed_image_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[with_image(&["legenda"], 40)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[with_image(&["legenda"], 200)], 0);
    let c = run(&opts(&a, &b));
    assert_eq!(kinds(&c), vec![ChangeKind::Image]);
    let img = &c.changes[0];
    assert_eq!((img.old_rects.len(), img.new_rects.len()), (1, 1));
    // 100×100 pt em x=300 do PDF (na tela, y = 792 - 400)
    let r = img.new_rects[0];
    assert!(
        (r.x0 - 300.0).abs() < 1.0 && (r.y0 - 392.0).abs() < 1.0,
        "{r:?}"
    );
    assert!(img.description.contains("substituída"));
}

#[test]
fn same_image_is_not_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[with_image(&["x"], 40)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[with_image(&["x"], 40)], 0);
    assert!(run(&opts(&a, &b)).changes.is_empty());
}

#[test]
fn added_image_is_inserted() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["x"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[with_image(&["x"], 40)], 0);
    let c = run(&opts(&a, &b));
    assert_eq!(kinds(&c), vec![ChangeKind::Image]);
    assert!(c.changes[0].old_rects.is_empty());
    assert!(c.changes[0].description.contains("inserida"));
}

#[test]
fn text_only_skips_images() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[with_image(&["legenda"], 40)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[with_image(&["legenda"], 200)], 0);
    let mut o = opts(&a, &b);
    o.text_only = true;
    assert!(run(&o).changes.is_empty());
}

#[test]
fn image_only_documents_compare_images() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(
        dir.path(),
        "a.pdf",
        &[with_image(&[], 40), with_image(&[], 90)],
        0,
    );
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[with_image(&[], 40), with_image(&[], 91)],
        0,
    );
    let c = run(&opts(&a, &b));
    assert_eq!(c.pairs, vec![(Some(0), Some(0)), (Some(1), Some(1))]);
    assert_eq!(kinds(&c), vec![ChangeKind::Image]);
    assert_eq!(c.changes[0].pair, 1);
}

#[test]
fn formatting_only_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["titulo do texto", "corpo"])], 0);
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&["*titulo do texto", "corpo"])],
        0,
    );
    assert!(run(&opts(&a, &b)).changes.is_empty());
    let mut o = opts(&a, &b);
    o.formatting = true;
    let c = run(&o);
    assert_eq!(kinds(&c), vec![ChangeKind::Formatting]);
    let f = &c.changes[0];
    // as três palavras seguidas, com a mesma mudança, viram uma alteração só (uma linha)
    assert_eq!(f.new_rects.len(), 1);
    assert!(f.description.contains("Helvetica 12") && f.description.contains("Helvetica-Bold 12"));
}

/// Imagem de 5000×5000 pixels num fluxo comprimido pequeno (bomba de descompressão): a comparação
/// não a decodifica e termina.
#[test]
fn decompression_bomb_is_not_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let ops = "q 100 0 0 100 300 300 cm /Im0 Do Q";
    let mut raw = format!(
        "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
         2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
         3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</XObject<</Im0 5 0 R>>>>/Contents 4 0 R>>endobj\n\
         4 0 obj<</Length {}>>stream\n{ops}\nendstream endobj\n\
         5 0 obj<</Type/XObject/Subtype/Image/Width 5000/Height 5000/ColorSpace/DeviceGray/BitsPerComponent 8/Length {}>>stream\n",
        ops.len() + 1,
        5000 * 5000
    )
    .into_bytes();
    raw.extend(std::iter::repeat_n(0u8, 5000 * 5000));
    raw.extend_from_slice(b"\nendstream endobj\ntrailer<</Root 1 0 R>>\n");
    let doc = mupdf::pdf::PdfDocument::from_bytes(&raw).unwrap();
    let path = dir.path().join("bomba.pdf");
    let mut w = mupdf::pdf::PdfWriteOptions::default();
    w.set_compress(true);
    doc.save_with_options(path.to_str().unwrap(), w).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() < 200_000);
    let c = run(&opts(&path, &path));
    assert!(c.changes.is_empty());
}

/// Compara e grava o resultado em `dir/resultado.pdf`.
fn report(dir: &Path, o: &CompareOptions) -> (Comparison, PathBuf) {
    let dest = dir.join("resultado.pdf");
    let c = compare_files(o, &dest, &mut |_| true).unwrap();
    (c, dest)
}

#[test]
fn report_has_summary_pairs_and_locked_annotations() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = ["o gato preto", "linha fixa aqui"];
    let p1b = ["o cao preto dormiu", "linha fixa aqui"];
    let p2 = [
        "Capitulo dois",
        "texto do segundo capitulo com outras palavras",
    ];
    let novo = ["Pagina nova", "conteudo inteiramente diferente e inedito"];
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&p1), pg(&p2)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&p1b), pg(&novo), pg(&p2)], 0);
    let (c, dest) = report(dir.path(), &opts(&a, &b));
    let doc = PdfDoc::open(&dest, None).unwrap();
    assert_eq!(doc.page_count().unwrap(), 1 + c.pairs.len() as i32);
    let summary = doc.page_text(0).unwrap();
    assert!(summary.contains("Resultados da comparação"), "{summary}");
    assert!(summary.contains("a.pdf") && summary.contains("b.pdf"));
    let annots = doc.all_annotations().unwrap();
    let marks: Vec<_> = annots
        .iter()
        .filter(|a| a.kind == AnnotKind::Highlight)
        .collect();
    assert_eq!(marks.len(), c.changes.len());
    assert!(marks
        .iter()
        .all(|a| a.author == "Comparação" && a.page >= 1));
    assert!(marks.iter().any(|a| a.contents.contains("dormiu")));
    // travadas, com o assunto do tipo; o PDF diz que é um resultado de comparação
    let raw = mupdf::pdf::PdfDocument::open(dest.to_str().unwrap()).unwrap();
    let page = raw.load_pdf_page(1).unwrap();
    for annot in page.annotations() {
        if annot.r#type().unwrap() == mupdf::pdf::PdfAnnotationType::Highlight {
            assert!(annot
                .flags()
                .unwrap()
                .contains(mupdf::pdf::AnnotationFlags::IS_LOCKED));
            let subj = annot.object().get_dict("Subj").unwrap().unwrap();
            assert!(!subj.as_string().unwrap().is_empty());
        }
    }
    let info = raw.trailer().unwrap().get_dict("Info").unwrap().unwrap();
    assert!(info
        .get_dict("GeraCompare")
        .unwrap()
        .unwrap()
        .as_bool()
        .unwrap());
    assert_eq!(raw.outlines().unwrap().len(), 1 + c.pairs.len());
    // a página sem par diz o que aconteceu
    let t = doc.page_text(2).unwrap();
    assert!(t.contains("Página inserida"), "{t:?}");
}

#[test]
fn identical_documents_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["igual"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["igual"])], 0);
    let (_, dest) = report(dir.path(), &opts(&a, &b));
    let doc = PdfDoc::open(&dest, None).unwrap();
    assert!(doc
        .page_text(0)
        .unwrap()
        .contains("Não foram encontradas diferenças"));
    assert!(doc.all_annotations().unwrap().is_empty());
}

/// A palavra sob a anotação, no resultado, é a palavra nova (também com a página girada).
fn marks_land_on_their_words(rotate: i32) {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(
        dir.path(),
        "a.pdf",
        &[pg(&["o gato preto", "linha"])],
        rotate,
    );
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&["o cao preto", "linha"])],
        rotate,
    );
    let (_, dest) = report(dir.path(), &opts(&a, &b));
    let doc = PdfDoc::open(&dest, None).unwrap();
    let mark = doc
        .all_annotations()
        .unwrap()
        .into_iter()
        .find(|a| a.kind == AnnotKind::Highlight)
        .unwrap();
    let words: Vec<String> = doc
        .words_in_rect(mark.page, mark.rect)
        .unwrap()
        .into_iter()
        .map(|w| w.text)
        .collect();
    assert!(words.iter().any(|w| w == "cao"), "{words:?}");
    assert!(!words.iter().any(|w| w == "gato"), "{words:?}");
}

#[test]
fn marks_land_on_their_words_upright() {
    marks_land_on_their_words(0);
}

#[test]
fn rotated_pages_keep_marks_on_their_words() {
    marks_land_on_their_words(90);
}

#[test]
fn cancel_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["a b"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["a c"])], 0);
    let dest = dir.path().join("resultado.pdf");
    let r = compare_files(&opts(&a, &b), &dest, &mut |s| {
        !matches!(s, CompareStage::Writing(..))
    });
    assert!(r.is_err());
    assert!(!dest.exists());
}

#[test]
fn destination_cannot_be_an_input() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["a"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["b"])], 0);
    let before = std::fs::read(&a).unwrap();
    let r = compare_files(&opts(&a, &b), &a, &mut |_| true);
    assert!(matches!(r, Err(PdfError::Refused(_))), "{r:?}");
    assert_eq!(std::fs::read(&a).unwrap(), before);
}

#[test]
fn result_is_recognized_when_opened() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["um"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["dois"])], 0);
    let (_, dest) = report(dir.path(), &opts(&a, &b));
    assert!(
        PdfDoc::open(&dest, None)
            .unwrap()
            .summary()
            .unwrap()
            .is_comparison
    );
    assert!(
        !PdfDoc::open(&a, None)
            .unwrap()
            .summary()
            .unwrap()
            .is_comparison
    );
}

/// O lado vazio (página inserida) é pintado de cinza claro: a cor não pode vazar para a página do
/// outro lado, que escreve o texto na cor padrão (preto).
#[test]
fn blank_side_color_does_not_leak_into_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["igual"])], 0);
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&["igual"]), pg(&["nova pagina com texto"])],
        0,
    );
    let (c, dest) = report(dir.path(), &opts(&a, &b));
    assert_eq!(c.pairs[1], (None, Some(1)));
    let doc = PdfDoc::open(&dest, None).unwrap();
    let img = doc.render(2, 1.0, false).unwrap();
    let darkest = img
        .rgb
        .chunks(3)
        .map(|p| p.iter().map(|&v| v as u32).sum::<u32>())
        .min()
        .unwrap();
    // texto preto sob o destaque verde claro (multiplicação): bem escuro
    assert!(darkest < 200, "o texto saiu claro demais: {darkest}");
}

const HEADER: &str = "ACME Relatorio Anual 2026 Confidencial";
const FOOTER: &str = "Rodape da empresa ACME";

/// As alterações só podem cair nos pares `expected`.
fn changes_only_in(c: &Comparison, expected: &[usize]) {
    for ch in &c.changes {
        assert!(
            expected.contains(&ch.pair),
            "alteração no par {}: {}",
            ch.pair,
            ch.description
        );
    }
}

#[test]
fn repeated_header_does_not_mark_unchanged_pages() {
    let dir = tempfile::tempdir().unwrap();
    let body = |t: &'static str| [HEADER, t, FOOTER];
    let (p1, p2, p3) = (
        body("primeira pagina fala de vendas"),
        body("segunda pagina fala de custos"),
        body("terceira pagina fala de metas"),
    );
    let ins = body("nova pagina inserida");
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&p1), pg(&p2), pg(&p3)], 0);
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&p1), pg(&ins), pg(&p2), pg(&p3)],
        0,
    );
    let c = run(&opts(&a, &b));
    assert_eq!(
        c.pairs,
        vec![
            (Some(0), Some(0)),
            (None, Some(1)),
            (Some(1), Some(2)),
            (Some(2), Some(3))
        ]
    );
    changes_only_in(&c, &[1]);
    // e ao contrário: página excluída
    let c = run(&opts(&b, &a));
    assert_eq!(
        c.pairs,
        vec![
            (Some(0), Some(0)),
            (Some(1), None),
            (Some(2), Some(1)),
            (Some(3), Some(2))
        ]
    );
    changes_only_in(&c, &[1]);
}

#[test]
fn repeated_header_does_not_misalign_pages() {
    let dir = tempfile::tempdir().unwrap();
    let body = |t: &'static str| [HEADER, t];
    let (pa, pb, pc) = (
        body("capitulo a com texto proprio"),
        body("capitulo b com outro texto"),
        body("capitulo c encerra o documento"),
    );
    let (px, ph) = (body("pagina x totalmente nova"), [HEADER, ""]);
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&pa), pg(&pb), pg(&pc)], 0);
    let b = text_pdf(
        dir.path(),
        "b.pdf",
        &[pg(&pa), pg(&px), pg(&pb), pg(&ph[..1]), pg(&pc)],
        0,
    );
    let c = run(&opts(&a, &b));
    assert_eq!(
        c.pairs,
        vec![
            (Some(0), Some(0)),
            (None, Some(1)),
            (Some(1), Some(2)),
            (None, Some(3)),
            (Some(2), Some(4))
        ]
    );
    changes_only_in(&c, &[1, 3]);
}

#[test]
fn blank_inserted_page_is_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["texto"])], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&["texto"]), pg(&[])], 0);
    let (c, dest) = report(dir.path(), &opts(&a, &b));
    assert_eq!(c.pairs, vec![(Some(0), Some(0)), (None, Some(1))]);
    assert_eq!(kinds(&c), vec![ChangeKind::Inserted]);
    assert!(c.changes[0].description.contains("Página inserida"));
    assert!(!c.changes[0].new_rects.is_empty());
    let summary = PdfDoc::open(&dest, None).unwrap().page_text(0).unwrap();
    assert!(summary.contains("Páginas inseridas: 1"), "{summary}");
}

#[test]
fn documents_without_text_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let a = text_pdf(dir.path(), "a.pdf", &[with_image(&[], 40)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[with_image(&[], 90)], 0);
    let mut o = opts(&a, &b);
    o.text_only = true;
    let (c, dest) = report(dir.path(), &o);
    assert!(c.changes.is_empty());
    let summary = PdfDoc::open(&dest, None).unwrap().page_text(0).unwrap();
    assert!(summary.contains("OCR"), "{summary}");
    assert!(
        !summary.contains("Não foram encontradas diferenças"),
        "{summary}"
    );
}
