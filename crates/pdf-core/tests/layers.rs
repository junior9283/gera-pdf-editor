use pdf_core::{sample::pdf_from_objects, LayerNode, PdfDoc};

/// Página com um retângulo vermelho na camada A (5), a camada B (6) dentro de A e escondida, e a
/// camada C (7) num grupo "Grupo", travada.
fn layered(dir: &tempfile::TempDir) -> (std::path::PathBuf, PdfDoc) {
    let content = "/OC /a BDC 1 0 0 rg 100 100 200 200 re f EMC";
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/OCProperties<</OCGs[5 0 R 6 0 R 7 0 R]/D<</Order[5 0 R[6 0 R][(Grupo)7 0 R]]/OFF[6 0 R]/Locked[7 0 R]>>>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 400]/Contents 4 0 R/Resources<</Properties<</a 5 0 R>>>>>>".to_string(),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        "<</Type/OCG/Name(A)>>".to_string(),
        "<</Type/OCG/Name(B)>>".to_string(),
        "<</Type/OCG/Name(C)>>".to_string(),
    ];
    let path = dir.path().join("camadas.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    (path, d)
}

/// O centro do retângulo (200, 200 em pontos; y do PDF de baixo para cima) está vermelho?
fn red_shows(d: &PdfDoc) -> bool {
    let img = d.render(0, 1.0, false).unwrap();
    let i = ((img.height as usize / 2) * img.width as usize + img.width as usize / 2) * 3;
    img.rgb[i] > 200 && img.rgb[i + 1] < 80
}

/// (nome, visível, padrão, travada) na ordem da árvore, com os rótulos como "[rótulo]".
fn flat(nodes: &[LayerNode], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            LayerNode::Layer {
                name,
                visible,
                default_on,
                locked,
                kids,
                ..
            } => {
                out.push(format!("{name} {visible} {default_on} {locked}"));
                flat(kids, out);
            }
            LayerNode::Label { text, kids } => {
                out.push(format!("[{text}]"));
                flat(kids, out);
            }
        }
    }
}

fn layers(d: &PdfDoc) -> Vec<String> {
    let mut out = Vec::new();
    flat(&d.layers(), &mut out);
    out
}

fn id_of(d: &PdfDoc, wanted: &str) -> i32 {
    fn find(nodes: &[LayerNode], wanted: &str) -> Option<i32> {
        nodes.iter().find_map(|n| match n {
            LayerNode::Layer { id, name, kids, .. } => {
                if name == wanted {
                    Some(*id)
                } else {
                    find(kids, wanted)
                }
            }
            LayerNode::Label { kids, .. } => find(kids, wanted),
        })
    }
    find(&d.layers(), wanted).unwrap()
}

#[test]
fn nested_order_with_labels_and_locked() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, d) = layered(&dir);
    assert_eq!(
        layers(&d),
        [
            "A true true false",
            "B false false false",
            "[Grupo]",
            "C true true true"
        ]
    );
}

#[test]
fn eye_hides_content_without_dirtying() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = layered(&dir);
    assert!(red_shows(&d));
    let a = id_of(&d, "A");
    d.set_layer_view(a, false).unwrap();
    assert!(!red_shows(&d), "a camada A sumiu da página");
    assert!(
        layers(&d)[0].starts_with("A false true"),
        "{:?}",
        layers(&d)
    );
    assert!(!d.is_dirty(), "o olho é só vista");
    assert!(!d.can_undo());
    d.reset_layer_view().unwrap();
    assert!(red_shows(&d), "redefinir volta ao estado inicial");
}

#[test]
fn eye_state_never_reaches_the_saved_file() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = layered(&dir);
    let a = id_of(&d, "A");
    d.set_layer_view(a, false).unwrap();
    let out = dir.path().join("salvo.pdf");
    d.save(&out).unwrap();
    assert!(!red_shows(&d), "depois de salvar, a vista continua igual");
    let back = PdfDoc::open(&out, None).unwrap();
    assert!(red_shows(&back), "o arquivo guarda o estado inicial");
    assert_eq!(layers(&back)[0], "A true true false");
}

#[test]
fn eye_survives_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = layered(&dir);
    let a = id_of(&d, "A");
    d.set_layer_view(a, false).unwrap();
    let c = id_of(&d, "C");
    d.edit_layer(c, "Cotas", true).unwrap();
    assert!(!red_shows(&d));
    d.undo().unwrap();
    assert!(!red_shows(&d), "desfazer não reacende a camada");
    assert!(layers(&d).contains(&"C true true true".to_string()));
}

#[test]
fn locked_layer_does_not_change() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = layered(&dir);
    let c = id_of(&d, "C");
    assert!(d.set_layer_view(c, false).is_err());
}

#[test]
fn layer_properties_edit_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, mut d) = layered(&dir);
    let a = id_of(&d, "A");
    d.edit_layer(a, "Cotas", false).unwrap();
    assert!(d.is_dirty());
    assert_eq!(layers(&d)[0], "Cotas false false false");
    assert!(!red_shows(&d), "estado padrão desligado esconde");
    let out = dir.path().join("salvo.pdf");
    d.save(&out).unwrap();
    let mut back = PdfDoc::open(&out, None).unwrap();
    assert_eq!(layers(&back)[0], "Cotas false false false");
    back.edit_layer(a, "A", true).unwrap();
    back.undo().unwrap();
    assert_eq!(layers(&back)[0], "Cotas false false false");
}

#[test]
fn document_without_layers_lists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 400]>>".to_string(),
    ];
    let path = dir.path().join("simples.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    assert!(PdfDoc::open(&path, None).unwrap().layers().is_empty());
}

#[test]
fn an_order_that_points_to_itself_is_read_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let refs = "8 0 R ".repeat(100);
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R/OCProperties<</OCGs[5 0 R]/D<</Order 8 0 R>>>>>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 400 400]>>".to_string(),
        "<</Length 0>>\nstream\n\nendstream".to_string(),
        "<</Type/OCG/Name(A)>>".to_string(),
        "<<>>".to_string(),
        "<<>>".to_string(),
        format!("[5 0 R {refs}]"),
    ];
    let path = dir.path().join("laco.pdf");
    std::fs::write(&path, pdf_from_objects(&objs)).unwrap();
    let d = PdfDoc::open(&path, None).unwrap();
    let t = std::time::Instant::now();
    let list = d.layers();
    assert_eq!(list.len(), 1);
    assert!(t.elapsed().as_secs() < 2, "{:?}", t.elapsed());
}
