mod common;
use common::write_sample;
use pdf_core::{AttachmentOrigin, PdfDoc};

fn doc() -> (tempfile::TempDir, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let d = PdfDoc::open(&write_sample(&dir, "a.pdf", 2, 0), None).unwrap();
    (dir, d)
}

fn file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn add_gives_unique_names_and_bytes_round_trip() {
    let (dir, mut d) = doc();
    let src = file(&dir, "nota.txt", b"conteudo do anexo");
    let a = d.add_attachment(&src, "primeira").unwrap();
    let b = d.add_attachment(&src, "").unwrap();
    assert_eq!(a, "nota.txt");
    assert_eq!(b, "nota (2).txt", "nome repetido ganha número");
    let list = d.attachments();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].description, "primeira");
    assert_eq!(list[0].size, 17);
    assert_eq!(list[0].origin, AttachmentOrigin::Document);
    assert_eq!(
        d.attachment_bytes(&AttachmentOrigin::Document, "nota (2).txt")
            .unwrap(),
        b"conteudo do anexo"
    );
    // salvo e reaberto, continua
    let out = dir.path().join("com-anexo.pdf");
    d.save(&out).unwrap();
    assert_eq!(PdfDoc::open(&out, None).unwrap().attachments().len(), 2);
}

#[test]
fn remove_and_description_undo() {
    let (dir, mut d) = doc();
    let src = file(&dir, "x.pdf", b"%PDF-1.4 fake");
    let name = d.add_attachment(&src, "antes").unwrap();
    d.set_attachment_description(&name, "depois").unwrap();
    assert_eq!(d.attachments()[0].description, "depois");
    d.undo().unwrap();
    assert_eq!(d.attachments()[0].description, "antes");
    d.remove_attachment(&name).unwrap();
    assert!(d.attachments().is_empty());
    d.undo().unwrap();
    assert_eq!(d.attachments().len(), 1);
    assert!(d.remove_attachment("nao-existe").is_err());
}

#[test]
fn lists_comment_attachments_with_their_page() {
    let dir = tempfile::tempdir().unwrap();
    let data = b"dados do comentario";
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        "<</Type/Annot/Subtype/FileAttachment/Rect[10 10 30 30]/FS 5 0 R/Contents(anexo do comentário)>>".to_string(),
        "<</Type/Filespec/F(planilha.csv)/UF(planilha.csv)/EF<</F 6 0 R>>>>".to_string(),
        format!("<</Type/EmbeddedFile/Length {}>>\nstream\n{}\nendstream", data.len(), String::from_utf8_lossy(data)),
    ];
    let path = dir.path().join("c.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let list = d.attachments();
    assert_eq!(list.len(), 1);
    let a = &list[0];
    assert_eq!(a.name, "planilha.csv");
    assert!(matches!(
        a.origin,
        AttachmentOrigin::Comment { page: 0, .. }
    ));
    assert_eq!(d.attachment_bytes(&a.origin, &a.name).unwrap(), data);
}

#[test]
fn too_big_is_refused() {
    let (dir, mut d) = doc();
    let big = dir.path().join("grande.bin");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(pdf_core::MAX_ATTACHMENT_BYTES + 1).unwrap();
    assert!(d.add_attachment(&big, "").is_err());
    assert!(!d.is_dirty());
}
