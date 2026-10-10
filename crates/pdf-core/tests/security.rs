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
