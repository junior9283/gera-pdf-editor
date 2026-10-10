mod common;
use common::write_sample;
use pdf_core::{AnnotKind, AnnotOp, NewAnnot, PdfDoc, PdfRect};

fn doc() -> (tempfile::TempDir, std::path::PathBuf, PdfDoc) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "a.pdf", 1, 0);
    let d = PdfDoc::open(&path, None).unwrap();
    (dir, path, d)
}

fn reopen_only(path: &std::path::Path) -> pdf_core::AnnotInfo {
    let mut v = PdfDoc::open(path, None).unwrap().annotations(0).unwrap();
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

fn shape(ellipse: bool) -> NewAnnot {
    NewAnnot::Shape {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 200.0, 150.0),
        ellipse,
    }
}

fn line() -> NewAnnot {
    NewAnnot::Line {
        page: 0,
        a: (100.0, 100.0),
        b: (300.0, 200.0),
        arrow: true,
    }
}

#[test]
fn shapes_and_lines_are_created_and_read_back() {
    for (annot, kind) in [
        (shape(false), AnnotKind::Rect),
        (shape(true), AnnotKind::Ellipse),
        (line(), AnnotKind::Line),
    ] {
        let (_d, path, mut d) = doc();
        d.set_author(Some("Maria Silva".into()));
        d.apply(AnnotOp::Create(annot)).unwrap();
        d.save(&path).unwrap();
        let a = reopen_only(&path);
        assert_eq!(a.kind, kind);
        assert_eq!(a.author, "Maria Silva");
        assert_eq!(a.color, Some([1.0, 0.0, 0.0]), "vermelho padrão");
        assert_eq!(a.width, Some(1.0));
        assert_eq!(a.fill, None);
        if kind == AnnotKind::Line {
            assert_eq!(a.line, Some(((100.0, 100.0), (300.0, 200.0))));
        }
    }
}

#[test]
fn text_box_with_accents_and_lines_round_trips() {
    let (_d, path, mut d) = doc();
    d.apply(AnnotOp::Create(NewAnnot::TextBox {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
        text: "Atenção: revisão\nsegunda linha".into(),
        size: 12.0,
        color: [0.0, 0.0, 0.0],
    }))
    .unwrap();
    d.save(&path).unwrap();
    let a = reopen_only(&path);
    assert_eq!(a.kind, AnnotKind::TextBox);
    assert_eq!(a.contents, "Atenção: revisão\nsegunda linha");
    assert_eq!(a.font, Some((12.0, [0.0, 0.0, 0.0])));
    assert_eq!(
        a.color,
        Some([0.0, 0.0, 0.0]),
        "a borda segue a cor do texto"
    );
    assert_eq!(a.fill, None, "sem fundo");
    assert_eq!(a.width, Some(1.0));
}

#[test]
fn empty_text_boxes_and_degenerate_shapes_are_refused() {
    let (_d, _p, mut d) = doc();
    for a in [
        NewAnnot::TextBox {
            page: 0,
            rect: PdfRect::new(1.0, 1.0, 50.0, 20.0),
            text: " ".into(),
            size: 12.0,
            color: [0.0; 3],
        },
        NewAnnot::Shape {
            page: 0,
            rect: PdfRect::new(5.0, 5.0, 6.0, 6.0),
            ellipse: false,
        },
        NewAnnot::Line {
            page: 0,
            a: (5.0, 5.0),
            b: (6.0, 6.0),
            arrow: false,
        },
    ] {
        assert!(d.apply(AnnotOp::Create(a)).is_err());
    }
    assert!(d.annotations(0).unwrap().is_empty());
}

fn created(d: &mut PdfDoc, a: NewAnnot) -> pdf_core::AnnotInfo {
    d.apply(AnnotOp::Create(a)).unwrap();
    d.annotations(0).unwrap().pop().unwrap()
}

#[test]
fn line_points_move_and_undo_exactly() {
    let (_d, _p, mut d) = doc();
    let l = created(&mut d, line());
    d.apply(AnnotOp::MoveLinePoint {
        page: 0,
        id: l.id,
        end: pdf_core::LineEnd::End,
        to: (400.0, 120.0),
    })
    .unwrap();
    assert_eq!(
        d.annotations(0).unwrap()[0].line,
        Some(((100.0, 100.0), (400.0, 120.0)))
    );
    d.apply(AnnotOp::Move {
        page: 0,
        id: l.id,
        dx: 10.0,
        dy: 5.0,
    })
    .unwrap();
    assert_eq!(
        d.annotations(0).unwrap()[0].line,
        Some(((110.0, 105.0), (410.0, 125.0)))
    );
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], l, "volta idêntica");
}

#[test]
fn undoing_fill_on_a_shape_without_fill_removes_it() {
    let (_d, _p, mut d) = doc();
    let s = created(&mut d, shape(false));
    d.apply(AnnotOp::Fill {
        page: 0,
        id: s.id,
        color: Some([0.1, 0.45, 0.95]),
    })
    .unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].fill, Some([0.1, 0.45, 0.95]));
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].fill, None);
}

