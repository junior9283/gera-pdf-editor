use pdf_core::DigitalId;

#[test]
fn a_self_signed_id_is_created_and_reopened_with_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ana.pfx");
    let id = DigitalId::create_self_signed(
        "Ana Lúcia Souza",
        "ana@exemplo.com",
        "Exemplo Ltda",
        "s3nha",
        &path,
    )
    .unwrap();
    assert_eq!(id.name, "Ana Lúcia Souza");
    assert_eq!(id.email, "ana@exemplo.com");
    let back = DigitalId::open(&path, "s3nha").unwrap();
    assert_eq!(back.name, "Ana Lúcia Souza");
    assert_eq!(back.fingerprint(), id.fingerprint());
    assert_eq!(back.fingerprint().len(), 64, "SHA-256 em hexadecimal");
    assert!(DigitalId::open(&path, "errada").is_err());
}

#[test]
fn a_file_that_is_not_pkcs12_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.pfx");
    std::fs::write(&path, b"nada").unwrap();
    assert!(DigitalId::open(&path, "x").is_err());
    assert!(DigitalId::open(dir.path(), "x").is_err(), "pasta");
}
