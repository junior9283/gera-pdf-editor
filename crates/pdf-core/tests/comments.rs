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

fn blank() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

#[test]
fn caret_with_note_round_trip() {
    let (_d, path, mut d) = blank();
    d.apply(AnnotOp::Create(NewAnnot::Caret {
        page: 0,
        rect: pdf_core::PdfRect::new(100.0, 100.0, 110.0, 112.0),
        text: "faltou uma vírgula".into(),
    }))
    .unwrap();
    d.save(&path).unwrap();
    let all = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, pdf_core::AnnotKind::Caret);
    assert_eq!(all[0].contents, "faltou uma vírgula");
}

#[test]
fn replace_text_is_one_undo_step_and_deletes_as_a_pair() {
    let (_d, path, mut d) = blank();
    d.apply(AnnotOp::Create(NewAnnot::ReplaceText {
        page: 0,
        rects: vec![pdf_core::PdfRect::new(100.0, 100.0, 160.0, 112.0)],
        text: "novo texto".into(),
    }))
    .unwrap();
    let all = d.annotations(0).unwrap();
    assert_eq!(all.len(), 2, "riscado e cursor");
    let caret = all
        .iter()
        .find(|a| a.kind == pdf_core::AnnotKind::Caret)
        .unwrap();
    let strike = all
        .iter()
        .find(|a| a.kind == pdf_core::AnnotKind::StrikeOut)
        .unwrap();
    assert_eq!(caret.contents, "novo texto");
    assert_eq!(
        strike.group,
        Some(caret.id),
        "o riscado é do grupo do cursor"
    );
    assert_eq!(strike.reply_to, None, "grupo não é resposta");
    // excluir o riscado exclui o par
    d.apply(AnnotOp::Delete {
        page: 0,
        id: strike.id,
    })
    .unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 2, "desfazer volta os dois");
    d.undo().unwrap();
    assert!(
        d.annotations(0).unwrap().is_empty(),
        "criar foi uma etapa só"
    );
    d.redo().unwrap();
    d.save(&path).unwrap();
    let back = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert!(back.iter().any(|a| a.group.is_some()));
}

#[test]
fn callout_round_trip_and_move_tip() {
    let (_d, path, mut d) = blank();
    let rect = pdf_core::PdfRect::new(200.0, 100.0, 320.0, 140.0);
    d.apply(AnnotOp::Create(NewAnnot::Callout {
        page: 0,
        rect,
        knee: (150.0, 160.0),
        tip: (100.0, 200.0),
        text: "veja aqui".into(),
        size: 12.0,
        color: [0.0; 3],
    }))
    .unwrap();
    let a = d.annotations(0).unwrap().remove(0);
    assert_eq!(a.kind, pdf_core::AnnotKind::Callout);
    let [tip, knee, _end] = a.callout.unwrap();
    assert!(
        (tip.0 - 100.0).abs() < 0.5 && (tip.1 - 200.0).abs() < 0.5,
        "{tip:?}"
    );
    assert!((knee.0 - 150.0).abs() < 0.5, "{knee:?}");
    d.apply(AnnotOp::MoveCallout {
        page: 0,
        id: a.id,
        knee: (160.0, 170.0),
        tip: (90.0, 250.0),
    })
    .unwrap();
    d.save(&path).unwrap();
    let back = PdfDoc::open(&path, None)
        .unwrap()
        .annotations(0)
        .unwrap()
        .remove(0);
    let [tip, _, _] = back.callout.unwrap();
    assert!(
        (tip.0 - 90.0).abs() < 0.5 && (tip.1 - 250.0).abs() < 0.5,
        "{tip:?}"
    );
    assert_eq!(back.contents, "veja aqui");
}

#[test]
fn editing_a_callout_text_keeps_box_and_line() {
    let (_d, _p, mut d) = blank();
    let rect = pdf_core::PdfRect::new(200.0, 100.0, 320.0, 140.0);
    d.apply(AnnotOp::Create(NewAnnot::Callout {
        page: 0,
        rect,
        knee: (150.0, 160.0),
        tip: (100.0, 200.0),
        text: "veja".into(),
        size: 12.0,
        color: [0.0; 3],
    }))
    .unwrap();
    let a = d.annotations(0).unwrap().remove(0);
    d.apply(AnnotOp::SetContents {
        page: 0,
        id: a.id,
        text: "veja aqui também".into(),
    })
    .unwrap();
    let b = d.annotations(0).unwrap().remove(0);
    assert_eq!(b.kind, pdf_core::AnnotKind::Callout);
    assert_eq!(
        b.callout.unwrap()[0],
        a.callout.unwrap()[0],
        "a ponta não sai do lugar"
    );
    assert!(
        (b.rect.x0 - 200.0).abs() < 0.5 && (b.rect.y0 - 100.0).abs() < 0.5,
        "{:?}",
        b.rect
    );
}

#[test]
fn resizing_a_callout_box_keeps_the_tip_and_undoes() {
    let (_d, _p, mut d) = blank();
    d.apply(AnnotOp::Create(NewAnnot::Callout {
        page: 0,
        rect: pdf_core::PdfRect::new(200.0, 100.0, 320.0, 140.0),
        knee: (150.0, 160.0),
        tip: (100.0, 200.0),
        text: "veja".into(),
        size: 12.0,
        color: [0.0; 3],
    }))
    .unwrap();
    let a = d.annotations(0).unwrap().remove(0);
    assert!(a.resizable(), "a caixa do texto explicativo redimensiona");
    d.apply(AnnotOp::Resize {
        page: 0,
        id: a.id,
        rect: pdf_core::PdfRect::new(200.0, 100.0, 400.0, 180.0),
    })
    .unwrap();
    let b = d.annotations(0).unwrap().remove(0);
    assert!((b.rect.x1 - 400.0).abs() < 0.5, "{:?}", b.rect);
    assert_eq!(b.callout.unwrap()[0], a.callout.unwrap()[0], "ponta parada");
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].rect, a.rect);
}

