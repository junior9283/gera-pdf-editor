mod common;
use common::write_sample;
use pdf_core::{combine, AnnotOp, NewAnnot, PdfDoc, SplitRule};

fn order(d: &PdfDoc) -> Vec<u32> {
    (0..d.page_count().unwrap())
        .map(|p| {
            let t = d.page_text(p).unwrap();
            t.split_whitespace()
                .skip_while(|w| *w != "Page")
                .nth(1)
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

fn open(p: &std::path::Path) -> PdfDoc {
    PdfDoc::open(p, None).unwrap()
}

#[test]
fn split_by_page_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "livro.pdf", 5, 0);
    let d = open(&path);
    let out = tempfile::tempdir().unwrap();
    let parts = d.split(SplitRule::Pages(2), out.path(), "livro").unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].file_name().unwrap(), "livro_Parte1.pdf");
    assert_eq!(order(&open(&parts[0])), [1, 2]);
    assert_eq!(order(&open(&parts[2])), [5]);
    assert!(!d.is_dirty());
}

#[test]
fn split_keeps_comments_and_bookmarks_of_each_part() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 4, 0);
    let mut d = open(&path);
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 3,
        at: (50.0, 50.0),
        text: "fim".into(),
    }))
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    let parts = d.split(SplitRule::Pages(2), out.path(), "a").unwrap();
    let second = open(&parts[1]);
    assert_eq!(second.annotations(1).unwrap()[0].contents, "fim");
    let marks = second.outline().unwrap();
    let titles: Vec<_> = marks.iter().map(|m| (m.title.as_str(), m.page)).collect();
    assert_eq!(titles, [("Capitulo 3", Some(0)), ("Capitulo 4", Some(1))]);
}

#[test]
fn split_by_size_puts_a_big_page_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 6, 0);
    let d = open(&path);
    let out = tempfile::tempdir().unwrap();
    // limite menor que qualquer arquivo: cada página sozinha, sem laço infinito
    let parts = d.split(SplitRule::MaxBytes(10), out.path(), "a").unwrap();
    assert_eq!(parts.len(), 6);
    let roomy = d
        .split(SplitRule::MaxBytes(50_000_000), out.path(), "b")
        .unwrap();
    assert_eq!(roomy.len(), 1);
}

#[test]
fn split_by_top_level_bookmarks() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 3, 0);
    let d = open(&path);
    let out = tempfile::tempdir().unwrap();
    let parts = d.split(SplitRule::TopBookmarks, out.path(), "a").unwrap();
    let names: Vec<_> = parts
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["a_Capitulo 1.pdf", "a_Capitulo 2.pdf", "a_Capitulo 3.pdf"]
    );
    assert_eq!(order(&open(&parts[1])), [2]);
}

#[test]
fn bookmark_titles_become_safe_file_names() {
    assert_eq!(
        pdf_core::safe_file_name("Cap. 1/2: Início?"),
        "Cap. 1_2_ Início_"
    );
    assert_eq!(pdf_core::safe_file_name("   "), "Parte");
}

#[test]
fn split_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 2, 0);
    let d = open(&path);
    std::fs::write(dir.path().join("a_Parte1.pdf"), b"meu").unwrap();
    let parts = d.split(SplitRule::Pages(1), dir.path(), "a").unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("a_Parte1.pdf")).unwrap(),
        b"meu"
    );
    assert_eq!(parts[0].file_name().unwrap(), "a_Parte1 (2).pdf");
}

#[test]
fn combine_keeps_order_comments_and_bookmarks_per_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_sample(&dir, "Relatório.pdf", 2, 0);
    let b = write_sample(&dir, "Anexo.pdf", 3, 0);
    let mut db = open(&b);
    db.apply(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (50.0, 50.0),
        text: "nota".into(),
    }))
    .unwrap();
    db.save(&b).unwrap();
    let dest = dir.path().join("junto.pdf");
    combine(&[(a.clone(), None), (b.clone(), None)], &dest).unwrap();
    let c = open(&dest);
    assert_eq!(order(&c), [1, 2, 1, 2, 3]);
    assert_eq!(c.annotations(2).unwrap()[0].contents, "nota");
    let marks: Vec<_> = c
        .outline()
        .unwrap()
        .into_iter()
        .map(|m| (m.title, m.page, m.depth))
        .collect();
    assert_eq!(marks[0], ("Relatório".into(), Some(0), 0));
    assert_eq!(marks[1], ("Capitulo 1".into(), Some(0), 1));
    assert!(marks.contains(&("Anexo".into(), Some(2), 0)));
    assert!(marks.contains(&("Capitulo 3".into(), Some(4), 1)));
}

#[test]
fn combine_same_file_twice() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_sample(&dir, "a.pdf", 2, 0);
    let dest = dir.path().join("dobro.pdf");
    combine(&[(a.clone(), None), (a.clone(), None)], &dest).unwrap();
    assert_eq!(order(&open(&dest)), [1, 2, 1, 2]);
    assert_eq!(
        open(&dest)
            .outline()
            .unwrap()
            .iter()
            .filter(|m| m.depth == 0)
            .count(),
        2
    );
}

#[test]
fn combine_needs_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let locked = common::encrypted_sample(&dir);
    let dest = dir.path().join("x.pdf");
    assert!(combine(&[(locked.clone(), None)], &dest).is_err());
    assert!(!dest.exists());
    combine(&[(locked, Some("segredo".into()))], &dest).unwrap();
    assert_eq!(open(&dest).page_count().unwrap(), 1);
}

#[test]
fn every_part_respects_the_size_limit_bookmarks_included() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 40, 0);
    let d = open(&path);
    let out = tempfile::tempdir().unwrap();
    let limit = 1500;
    let parts = d
        .split(SplitRule::MaxBytes(limit), out.path(), "a")
        .unwrap();
    assert!(parts.len() > 1);
    for p in &parts {
        let size = std::fs::metadata(p).unwrap().len();
        let pages = open(p).page_count().unwrap();
        assert!(
            size <= limit || pages == 1,
            "{} tem {size} bytes",
            p.display()
        );
    }
    let total: i32 = parts.iter().map(|p| open(p).page_count().unwrap()).sum();
    assert_eq!(total, 40);
}

#[test]
fn long_bookmark_titles_still_make_valid_file_names() {
    let long = "Capítulo ".repeat(60);
    let name = pdf_core::safe_file_name(&long);
    assert!(name.len() <= 150, "{}", name.len());
    assert_eq!(pdf_core::safe_file_name("fim. . "), "fim");
}
