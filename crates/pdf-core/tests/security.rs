mod common;
use common::write_sample;
use pdf_core::{PdfDoc, PdfError, Permissions, Protection};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn no_print() -> Permissions {
    Permissions {
        print: false,
        print_hq: false,
        ..Permissions::ALL
    }
}

#[test]
fn protecting_with_both_passwords_and_reopening() {
    let (_d, path, mut d) = doc();
    d.set_protection(Some(Protection {
        open_password: "abrir ção".into(),
        owner_password: "dono".into(),
        permissions: no_print(),
    }))
    .unwrap();
    d.save(&path).unwrap();
    assert!(matches!(
        PdfDoc::open(&path, None),
        Err(PdfError::PasswordRequired)
    ));
    let mut d = PdfDoc::open(&path, Some("abrir ção")).unwrap();
    let s = d.security().unwrap();
    assert!(s.encrypted && s.needs_user_password && !s.unlocked);
    assert!(!s.permissions.print && s.permissions.copy && s.permissions.annotate);
    assert!(d.unlock("errada").is_err());
    assert!(
        d.unlock("abrir ção").is_err(),
        "a senha de abrir não desbloqueia"
    );
    d.unlock("dono").unwrap();
    let s = d.security().unwrap();
    assert!(s.unlocked && s.permissions == Permissions::ALL);
}

#[test]
fn only_a_permissions_password_opens_without_asking_but_restricts() {
    let (_d, path, mut d) = doc();
    d.set_protection(Some(Protection {
        open_password: String::new(),
        owner_password: "dono".into(),
        permissions: Permissions {
            copy: false,
            modify: false,
            assemble: false,
            ..Permissions::ALL
        },
    }))
    .unwrap();
    d.save(&path).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let s = d.security().unwrap();
    assert!(s.encrypted && !s.needs_user_password);
    assert!(!s.permissions.copy && !s.permissions.assemble && s.permissions.print);
    assert!(d.unlock("outra").is_err());
    d.unlock("dono").unwrap();
    assert!(d.security().unwrap().permissions.copy);
}

#[test]
fn restrictions_need_a_permissions_password() {
    let (_d, _p, mut d) = doc();
    let r = d.set_protection(Some(Protection {
        open_password: "x".into(),
        owner_password: String::new(),
        permissions: no_print(),
    }));
    assert!(r.is_err());
}

#[test]
fn removing_security_needs_the_permissions_password() {
    let (_d, path, mut d) = doc();
    d.set_protection(Some(Protection {
        open_password: String::new(),
        owner_password: "dono".into(),
        permissions: no_print(),
    }))
    .unwrap();
    d.save(&path).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    assert!(d.set_protection(None).is_err(), "sem desbloquear");
    d.unlock("dono").unwrap();
    d.set_protection(None).unwrap();
    d.save(&path).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().security().unwrap();
    assert!(!s.encrypted && s.permissions == Permissions::ALL);
}

#[test]
fn an_open_password_alone_keeps_everything_allowed() {
    let (_d, path, mut d) = doc();
    d.set_protection(Some(Protection {
        open_password: "abrir".into(),
        owner_password: String::new(),
        permissions: Permissions::ALL,
    }))
    .unwrap();
    d.save(&path).unwrap();
    let d = PdfDoc::open(&path, Some("abrir")).unwrap();
    assert_eq!(d.security().unwrap().permissions, Permissions::ALL);
    // salvar de novo mantém a proteção
    let mut d = d;
    d.save(&path).unwrap();
    assert!(matches!(
        PdfDoc::open(&path, None),
        Err(PdfError::PasswordRequired)
    ));
}

#[test]
fn unprotected_documents_allow_everything() {
    let (_d, _p, d) = doc();
    let s = d.security().unwrap();
    assert!(!s.encrypted && s.permissions == Permissions::ALL);
}

/// Arquivo protegido por outro programa (o MuPDF), com a revisão de criptografia pedida.
fn foreign(dir: &tempfile::TempDir, enc: mupdf::pdf::Encryption, user: &str) -> std::path::PathBuf {
    let path = dir.path().join(format!("{enc:?}.pdf"));
    let doc = mupdf::pdf::PdfDocument::from_bytes(&pdf_core::sample::sample_pdf(1, 0)).unwrap();
    let mut o = mupdf::pdf::PdfWriteOptions::default();
    o.set_encryption(enc)
        .set_user_password(user)
        .set_owner_password("dono-certo")
        .set_permissions(mupdf::pdf::Permission::ACCESSIBILITY);
    doc.save_with_options(path.to_str().unwrap(), o).unwrap();
    path
}

#[test]
fn only_the_real_permissions_password_unlocks_in_every_revision() {
    use mupdf::pdf::Encryption::*;
    let dir = tempfile::tempdir().unwrap();
    let long = "a".repeat(36);
    for enc in [Rc4_40, Rc4_128, Aes128, Aes256] {
        let path = foreign(&dir, enc, &long);
        let mut d = PdfDoc::open(&path, Some(&long)).unwrap();
        // variante da senha de abrir que o MuPDF aceita como de usuário (corte em 32 bytes)
        let variant = format!("{}XYZ", &long[..32]);
        assert!(
            d.unlock(&variant).is_err(),
            "{enc:?}: variante da senha de abrir"
        );
        assert!(
            d.unlock(&long).is_err(),
            "{enc:?}: a própria senha de abrir"
        );
        d.unlock("dono-certo").unwrap();
        assert!(d.security().unwrap().unlocked, "{enc:?}");
    }
}

#[test]
fn opening_with_the_permissions_password_unlocks_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = foreign(&dir, mupdf::pdf::Encryption::Aes256, "abrir");
    let d = PdfDoc::open(&path, Some("dono-certo")).unwrap();
    let s = d.security().unwrap();
    assert!(s.unlocked && s.permissions == Permissions::ALL);
    let path = foreign(&dir, mupdf::pdf::Encryption::Rc4_128, "abrir");
    let d = PdfDoc::open(&path, Some("dono-certo")).unwrap();
    assert!(d.security().unwrap().unlocked);
}

#[test]
fn restricted_files_cannot_be_combined_or_inserted() {
    let dir = tempfile::tempdir().unwrap();
    let restricted = foreign(&dir, mupdf::pdf::Encryption::Aes256, "");
    let plain = write_sample(&dir, "b.pdf", 1, 0);
    let out = dir.path().join("c.pdf");
    let r = pdf_core::combine(&[(restricted.clone(), None), (plain.clone(), None)], &out);
    assert!(r.is_err(), "combinar arquivo restrito tiraria a proteção");
    let ok = pdf_core::combine(
        &[
            (restricted.clone(), Some("dono-certo".into())),
            (plain.clone(), None),
        ],
        &out,
    );
    assert!(ok.is_ok(), "com a senha de permissões, pode");
    let mut d = PdfDoc::open(&plain, None).unwrap();
    let ins = d.apply_pages(pdf_core::PageOp::InsertFrom {
        at: 0,
        path: restricted,
        password: None,
        range: None,
    });
    assert!(ins.is_err());
}

#[test]
fn signed_documents_refuse_new_security() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "s.pdf", 1, 0);
    let id = pdf_core::DigitalId::create_self_signed("A", "", "", "x", &dir.path().join("a.pfx"))
        .unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        pdf_core::SignPlace::New {
            page: 0,
            rect: pdf_core::PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        &id,
        "",
        "",
    )
    .unwrap();
    assert!(d
        .set_protection(Some(Protection {
            open_password: "x".into(),
            owner_password: String::new(),
            permissions: Permissions::ALL,
        }))
        .is_err());
}
