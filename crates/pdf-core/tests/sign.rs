mod common;
use common::write_sample;
use pdf_core::{AnnotOp, DigitalId, Integrity, NewAnnot, NewFieldKind, PdfDoc, PdfRect, SignPlace};
use std::sync::OnceLock;

/// Um ID para todos os testes (gerar chave RSA custa).
fn id() -> &'static DigitalId {
    static ID: OnceLock<(tempfile::TempDir, DigitalId)> = OnceLock::new();
    &ID.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let id = DigitalId::create_self_signed(
            "Ana Souza",
            "ana@exemplo.com",
            "",
            "s3nha",
            &dir.path().join("ana.pfx"),
        )
        .unwrap();
        (dir, id)
    })
    .1
}

fn signed(rotate: i32) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 2, rotate);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 600.0, 300.0, 660.0),
        },
        id(),
        "Aprovado",
        "São Paulo",
    )
    .unwrap();
    (dir, path)
}

#[test]
fn a_signed_document_validates_with_unknown_identity() {
    let (_d, path) = signed(0);
    let d = PdfDoc::open(&path, None).unwrap();
    let s = d.signatures().unwrap();
    assert_eq!(s.len(), 1);
    let s = &s[0];
    assert_eq!(s.integrity, Integrity::Intact, "{:?}", s.integrity);
    assert!(!s.modified_after);
    assert_eq!(s.signer, "Ana Souza");
    assert_eq!(
        (s.reason.as_str(), s.location.as_str()),
        ("Aprovado", "São Paulo")
    );
    assert_eq!(s.fingerprints[0], id().fingerprint());
    assert!(s.signed_at.starts_with("D:"));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(19).any(|w| w == b"adbe.pkcs7.detached"));
    assert!(d.has_signatures().unwrap());
}

#[test]
fn changing_a_signed_byte_breaks_the_signature() {
    let (_d, path) = signed(0);
    let mut bytes = std::fs::read(&path).unwrap();
    // troca uma letra do conteúdo da página (dentro do intervalo assinado)
    let i = bytes.windows(5).position(|w| w == b"Hello").unwrap();
    bytes[i] = b'J';
    std::fs::write(&path, &bytes).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(
        matches!(s[0].integrity, Integrity::Broken(_)),
        "{:?}",
        s[0].integrity
    );
}

#[test]
fn editing_after_signing_keeps_the_signature_and_flags_the_change() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 1,
        at: (50.0, 50.0),
        text: "depois".into(),
    }))
    .unwrap();
    d.save(&path).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert_eq!(
        s[0].integrity,
        Integrity::Intact,
        "Salvar incremental não quebra a assinatura"
    );
    assert!(s[0].modified_after);
}

#[test]
fn a_second_signature_keeps_the_first_valid() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 1,
            rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        },
        id(),
        "",
        "",
    )
    .unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert_eq!(s.len(), 2);
    assert!(s.iter().all(|s| s.integrity == Integrity::Intact), "{s:?}");
    let first = s.iter().find(|s| s.reason == "Aprovado").unwrap();
    assert!(
        first.modified_after,
        "a segunda assinatura é uma revisão posterior"
    );
}

#[test]
fn an_empty_signature_field_can_be_signed() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let field = d
        .add_field(
            0,
            PdfRect::new(100.0, 100.0, 280.0, 140.0),
            NewFieldKind::Signature,
            "Assinatura1",
        )
        .unwrap();
    // o campo novo ainda não foi salvo: assinar grava as pendências antes
    d.sign_digitally(SignPlace::Field { page: 0, id: field }, id(), "", "")
        .unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let s = d.signatures().unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].field, "Assinatura1");
    assert_eq!(s[0].integrity, Integrity::Intact);
}

#[test]
fn password_protected_documents_are_signed_and_stay_protected() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::encrypted_sample(&dir);
    let mut d = PdfDoc::open(&path, Some("segredo")).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        },
        id(),
        "Ok",
        "",
    )
    .unwrap();
    assert!(PdfDoc::open(&path, None).is_err(), "continua com senha");
    let s = PdfDoc::open(&path, Some("segredo"))
        .unwrap()
        .signatures()
        .unwrap();
    assert_eq!(s[0].integrity, Integrity::Intact, "{:?}", s[0].integrity);
    assert_eq!(s[0].reason, "Ok");
}

#[test]
fn malformed_signatures_are_reported_not_crashing() {
    let (_d, path) = signed(0);
    let bytes = std::fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // ByteRange absurdo, com o mesmo tamanho para não mexer nos offsets
    let i = text.find("/ByteRange").unwrap();
    let j = i + text[i..].find(']').unwrap();
    let mut broken = bytes.clone();
    let fake = "/ByteRange[0 99999999 -5 7".to_owned();
    let fake = format!("{fake:<width$}", width = j - i);
    broken[i..j].copy_from_slice(fake.as_bytes());
    std::fs::write(&path, &broken).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(matches!(s[0].integrity, Integrity::Broken(_)));
}

#[test]
fn signature_on_a_rotated_page_is_drawn_upright() {
    let (_d, path) = signed(90);
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.signatures().unwrap()[0].integrity, Integrity::Intact);
}

