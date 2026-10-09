use pdf_core::NewAnnot;

/// Anotações criadas na sessão e ainda não gravadas no PDF, com desfazer/refazer.
#[derive(Debug, Default)]
pub struct PendingAnnots {
    items: Vec<NewAnnot>,
    undone: Vec<NewAnnot>,
}

impl PendingAnnots {
    /// Guarda a anotação; devolve `false` (e ignora) se ela não tem conteúdo desenhável.
    pub fn add(&mut self, a: NewAnnot) -> bool {
        if a.is_empty() {
            return false;
        }
        self.items.push(a);
        self.undone.clear();
        true
    }

    pub fn undo(&mut self) -> bool {
        shift(&mut self.items, &mut self.undone)
    }

    pub fn redo(&mut self) -> bool {
        shift(&mut self.undone, &mut self.items)
    }

    pub fn items(&self) -> &[NewAnnot] {
        &self.items
    }

    pub fn is_dirty(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.undone.clear();
    }
}

/// Move o último item de `from` para `to`; `false` se `from` está vazia.
fn shift(from: &mut Vec<NewAnnot>, to: &mut Vec<NewAnnot>) -> bool {
    from.pop().map(|a| to.push(a)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::PdfRect;

    fn hl(page: i32) -> NewAnnot {
        NewAnnot::Highlight {
            page,
            rects: vec![PdfRect::new(0.0, 0.0, 10.0, 10.0)],
        }
    }

    #[test]
    fn add_undo_redo() {
        let mut p = PendingAnnots::default();
        assert!(!p.is_dirty());
        assert!(p.add(hl(0)) && p.add(hl(1)));
        assert_eq!(p.items().len(), 2);
        assert!(p.undo());
        assert_eq!(p.items(), &[hl(0)]);
        assert!(p.redo());
        assert_eq!(p.items(), &[hl(0), hl(1)]);
        assert!(!p.redo());
    }

    #[test]
    fn a_new_edit_discards_the_redo_stack() {
        let mut p = PendingAnnots::default();
        p.add(hl(0));
        p.undo();
        p.add(hl(2));
        assert!(!p.redo());
        assert_eq!(p.items(), &[hl(2)]);
    }

    #[test]
    fn empty_annotations_are_ignored() {
        let mut p = PendingAnnots::default();
        assert!(!p.add(NewAnnot::Highlight {
            page: 0,
            rects: vec![]
        }));
        assert!(!p.add(NewAnnot::Ink {
            page: 0,
            strokes: vec![vec![(1.0, 1.0)]]
        }));
        assert!(!p.add(NewAnnot::Note {
            page: 0,
            at: (0.0, 0.0),
            text: "  ".into()
        }));
        assert!(!p.is_dirty());
        assert!(!p.undo());
    }

    #[test]
    fn clear_resets_everything() {
        let mut p = PendingAnnots::default();
        p.add(hl(0));
        p.undo();
        p.clear();
        assert!(!p.is_dirty() && !p.redo());
    }
}
