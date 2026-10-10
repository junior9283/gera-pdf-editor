use pdf_core::{compare, ChangeKind, CompareInput, CompareOptions, Comparison, PdfError};
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
    let pages = [pg(&["um texto qualquer", "outra linha"]), pg(&["segunda pagina"])];
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
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&["o gato preto", "linha fixa aqui"])], 0);
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
    let p1 = ["Capitulo um", "texto do primeiro capitulo com varias palavras"];
    let p2 = ["Capitulo dois", "texto do segundo capitulo com outras palavras"];
    let novo = ["Pagina nova", "conteudo inteiramente diferente e inedito"];
    let a = text_pdf(dir.path(), "a.pdf", &[pg(&p1), pg(&p2)], 0);
    let b = text_pdf(dir.path(), "b.pdf", &[pg(&p1), pg(&novo), pg(&p2)], 0);
    let c = run(&opts(&a, &b));
    assert_eq!(
        c.pairs,
        vec![(Some(0), Some(0)), (None, Some(1)), (Some(1), Some(2))]
    );
    assert!(c.changes.iter().all(|x| x.kind == ChangeKind::Inserted && x.pair == 1));
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
    assert!(matches!(r, Err(PdfError::Refused(_))), "{r:?}");
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
