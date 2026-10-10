mod common;
use common::write_sample;
use pdf_core::{AnnotOp, NewAnnot, PdfDoc, StateModel};

fn doc_with_note() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc, i32) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let mut d = PdfDoc::open(&path, None).unwrap();
    d.set_author(Some("Ana".into()));
    d.apply(AnnotOp::Create(NewAnnot::Note {
        page: 0,
        at: (100.0, 100.0),
        text: "pai".into(),
    }))
    .unwrap();
    let id = d.annotations(0).unwrap()[0].id;
    (dir, path, d, id)
}

fn reply(id: i32, text: &str) -> AnnotOp {
    AnnotOp::Reply {
        page: 0,
        parent: id,
        text: text.into(),
    }
}

#[test]
fn replies_and_states_are_children_with_authors_and_dates() {
    let (_d, path, mut d, id) = doc_with_note();
    d.apply(reply(id, "concordo")).unwrap();
    d.apply(AnnotOp::SetState {
        page: 0,
        parent: id,
        model: StateModel::Review,
        state: "Accepted".into(),
    })
    .unwrap();
    d.save(&path).unwrap();
    let all = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert_eq!(all.len(), 3, "{all:?}");
    let root = all.iter().find(|a| a.id == id).unwrap();
    assert!(root.created.starts_with("D:20"), "{:?}", root.created);
    let r = all.iter().find(|a| a.contents == "concordo").unwrap();
    assert_eq!(r.reply_to, Some(id));
    assert_eq!(r.author, "Ana");
    assert!(r.created.starts_with("D:20") && r.modified.starts_with("D:20"));
    let s = all.iter().find(|a| a.state.is_some()).unwrap();
    assert_eq!(s.state, Some((StateModel::Review, "Accepted".into())));
    assert_eq!(s.reply_to, Some(id));
}

#[test]
fn replies_are_not_drawn_on_the_page() {
    let (_d, _p, mut d, id) = doc_with_note();
    let before = d.render(0, 1.0, false).unwrap().rgb;
    d.apply(reply(id, "x")).unwrap();
    assert!(d.render(0, 1.0, false).unwrap().rgb == before);
}

#[test]
fn deleting_a_comment_takes_its_replies_and_undo_brings_them_back() {
    let (_d, _p, mut d, id) = doc_with_note();
    d.apply(reply(id, "r1")).unwrap();
    d.apply(reply(id, "r2")).unwrap();
    let before = d.annotations(0).unwrap();
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap(), before);
}

#[test]
fn editing_text_updates_the_modification_date_and_undo_restores_it() {
    let (_d, _p, mut d, id) = doc_with_note();
    let before = d.annotations(0).unwrap()[0].clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    d.apply(AnnotOp::SetContents {
        page: 0,
        id,
        text: "novo".into(),
    })
    .unwrap();
    assert_ne!(d.annotations(0).unwrap()[0].modified, before.modified);
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], before);
}

/// Flags /F das anotações com /IRT (filhas), lidas direto do arquivo.
fn child_flags(path: &std::path::Path) -> Vec<mupdf::pdf::AnnotationFlags> {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let page = doc.load_pdf_page(0).unwrap();
    page.annotations()
        .filter(|a| a.object().get_dict("IRT").unwrap().is_some())
        .map(|a| a.flags().unwrap())
        .collect()
}

#[test]
fn replies_and_states_never_print() {
    let (_d, path, mut d, id) = doc_with_note();
    d.apply(reply(id, "x")).unwrap();
    d.apply(AnnotOp::SetState {
        page: 0,
        parent: id,
        model: StateModel::Review,
        state: "Accepted".into(),
    })
    .unwrap();
    d.save(&path).unwrap();
    let flags = child_flags(&path);
    assert_eq!(flags.len(), 2);
    assert!(flags
        .iter()
        .all(|f| !f.contains(mupdf::pdf::AnnotationFlags::IS_PRINT)));
}

/// Põe a flag Hidden (como o Acrobat grava os status) nas filhas do arquivo.
fn hide_children_like_acrobat(path: &std::path::Path) {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let mut page = doc.load_pdf_page(0).unwrap();
    let all: Vec<_> = page.annotations().collect();
    for mut a in all {
        if a.object().get_dict("IRT").unwrap().is_some() {
            let f = (a.flags().unwrap() | mupdf::pdf::AnnotationFlags::IS_HIDDEN)
                - mupdf::pdf::AnnotationFlags::NO_VIEW;
            a.set_flags(f).unwrap();
            a.update().unwrap();
        }
    }
    page.update().unwrap();
    doc.save(path.to_str().unwrap()).unwrap();
}

#[test]
fn hidden_states_from_acrobat_are_listed_and_survive_delete_undo() {
    let (_d, path, mut d, id) = doc_with_note();
    d.apply(AnnotOp::SetState {
        page: 0,
        parent: id,
        model: StateModel::Review,
        state: "Accepted".into(),
    })
    .unwrap();
    d.save(&path).unwrap();
    hide_children_like_acrobat(&path);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let all = d.annotations(0).unwrap();
    assert_eq!(all.len(), 2, "o status Hidden do Acrobat aparece no painel");
    let page_before = d.render(0, 1.0, false).unwrap().rgb;
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap(), all);
    assert!(
        d.render(0, 1.0, false).unwrap().rgb == page_before,
        "o status não pode aparecer na página depois do desfazer"
    );
}

#[test]
fn deleting_the_root_takes_replies_of_replies() {
    let (_d, path, mut d, id) = doc_with_note();
    d.apply(reply(id, "filha")).unwrap();
    let child = d
        .annotations(0)
        .unwrap()
        .into_iter()
        .find(|a| a.contents == "filha")
        .unwrap()
        .id;
    d.apply(reply(child, "neta")).unwrap();
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.save(&path).unwrap();
    assert_eq!(
        common::count_raw_annotations(&path),
        0,
        "nada pendurado no arquivo"
    );
}

/// Grava uma resposta como outro programa: só /IRT e /RT /R, sem NoView.
fn plain_reply_in_file(path: &std::path::Path) {
    let doc = mupdf::pdf::PdfDocument::open(path.to_str().unwrap()).unwrap();
    let mut page = doc.load_pdf_page(0).unwrap();
    let parent = page.annotations().next().unwrap();
    let mut r = page
        .add_text_annotation(mupdf::Rect::new(300.0, 300.0, 320.0, 320.0), "de fora")
        .unwrap();
    r.object().dict_put("IRT", parent.object()).unwrap();
    r.object()
        .dict_put("RT", doc.new_name("R").unwrap())
        .unwrap();
    r.update().unwrap();
    page.update().unwrap();
    doc.save(path.to_str().unwrap()).unwrap();
}

#[test]
fn replies_from_other_programs_are_not_drawn() {
    let (_d, path, mut d, _id) = doc_with_note();
    d.save(&path).unwrap();
    let clean = PdfDoc::open(&path, None)
        .unwrap()
        .render(0, 1.0, false)
        .unwrap()
        .rgb;
    plain_reply_in_file(&path);
    let d = PdfDoc::open(&path, None).unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 2);
    assert!(
        d.render(0, 1.0, false).unwrap().rgb == clean,
        "sem ícone fantasma"
    );
}
