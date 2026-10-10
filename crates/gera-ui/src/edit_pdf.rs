//! Editar PDF (como no Acrobat): as imagens da página aparecem com contorno; clicar seleciona,
//! arrastar move e as alças dos cantos redimensionam mantendo a proporção. Delete exclui, as
//! setas movem e a linha da ferramenta gira, substitui e acrescenta imagens.
use crate::edit::screen_rect;
use app_state::annot_geom::{handle_at, resized_keep_aspect, Handle};
use app_state::transform::PageMap;
use pdf_core::{ImageEdit, PageImage, PdfRect, Request, Worker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Arrasto em andamento: mover ou redimensionar a imagem selecionada.
struct Drag {
    page: i32,
    name: String,
    rect: PdfRect,
    grab: Option<Handle>,
    from: (f32, f32),
    to: (f32, f32),
}

impl Drag {
    fn new(page: i32, img: &PageImage, grab: Option<Handle>, at: (f32, f32)) -> Self {
        Self {
            page,
            name: img.name.clone(),
            rect: img.rect,
            grab,
            from: at,
            to: at,
        }
    }

    fn delta(&self) -> (f32, f32) {
        (self.to.0 - self.from.0, self.to.1 - self.from.1)
    }

    fn preview(&self) -> PdfRect {
        let (dx, dy) = self.delta();
        match self.grab {
            None => self.rect.translated(dx, dy),
            Some(h) => {
                let (x, y) = h.point(self.rect);
                resized_keep_aspect(self.rect, h, (x + dx, y + dy), 4.0)
            }
        }
    }

    /// A edição que o arrasto pede (nenhuma se nada mudou).
    fn edit(&self) -> Option<ImageEdit> {
        let (dx, dy) = self.delta();
        if dx.abs() < 0.01 && dy.abs() < 0.01 {
            return None;
        }
        Some(match self.grab {
            None => ImageEdit::Move { dx, dy },
            Some(_) => ImageEdit::Resize {
                rect: self.preview(),
            },
        })
    }
}

/// O que a página pede à aba.
pub enum ImageAction {
    Edit {
        page: i32,
        name: String,
        edit: ImageEdit,
    },
    Add {
        page: i32,
        path: PathBuf,
        center: (f32, f32),
    },
    /// Aviso para a barra de status.
    Notice(String),
}

#[derive(Default)]
pub struct EditPdfState {
    /// Imagens de cada página, como o worker as listou.
    images: HashMap<i32, Vec<PageImage>>,
    /// Páginas com a lista pedida e ainda não recebida.
    requested: HashSet<i32>,
    /// Imagem selecionada: (página, nome).
    pub selected: Option<(i32, String)>,
    drag: Option<Drag>,
    /// Imagem escolhida em Adicionar imagem: o próximo clique na página a coloca.
    pub placing: Option<PathBuf>,
}

/// Cor dos contornos das imagens editáveis.
const OUTLINE: egui::Color32 = egui::Color32::from_rgb(40, 120, 230);
/// Contorno das imagens que não dá para editar.
const LOCKED: egui::Color32 = egui::Color32::from_rgb(150, 150, 150);
/// Tamanho da alça, em pixels da tela.
const HANDLE: f32 = 6.0;

impl EditPdfState {
    /// A lista de imagens da página chegou.
    pub fn on_images(&mut self, page: i32, images: Vec<PageImage>) {
        self.requested.remove(&page);
        if let Some((p, name)) = &self.selected {
            if *p == page && !images.iter().any(|i| i.editable && &i.name == name) {
                self.selected = None;
            }
        }
        self.images.insert(page, images);
    }

    /// As páginas mudaram: as listas delas são pedidas de novo.
    pub fn invalidate(&mut self, pages: &[i32]) {
        for p in pages {
            self.images.remove(p);
            self.requested.remove(p);
        }
    }

    /// O documento todo mudou (abrir, desfazer).
    pub fn clear(&mut self) {
        self.images.clear();
        self.requested.clear();
        self.drag = None;
    }

    /// Outra ferramenta: nada selecionado nem a colocar.
    pub fn reset(&mut self) {
        self.selected = None;
        self.placing = None;
    }

    /// Excluir: a imagem selecionada sai (e a seleção com ela).
    pub fn delete_selected(&mut self) -> Option<ImageAction> {
        let (page, name) = self.selected.take()?;
        Some(ImageAction::Edit {
            page,
            name,
            edit: ImageEdit::Delete,
        })
    }

    /// Esc: solta a imagem a colocar, depois a seleção. Devolve se havia o que soltar.
    pub fn escape(&mut self) -> bool {
        self.placing.take().is_some() || self.selected.take().is_some()
    }

    /// A imagem selecionada, se estiver na página.
    fn selected_on(&self, page: i32) -> Option<&PageImage> {
        let (p, name) = self.selected.as_ref().filter(|(p, _)| *p == page)?;
        self.images.get(p)?.iter().find(|i| &i.name == name)
    }

    /// A edição pedida para a imagem selecionada.
    pub fn edit_selected(&self, edit: ImageEdit) -> Option<ImageAction> {
        let (page, name) = self.selected.clone()?;
        Some(ImageAction::Edit { page, name, edit })
    }

    /// A imagem sob `p` (a de cima).
    fn hit(&self, page: i32, p: (f32, f32)) -> Option<&PageImage> {
        self.images
            .get(&page)?
            .iter()
            .rev()
            .find(|i| i.rect.contains(p.0, p.1))
    }

    /// Começo de um arrasto em `p`: alça da selecionada, ou o corpo de uma imagem editável.
    fn start_drag(&mut self, page: i32, p: (f32, f32), handle_size: f32) -> Option<ImageAction> {
        let on_handle = self.selected_on(page).and_then(|i| {
            Some(Drag::new(
                page,
                i,
                Some(handle_at(i.rect, p, handle_size)?),
                p,
            ))
        });
        if on_handle.is_some() {
            self.drag = on_handle;
            return None;
        }
        match self.hit(page, p) {
            Some(img) if img.editable => {
                let drag = Drag::new(page, img, None, p);
                self.selected = Some((page, drag.name.clone()));
                self.drag = Some(drag);
                None
            }
            Some(_) => Some(locked_notice()),
            None => None,
        }
    }

    /// Clique em `p`: seleciona a imagem (ou solta a seleção fora delas).
    fn click(&mut self, page: i32, p: (f32, f32)) -> Option<ImageAction> {
        match self.hit(page, p).cloned() {
            Some(img) if img.editable => {
                self.selected = Some((page, img.name));
                None
            }
            Some(_) => {
                self.selected = None;
                Some(locked_notice())
            }
            None => {
                self.selected = None;
                None
            }
        }
    }
}

fn locked_notice() -> ImageAction {
    ImageAction::Notice(
        "Esta imagem não pode ser editada: aparece mais de uma vez ou faz parte de um formulário."
            .into(),
    )
}

/// Imagens da página no modo Editar PDF: contornos, seleção, arrastos e colocação.
pub fn on_page(
    ui: &mut egui::Ui,
    st: &mut EditPdfState,
    worker: &Worker,
    page: i32,
    rect: egui::Rect,
    map: &PageMap,
) -> Vec<ImageAction> {
    if !st.images.contains_key(&page) && st.requested.insert(page) {
        worker.send(Request::PageImages { page });
    }
    let mut actions = Vec::new();
    let response = ui.interact(
        rect,
        ui.id().with(("editar-pdf", page)),
        egui::Sense::click_and_drag(),
    );
    let pointer = response
        .interact_pointer_pos()
        .map(|p| map.to_page(p.into()));
    let handle_size = HANDLE / map.zoom;
    if st.placing.is_some() {
        if let Some(center) = pointer.filter(|_| response.clicked()) {
            let path = st.placing.take().expect("conferido acima");
            actions.push(ImageAction::Add { page, path, center });
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    } else {
        if response.drag_started() {
            if let Some(p) = crate::edit::press_point(&response, map, pointer) {
                actions.extend(st.start_drag(page, p, handle_size));
            }
        }
        if let Some(d) = st.drag.as_mut().filter(|d| d.page == page) {
            if let Some(p) = pointer {
                d.to = p;
            }
            if response.drag_stopped() {
                if let Some(edit) = d.edit() {
                    actions.push(ImageAction::Edit {
                        page,
                        name: d.name.clone(),
                        edit,
                    });
                }
                st.drag = None;
            }
        } else if response.clicked() {
            actions.extend(pointer.and_then(|p| st.click(page, p)));
        }
        hover_cursor(ui, st, &response, page, map, handle_size);
    }
    paint(ui.painter(), st, page, map);
    actions
}

/// Cursor de mover sobre as imagens editáveis e de redimensionar sobre as alças.
fn hover_cursor(
    ui: &egui::Ui,
    st: &EditPdfState,
    response: &egui::Response,
    page: i32,
    map: &PageMap,
    handle_size: f32,
) {
    let Some(p) = response.hover_pos().map(|p| map.to_page(p.into())) else {
        return;
    };
    let on_handle = st
        .selected_on(page)
        .and_then(|i| handle_at(i.rect, p, handle_size));
    let icon = match on_handle {
        Some(Handle::TopLeft | Handle::BottomRight) => egui::CursorIcon::ResizeNwSe,
        Some(Handle::TopRight | Handle::BottomLeft) => egui::CursorIcon::ResizeNeSw,
        None if st.hit(page, p).is_some_and(|i| i.editable) => egui::CursorIcon::Move,
        None => return,
    };
    ui.ctx().set_cursor_icon(icon);
}

/// Contornos das imagens; a selecionada com alças (e a prévia do arrasto).
fn paint(painter: &egui::Painter, st: &EditPdfState, page: i32, map: &PageMap) {
    let Some(images) = st.images.get(&page) else {
        return;
    };
    for img in images {
        let color = if img.editable { OUTLINE } else { LOCKED };
        painter.rect_stroke(
            screen_rect(&img.rect, map),
            0.0,
            egui::Stroke::new(1.0, color),
            egui::StrokeKind::Outside,
        );
    }
    let Some(img) = st.selected_on(page) else {
        return;
    };
    let dragging = st
        .drag
        .as_ref()
        .filter(|d| d.page == page && d.name == img.name);
    let rect = dragging.map_or(img.rect, Drag::preview);
    let r = screen_rect(&rect, map);
    if dragging.is_some() {
        painter.rect_filled(r, 0.0, OUTLINE.gamma_multiply(0.15));
    }
    painter.rect_stroke(
        r,
        0.0,
        egui::Stroke::new(2.0, OUTLINE),
        egui::StrokeKind::Outside,
    );
    for h in Handle::ALL {
        crate::edit::paint_handle(painter, map.to_screen(h.point(rect)).into(), OUTLINE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(name: &str, rect: PdfRect, editable: bool) -> PageImage {
        PageImage {
            name: name.into(),
            rect,
            editable,
        }
    }

    fn state() -> EditPdfState {
        let mut st = EditPdfState::default();
        st.on_images(
            0,
            vec![
                img("Im0", PdfRect::new(10.0, 10.0, 110.0, 60.0), true),
                img("", PdfRect::new(200.0, 200.0, 300.0, 300.0), false),
            ],
        );
        st
    }

    #[test]
    fn dragging_the_body_moves_and_a_corner_resizes_keeping_the_shape() {
        let mut st = state();
        assert!(st.start_drag(0, (50.0, 30.0), 6.0).is_none());
        st.drag.as_mut().unwrap().to = (70.0, 40.0);
        assert_eq!(
            st.drag.as_ref().unwrap().edit(),
            Some(ImageEdit::Move { dx: 20.0, dy: 10.0 })
        );
        assert_eq!(st.selected, Some((0, "Im0".into())));
        // alça de baixo à direita puxada 100 pt para a direita: dobra, mantendo 2:1
        st.start_drag(0, (110.0, 60.0), 6.0);
        st.drag.as_mut().unwrap().to = (210.0, 60.0);
        let Some(ImageEdit::Resize { rect }) = st.drag.as_ref().unwrap().edit() else {
            panic!("redimensionar");
        };
        assert_eq!((rect.width(), rect.height()), (200.0, 100.0));
    }

    #[test]
    fn locked_images_explain_why_and_are_not_selected() {
        let mut st = state();
        assert!(matches!(
            st.click(0, (250.0, 250.0)),
            Some(ImageAction::Notice(_))
        ));
        assert!(st.selected.is_none());
        assert!(matches!(
            st.start_drag(0, (250.0, 250.0), 6.0),
            Some(ImageAction::Notice(_))
        ));
        assert!(st.drag.is_none());
    }

    #[test]
    fn a_replaced_or_deleted_image_leaves_the_selection() {
        let mut st = state();
        st.click(0, (50.0, 30.0));
        assert_eq!(st.selected, Some((0, "Im0".into())));
        st.on_images(
            0,
            vec![img("GeraIm0", PdfRect::new(10.0, 10.0, 110.0, 60.0), true)],
        );
        assert!(st.selected.is_none());
    }

    #[test]
    fn escape_drops_the_image_to_place_then_the_selection() {
        let mut st = state();
        st.click(0, (50.0, 30.0));
        st.placing = Some("x.png".into());
        assert!(st.escape() && st.placing.is_none() && st.selected.is_some());
        assert!(st.escape() && st.selected.is_none());
        assert!(!st.escape());
    }

    #[test]
    fn delete_takes_the_selection() {
        let mut st = state();
        st.click(0, (50.0, 30.0));
        assert!(matches!(
            st.delete_selected(),
            Some(ImageAction::Edit {
                edit: ImageEdit::Delete,
                ..
            })
        ));
        assert!(st.selected.is_none() && st.delete_selected().is_none());
    }
}