#[test]
fn an_unrelated_certificate_in_the_signature_is_not_part_of_the_chain() {
    // um certificado confiável qualquer, embutido sem ter emitido o do titular, não conta
    let dir = tempfile::tempdir().unwrap();
    let other =
        DigitalId::create_self_signed("Outro", "", "", "x", &dir.path().join("o.pfx")).unwrap();
    let mut forged = id().clone();
    forged.chain = vec![other.cert_der.clone()];
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        &forged,
        "",
        "",
    )
    .unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert_eq!(s[0].fingerprints, vec![id().fingerprint()]);
}

#[test]
fn details_changed_after_signing_show_the_signed_ones() {
    let (_d, path) = signed(0);
    // revisão posterior trocando o motivo no dicionário da assinatura
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    let w = page.widgets().next().unwrap();
    let mut v = w.annotation().object().get_dict("V").unwrap().unwrap();
    v.dict_put(
        "Reason",
        mupdf::pdf::PdfObject::new_string("Forjado").unwrap(),
    )
    .unwrap();
    let mut opts = mupdf::pdf::PdfWriteOptions::default();
    opts.set_incremental(true);
    let mut out = Vec::new();
    doc.write_to_with_options(&mut out, opts).unwrap();
    std::fs::write(&path, out).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(s[0].modified_after);
    assert_eq!(s[0].reason, "Aprovado", "o motivo assinado, não o forjado");
}

fn intact(path: &std::path::Path) -> bool {
    let s = PdfDoc::open(path, None).unwrap().signatures().unwrap();
    !s.is_empty() && s.iter().all(|s| s.integrity == Integrity::Intact)
}

#[test]
fn undoing_a_page_change_and_saving_keeps_the_signature() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.apply_pages(pdf_core::PageOp::Rotate {
        pages: vec![1],
        quarter_turns: 1,
    })
    .unwrap();
    d.undo().unwrap();
    d.save(&path).unwrap();
    assert!(
        intact(&path),
        "desfazer por instantâneo não pode regravar o arquivo assinado"
    );
}

#[test]
fn reset_form_and_saving_keeps_the_signature() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.reset_form().unwrap();
    d.save(&path).unwrap();
    assert!(
        intact(&path),
        "Limpar formulário não apaga a assinatura nem regrava o arquivo"
    );
}

#[test]
fn deleting_the_signed_page_saves_incrementally() {
    let (_d, path) = signed(0);
    let before = std::fs::read(&path).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.apply_pages(pdf_core::PageOp::Delete { pages: vec![0] })
        .unwrap();
    d.save(&path).unwrap();
    let after = std::fs::read(&path).unwrap();
    assert!(
        after.starts_with(&before),
        "a revisão assinada continua no arquivo"
    );
}

#[test]
fn signing_after_save_as_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let other = dir.path().join("b.pdf");
    d.save(&other).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        id(),
        "",
        "",
    )
    .unwrap();
    assert!(intact(&other));
}

#[test]
fn a_repaired_pdf_can_be_signed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quebrado.pdf");
    // xref com offsets errados: o MuPDF repara ao abrir
    let mut bytes = pdf_core::sample::sample_pdf(1, 0);
    // startxref apontando para o lugar errado: o MuPDF reconstrói a tabela ao abrir
    let i = bytes.windows(9).rposition(|w| w == b"startxref").unwrap();
    let j = i + 10;
    bytes[j] = b'1';
    std::fs::write(&path, &bytes).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 150.0),
        },
        id(),
        "",
        "",
    )
    .unwrap();
    assert!(intact(&path));
}

#[test]
fn a_second_signature_does_not_cast_doubt_on_the_first() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.sign_digitally(
        SignPlace::New {
            page: 1,
            rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        },
        id(),
        "",
        "",
    )
    .unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    let first = s.iter().find(|s| s.reason == "Aprovado").unwrap();
    assert!(
        first.modified_after && first.changes_permitted,
        "só assinaturas e anotações depois"
    );
}

#[test]
fn changing_page_content_after_signing_is_not_permitted() {
    let (_d, path) = signed(0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.apply_pages(pdf_core::PageOp::Rotate {
        pages: vec![1],
        quarter_turns: 1,
    })
    .unwrap();
    d.save(&path).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(s[0].modified_after && !s[0].changes_permitted);
}

#[test]
fn the_certificate_was_valid_when_signing() {
    let (_d, path) = signed(0);
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(s[0].cert_valid_at_signing);
}

#[test]
fn swapping_a_page_resource_after_signing_is_not_permitted() {
    // o /Contents fica igual, mas a fonte usada pela página é trocada numa revisão posterior
    let (_d, path) = signed(0);
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.find_page(0).unwrap();
    let mut font = page
        .get_dict_inheritable("Resources")
        .unwrap()
        .unwrap()
        .get_dict("Font")
        .unwrap()
        .unwrap()
        .get_dict("F1")
        .unwrap()
        .unwrap();
    font.dict_put(
        "BaseFont",
        mupdf::pdf::PdfObject::new_name("Courier").unwrap(),
    )
    .unwrap();
    let mut opts = mupdf::pdf::PdfWriteOptions::default();
    opts.set_incremental(true);
    let mut out = Vec::new();
    doc.write_to_with_options(&mut out, opts).unwrap();
    std::fs::write(&path, out).unwrap();
    let s = PdfDoc::open(&path, None).unwrap().signatures().unwrap();
    assert!(s[0].modified_after);
    assert!(
        !s[0].changes_permitted,
        "a página mostra outra coisa: não é alteração permitida"
    );
}