#[test]
fn text_box_font_fill_and_resize_undo_exactly() {
    let (_d, _p, mut d) = doc();
    let t = created(
        &mut d,
        NewAnnot::TextBox {
            page: 0,
            rect: PdfRect::new(100.0, 100.0, 300.0, 160.0),
            text: "oi".into(),
            size: 12.0,
            color: [0.0; 3],
        },
    );
    d.apply(AnnotOp::Font {
        page: 0,
        id: t.id,
        size: 20.0,
        color: [0.9, 0.1, 0.1],
    })
    .unwrap();
    d.apply(AnnotOp::Fill {
        page: 0,
        id: t.id,
        color: Some([1.0, 0.9, 0.0]),
    })
    .unwrap();
    d.apply(AnnotOp::Resize {
        page: 0,
        id: t.id,
        rect: PdfRect::new(100.0, 100.0, 400.0, 200.0),
    })
    .unwrap();
    let now = d.annotations(0).unwrap()[0].clone();
    assert_eq!(now.font, Some((20.0, [0.9, 0.1, 0.1])));
    assert_eq!(now.fill, Some([1.0, 0.9, 0.0]));
    assert!((now.rect.x1 - 400.0).abs() < 1.0);
    for _ in 0..3 {
        d.undo().unwrap();
    }
    assert_eq!(d.annotations(0).unwrap()[0], t);
}

#[test]
fn shapes_resize_and_move() {
    let (_d, _p, mut d) = doc();
    let s = created(&mut d, shape(true));
    d.apply(AnnotOp::Resize {
        page: 0,
        id: s.id,
        rect: PdfRect::new(50.0, 60.0, 250.0, 260.0),
    })
    .unwrap();
    let r = d.annotations(0).unwrap()[0].rect;
    assert!(
        (r.x0 - 50.0).abs() < 1.5 && (r.y1 - 260.0).abs() < 1.5,
        "{r:?}"
    );
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], s);
}

#[test]
fn undoing_a_width_change_on_a_shape_restores_it() {
    let (_d, _p, mut d) = doc();
    let s = created(&mut d, shape(false));
    d.apply(AnnotOp::Style {
        page: 0,
        id: s.id,
        color: Some([0.1, 0.45, 0.95]),
        opacity: 0.5,
        width: Some(6.0),
    })
    .unwrap();
    assert_eq!(d.annotations(0).unwrap()[0].width, Some(6.0));
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], s);
}

fn stamp(sub: Option<&str>) -> NewAnnot {
    NewAnnot::Stamp {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 290.0, 160.0),
        title: "NÃO APROVADO".into(),
        subtitle: sub.map(str::to_owned),
        color: [0.8, 0.1, 0.1],
    }
}

/// Há pixels avermelhados dentro do retângulo (em pontos) da página renderizada a 1:1?
fn has_red_inside(d: &PdfDoc, r: PdfRect) -> bool {
    let img = d.render(0, 1.0, false).unwrap();
    let w = img.width as usize;
    (r.y0.max(0.0) as usize..r.y1 as usize).any(|y| {
        (r.x0.max(0.0) as usize..r.x1 as usize).any(|x| {
            let p = &img.rgb[(y * w + x) * 3..][..3];
            p[0] > 150 && p[1] < 90 && p[2] < 90
        })
    })
}

#[test]
fn stamps_are_drawn_and_survive_move_and_save() {
    let (_d, path, mut d) = doc();
    let s = created(&mut d, stamp(Some("Maria Silva, 09/10/2026 14:30")));
    assert_eq!(s.kind, AnnotKind::Stamp);
    assert!(has_red_inside(&d, s.rect));
    d.apply(AnnotOp::Move {
        page: 0,
        id: s.id,
        dx: 100.0,
        dy: 200.0,
    })
    .unwrap();
    d.save(&path).unwrap();
    let again = PdfDoc::open(&path, None).unwrap();
    let a = again.annotations(0).unwrap().pop().unwrap();
    assert!(a.rect.y0 > 250.0, "{:?}", a.rect);
    assert!(has_red_inside(&again, a.rect));
}

#[test]
fn stamp_survives_move_and_save_on_a_rotated_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(&dir, "g.pdf", 1, 90);
    let mut d = PdfDoc::open(&path, None).unwrap();
    let s = created(&mut d, stamp(None));
    d.apply(AnnotOp::Move {
        page: 0,
        id: s.id,
        dx: 20.0,
        dy: 20.0,
    })
    .unwrap();
    d.save(&path).unwrap();
    assert_eq!(
        PdfDoc::open(&path, None)
            .unwrap()
            .annotations(0)
            .unwrap()
            .len(),
        1
    );
}

