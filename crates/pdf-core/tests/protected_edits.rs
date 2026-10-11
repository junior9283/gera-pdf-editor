//! Edições num PDF com senha de abertura, depois de consultar a segurança (como o app faz ao
//! abrir): os textos novos precisam sair legíveis no arquivo salvo.
mod common;
use common::write_sample;
use pdf_core::{
    DestOp, OutlineOp, PageLabelRange, PageLabelStyle, PdfDoc, Permissions, Protection,
};

fn protected(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let mut d = PdfDoc::open(&write_sample(dir, "a.pdf", 2, 0), None).unwrap();
    d.set_protection(Some(Protection {
        open_password: "abc".into(),
        owner_password: "dono".into(),
        permissions: Permissions::ALL,
    }))
    .unwrap();
    let out = dir.path().join("protegido.pdf");
    d.save(&out).unwrap();
    out
}

#[test]
fn new_texts_survive_saving_a_password_protected_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = protected(&dir);
    let mut d = PdfDoc::open(&path, Some("abc")).unwrap();
    let sec = d.security().unwrap();
    assert!(sec.needs_user_password);
    d.apply_page_labels(
        0,
        1,
        Some(PageLabelRange {
            start_page: 0,
            style: PageLabelStyle::Decimal,
            prefix: "Cap-".into(),
            first: 1,
        }),
    )
    .unwrap();
    d.edit_destination(DestOp::Add {
        name: "Seção".into(),
        page: 1,
        at: None,
    })
    .unwrap();
    d.edit_outline(OutlineOp::Add {
        parent: None,
        after: None,
        title: "Introdução".into(),
        page: 0,
        at: None,
    })
    .unwrap();
    let _ = d.security().unwrap();
    let out = dir.path().join("depois.pdf");
    d.save(&out).unwrap();
    let back = PdfDoc::open(&out, Some("abc")).unwrap();
    assert_eq!(back.labels(), ["Cap-1", "Cap-2"]);
    assert!(back.destinations().iter().any(|x| x.name == "Seção"));
    assert!(back
        .outline_tree()
        .unwrap()
        .iter()
        .any(|n| n.title == "Introdução"));
}