#[test]
fn a_group_pointing_outside_the_page_annotations_still_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R]>>".to_string(),
        "<</Type/Annot/Subtype/StrikeOut/Rect[100 100 200 120]/QuadPoints[100 120 200 120 100 100 200 100]/RT/Group/IRT 3 0 R>>".to_string(),
    ];
    let path = dir.path().join("g.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let mut d = PdfDoc::open(&path, None).unwrap();
    let id = d.annotations(0).unwrap()[0].id;
    d.apply(AnnotOp::Delete { page: 0, id }).unwrap();
    assert!(d.annotations(0).unwrap().is_empty());
}

fn three_notes(d: &mut PdfDoc) -> Vec<i32> {
    for x in [100.0, 200.0, 300.0] {
        d.apply(AnnotOp::Create(NewAnnot::Note {
            page: 0,
            at: (x, 100.0),
            text: format!("nota {x}"),
        }))
        .unwrap();
    }
    d.annotations(0).unwrap().iter().map(|a| a.id).collect()
}

fn rect_of(d: &PdfDoc, id: i32) -> pdf_core::PdfRect {
    d.annotations(0)
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap()
        .rect
}

#[test]
fn grouping_moves_and_deletes_together_and_undoes() {
    let (_d, path, mut d) = blank();
    let ids = three_notes(&mut d);
    d.apply(AnnotOp::Group {
        page: 0,
        ids: vec![ids[0], ids[1]],
    })
    .unwrap();
    let all = d.annotations(0).unwrap();
    let second = all.iter().find(|a| a.id == ids[1]).unwrap();
    assert_eq!(second.group, Some(ids[0]), "o primeiro é o principal");
    let before = (rect_of(&d, ids[0]), rect_of(&d, ids[2]));
    // mover um membro move o grupo
    d.apply(AnnotOp::Move {
        page: 0,
        id: ids[1],
        dx: 10.0,
        dy: 0.0,
    })
    .unwrap();
    assert!((rect_of(&d, ids[0]).x0 - before.0.x0 - 10.0).abs() < 0.5);
    assert_eq!(rect_of(&d, ids[2]), before.1, "quem está fora não se move");
    d.undo().unwrap();
    assert_eq!(rect_of(&d, ids[0]), before.0, "uma etapa");
    d.apply(AnnotOp::Delete {
        page: 0,
        id: ids[1],
    })
    .unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 1, "o grupo sai junto");
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 3);
    d.undo().unwrap();
    assert!(d.annotations(0).unwrap().iter().all(|a| a.group.is_none()));
    d.redo().unwrap();
    d.save(&path).unwrap();
    let back = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert_eq!(back.iter().filter(|a| a.group.is_some()).count(), 1);
}

#[test]
fn ungroup_splits_and_undoes() {
    let (_d, _p, mut d) = blank();
    let ids = three_notes(&mut d);
    d.apply(AnnotOp::Group {
        page: 0,
        ids: ids.clone(),
    })
    .unwrap();
    d.apply(AnnotOp::Ungroup {
        page: 0,
        id: ids[2],
    })
    .unwrap();
    assert!(d.annotations(0).unwrap().iter().all(|a| a.group.is_none()));
    d.undo().unwrap();
    assert_eq!(
        d.annotations(0)
            .unwrap()
            .iter()
            .filter(|a| a.group == Some(ids[0]))
            .count(),
        2
    );
}

#[test]
fn replies_and_single_items_are_not_grouped() {
    let (_d, _p, mut d, id) = doc_with_note();
    d.apply(reply(id, "resposta")).unwrap();
    let r = d
        .annotations(0)
        .unwrap()
        .into_iter()
        .find(|a| a.reply_to.is_some())
        .unwrap();
    assert!(d
        .apply(AnnotOp::Group {
            page: 0,
            ids: vec![id, r.id],
        })
        .is_err());
    assert!(d
        .apply(AnnotOp::Group {
            page: 0,
            ids: vec![id],
        })
        .is_err());
}

#[test]
fn delete_many_is_one_step() {
    let (_d, _p, mut d) = blank();
    let ids = three_notes(&mut d);
    d.apply(AnnotOp::DeleteMany {
        page: 0,
        ids: vec![ids[0], ids[2]],
    })
    .unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 1);
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap().len(), 3);
}

#[test]
fn self_groups_and_group_cycles_from_other_programs_are_not_groups() {
    let dir = tempfile::tempdir().unwrap();
    let objs = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Annots[4 0 R 5 0 R 6 0 R]>>".to_string(),
        "<</Type/Annot/Subtype/Text/Rect[10 10 30 30]/RT/Group/IRT 4 0 R>>".to_string(),
        "<</Type/Annot/Subtype/Text/Rect[40 10 60 30]/RT/Group/IRT 6 0 R>>".to_string(),
        "<</Type/Annot/Subtype/Text/Rect[70 10 90 30]/RT/Group/IRT 5 0 R>>".to_string(),
    ];
    let path = dir.path().join("g.pdf");
    std::fs::write(&path, pdf_core::sample::pdf_from_objects(&objs)).unwrap();
    let all = PdfDoc::open(&path, None).unwrap().annotations(0).unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|a| a.group.is_none()), "{all:?}");
}
