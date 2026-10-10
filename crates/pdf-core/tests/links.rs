use pdf_core::{LinkTarget, PdfDoc};
use std::path::{Path, PathBuf};

/// Escreve um PDF a partir dos corpos dos objetos (o objeto `i` é `objs[i - 1]`), com xref.
fn raw_pdf(dir: &Path, name: &str, objs: &[String]) -> PathBuf {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    let path = dir.join(name);
    std::fs::write(&path, out).unwrap();
    path
}

/// Duas páginas 612×792; a primeira tem um link por ação de `actions` (cada uma, uma faixa de
/// 20 pt de altura, de cima para baixo). `catalog_extra` entra no catálogo.
fn links_pdf(dir: &Path, actions: &[&str], catalog_extra: &str) -> PathBuf {
    // 1 catálogo, 2 páginas, 3 e 4 páginas, 5 árvore de nomes, 6.. links
    let annots: Vec<String> = (0..actions.len()).map(|i| format!("{} 0 R", 6 + i)).collect();
    let mut objs = vec![
        format!("<</Type/Catalog/Pages 2 0 R/Names<</Dests 5 0 R>>{catalog_extra}>>"),
        "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_string(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[{}]>>",
            annots.join(" ")
        ),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>".to_string(),
        "<</Names[(cap1)[4 0 R/Fit]]>>".to_string(),
    ];
    for (i, a) in actions.iter().enumerate() {
        let top = 780 - 30 * i as i32;
        objs.push(format!(
            "<</Type/Annot/Subtype/Link/Rect[10 {} 110 {top}]/Border[0 0 0]{a}>>",
            top - 20
        ));
    }
    raw_pdf(dir, "links.pdf", &objs)
}

fn targets(path: &Path) -> Vec<LinkTarget> {
    let doc = PdfDoc::open(path, None).unwrap();
    doc.links(0).unwrap().into_iter().map(|l| l.target).collect()
}

#[test]
fn link_to_page_resolves_point() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(dir.path(), &["/Dest[4 0 R/XYZ 100 700 0]"], "");
    let doc = PdfDoc::open(&path, None).unwrap();
    let links = doc.links(0).unwrap();
    assert_eq!(links.len(), 1);
    // retângulo em coordenadas do MuPDF (y para baixo)
    let r = links[0].rect;
    assert!((r.x0 - 10.0).abs() < 0.5 && (r.y0 - 12.0).abs() < 0.5, "{r:?}");
    match &links[0].target {
        LinkTarget::Page { page, at: Some((x, y)) } => {
            assert_eq!(*page, 1);
            assert!((x - 100.0).abs() < 0.5 && (y - 92.0).abs() < 0.5, "({x}, {y})");
        }
        t => panic!("{t:?}"),
    }
}

#[test]
fn named_destination_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/Dest(cap1)", "/A<</S/GoTo/D(cap1)>>", "/A<</S/GoTo/D[4 0 R/Fit]>>"],
        "",
    );
    for t in targets(&path) {
        assert!(matches!(t, LinkTarget::Page { page: 1, .. }), "{t:?}");
    }
}

#[test]
fn web_link_is_uri_and_javascript_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/A<</S/URI/URI(https://example.com/a?b=1)>>",
            "/A<</S/URI/URI(mailto:alguem@example.com)>>",
            "/A<</S/URI/URI(javascript:app.alert\\(1\\))>>",
            "/A<</S/URI/URI(smb://servidor/x)>>",
        ],
        "",
    );
    let t = targets(&path);
    assert_eq!(t.len(), 4, "{t:?}");
    assert_eq!(t[0], LinkTarget::Uri("https://example.com/a?b=1".into()));
    assert_eq!(t[1], LinkTarget::Uri("mailto:alguem@example.com".into()));
    assert!(matches!(t[2], LinkTarget::Blocked(_)), "{:?}", t[2]);
    assert!(matches!(t[3], LinkTarget::Blocked(_)), "{:?}", t[3]);
}

#[test]
fn remote_pdf_link_is_absolute_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &[
            "/A<</S/GoToR/F(outro.pdf)/D[1/Fit]>>",
            "/A<</S/Launch/F<</Type/Filespec/F(sub/doc.pdf)>>>>",
        ],
        "",
    );
    let t = targets(&path);
    assert_eq!(
        t[0],
        LinkTarget::File {
            path: dir.path().join("outro.pdf"),
            page: Some(1)
        }
    );
    assert_eq!(
        t[1],
        LinkTarget::File {
            path: dir.path().join("sub/doc.pdf"),
            page: Some(0)
        }
    );
}

#[test]
fn launch_of_program_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/A<</S/Launch/F(calc.exe)>>", "/A<</S/Launch/Win<</F(cmd.exe)>>>>"],
        "",
    );
    for t in targets(&path) {
        assert!(matches!(t, LinkTarget::Blocked(_)), "{t:?}");
    }
}

#[test]
fn link_to_missing_page_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/Dest(naoexiste)", "/Dest[7/Fit]"],
        "",
    );
    assert_eq!(targets(&path), vec![]);
}

#[test]
fn malformed_link_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/A<<>>", "/A 5", "/Rect 3", "/Dest[4 0 R/Fit]"],
        "",
    );
    let t = targets(&path);
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(matches!(t[0], LinkTarget::Page { page: 1, .. }));
}

#[test]
fn summary_carries_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = links_pdf(
        dir.path(),
        &["/Dest[4 0 R/Fit]", "/A<</S/URI/URI(https://a.b)>>"],
        "",
    );
    let s = PdfDoc::open(&path, None).unwrap().summary().unwrap();
    assert_eq!(s.links.len(), 2);
    assert!(s.links.iter().all(|l| l.page == 0));
}

