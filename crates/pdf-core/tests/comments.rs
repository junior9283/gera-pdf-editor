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
    assert_eq!(d.render(0, 1.0, false).unwrap().rgb, before);
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