/// Há pixels azuis dentro do retângulo (em pontos) da página renderizada a 1:1?
fn has_blue_inside(d: &PdfDoc, r: PdfRect) -> bool {
    let img = d.render(0, 1.0, false).unwrap();
    let w = img.width as usize;
    (r.y0 as usize + 3..r.y1 as usize - 3).any(|y| {
        (r.x0 as usize + 3..r.x1 as usize - 3).any(|x| {
            let p = &img.rgb[(y * w + x) * 3..][..3];
            p[2] > 150 && p[0] < 90
        })
    })
}

#[test]
fn removing_a_fill_redraws_the_shape_without_it() {
    let (_d, _p, mut d) = doc();
    let s = created(&mut d, shape(false));
    d.apply(AnnotOp::Fill {
        page: 0,
        id: s.id,
        color: Some([0.1, 0.45, 0.95]),
    })
    .unwrap();
    assert!(has_blue_inside(&d, s.rect));
    d.undo().unwrap();
    assert!(
        !has_blue_inside(&d, s.rect),
        "o desfazer tem que apagar o azul da tela"
    );
}

/// Máscara (vermelho sim/não) da área `r` da página 0 renderizada a 1:1.
fn red_mask(d: &PdfDoc, r: PdfRect) -> Vec<bool> {
    let img = d.render(0, 1.0, false).unwrap();
    let w = img.width as usize;
    let mut out = Vec::new();
    for y in r.y0 as usize..r.y1 as usize {
        for x in r.x0 as usize..r.x1 as usize {
            let p = &img.rgb[(y * w + x) * 3..][..3];
            out.push(p[0] > 150 && p[1] < 90 && p[2] < 90);
        }
    }
    out
}

#[test]
fn stamp_on_a_rotated_page_looks_the_same_as_on_a_normal_page() {
    let dir = tempfile::tempdir().unwrap();
    let render = |rotate| {
        let path = write_sample(&dir, &format!("p{rotate}.pdf"), 1, rotate);
        let mut d = PdfDoc::open(&path, None).unwrap();
        let s = created(&mut d, stamp(Some("Maria Silva, 09/10/2026 14:30")));
        red_mask(&d, s.rect)
    };
    let flat = render(0);
    for rotate in [90, 180, 270] {
        let turned = render(rotate);
        assert_eq!(flat.len(), turned.len());
        let same = flat.iter().zip(&turned).filter(|(a, b)| a == b).count();
        assert!(
            same as f32 / flat.len() as f32 > 0.88, // deitado dá ~0,76; em pé, ≥ 0,92
            "carimbo em página girada {rotate}° tem que ficar em pé: {same}/{}",
            flat.len()
        );
    }
}

fn small_box(text: &str) -> NewAnnot {
    NewAnnot::TextBox {
        page: 0,
        rect: PdfRect::new(100.0, 100.0, 300.0, 124.0),
        text: text.into(),
        size: 12.0,
        color: [1.0, 0.0, 0.0],
    }
}

#[test]
fn text_boxes_grow_to_fit_their_text() {
    let (_d, _p, mut d) = doc();
    let t = created(&mut d, small_box("um\ndois\ntrês\nquatro"));
    assert!(t.rect.height() >= 4.0 * 12.0 * 1.2, "{:?}", t.rect);
    let (_d, _p, mut d) = doc();
    let long = "palavras que não cabem numa linha só da caixa de texto estreita";
    let t = created(&mut d, small_box(long));
    assert!(
        t.rect.height() >= 2.0 * 12.0 * 1.2,
        "quebra automática: {:?}",
        t.rect
    );
}

#[test]
fn editing_text_grows_the_box_and_undo_restores_both() {
    let (_d, _p, mut d) = doc();
    let t = created(&mut d, small_box("curto"));
    d.apply(AnnotOp::SetContents {
        page: 0,
        id: t.id,
        text: "primeira\nsegunda\nterceira".into(),
    })
    .unwrap();
    let grown = d.annotations(0).unwrap()[0].clone();
    assert!(grown.rect.height() >= 3.0 * 12.0 * 1.2, "{:?}", grown.rect);
    d.undo().unwrap();
    assert_eq!(
        d.annotations(0).unwrap()[0],
        t,
        "um Ctrl+Z desfaz texto e tamanho"
    );
    d.redo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], grown);
}

#[test]
fn bigger_font_grows_the_box() {
    let (_d, _p, mut d) = doc();
    let t = created(&mut d, small_box("um\ndois"));
    d.apply(AnnotOp::Font {
        page: 0,
        id: t.id,
        size: 24.0,
        color: [1.0, 0.0, 0.0],
    })
    .unwrap();
    let r = d.annotations(0).unwrap()[0].rect;
    assert!(r.height() >= 2.0 * 24.0 * 1.2, "{r:?}");
    d.undo().unwrap();
    assert_eq!(d.annotations(0).unwrap()[0], t);
}
