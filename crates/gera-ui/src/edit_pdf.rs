//! Editar PDF (como no Acrobat): os blocos de texto e as imagens da página aparecem com contorno.
//! Clicar num bloco de texto abre a edição no lugar (Esc ou clicar fora confirmam); clicar numa
//! imagem seleciona, arrastar move e as alças dos cantos redimensionam mantendo a proporção.
//! Delete exclui, as setas movem e a linha da ferramenta acrescenta texto e imagens, gira e
//! substitui.
use crate::edit::screen_rect;
use app_state::annot_geom::{handle_at, resized_keep_aspect, Handle};
use app_state::transform::PageMap;
use pdf_core::{ImageEdit, PageImage, PageTextBlock, PdfRect, Request, Worker};
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
#[derive(Debug)]
pub enum EditAction {
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
    /// Troca o texto do bloco de caixa `rect`.
    EditText {
        page: i32,
        rect: PdfRect,
        text: String,
    },
    /// Acrescenta texto com o canto superior esquerdo em `at`.
    AddText {
        page: i32,
        at: (f32, f32),
        text: String,
    },
    /// Aviso para a barra de status.
    Notice(String),
}

/// Texto em edição no lugar: um bloco existente ou um novo (Adicionar texto).
pub struct TextEditing {
    page: i32,
    /// Caixa do bloco (o novo: do clique até a margem direita).
    rect: PdfRect,
    original: String,
    pub buffer: String,
    size: f32,
    color: [u8; 3],
    /// Bloco novo, com o canto superior esquerdo no clique.
    new_at: Option<(f32, f32)>,
    /// Pede o foco no primeiro quadro.
    focus: bool,
}

/// Tamanho do texto novo, como no Acrobat (o mesmo do pdf-core).
const NEW_TEXT_SIZE: f32 = 12.0;

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
    /// Blocos de texto de cada página e as páginas com a lista pedida.
    texts: HashMap<i32, Vec<PageTextBlock>>,
    texts_requested: HashSet<i32>,
    /// Texto em edição no lugar.
    pub editing: Option<TextEditing>,
    /// Adicionar texto: o próximo clique na página abre um bloco novo.
    pub adding_text: bool,
}

/// Cor dos contornos das imagens editáveis.
const OUTLINE: egui::Color32 = egui::Color32::from_rgb(40, 120, 230);
/// Contorno dos blocos de texto.
const TEXT_OUTLINE: egui::Color32 = egui::Color32::from_rgb(170, 190, 220);
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

    /// A lista de blocos de texto da página chegou.
    pub fn on_text_blocks(&mut self, page: i32, blocks: Vec<PageTextBlock>) {
        self.texts_requested.remove(&page);
        self.texts.insert(page, blocks);
    }

    /// As páginas mudaram: as listas delas são pedidas de novo.
    pub fn invalidate(&mut self, pages: &[i32]) {
        for p in pages {
            self.images.remove(p);
            self.requested.remove(p);
            self.texts.remove(p);
            self.texts_requested.remove(p);
        }
    }

    /// O documento todo mudou (abrir, desfazer): o texto em edição se perde com ele.
    pub fn clear(&mut self) {
        self.images.clear();
        self.requested.clear();
        self.texts.clear();
        self.texts_requested.clear();
        self.drag = None;
        self.editing = None;
    }

    /// Outra ferramenta: confirma o texto em edição; nada selecionado nem a colocar.
    pub fn reset(&mut self) -> Option<EditAction> {
        self.selected = None;
        self.placing = None;
        self.adding_text = false;
        self.commit()
    }

    /// Fecha a edição de texto: troca (ou acrescenta) o texto se ele mudou.
    pub fn commit(&mut self) -> Option<EditAction> {
        let e = self.editing.take()?;
        let text = e.buffer.trim_end().to_owned();
        match e.new_at {
            Some(at) if !text.trim().is_empty() => Some(EditAction::AddText {
                page: e.page,
                at,
                text,
            }),
            Some(_) => None,
            None if text != e.original.trim_end() => Some(EditAction::EditText {
                page: e.page,
                rect: e.rect,
                text,
            }),
            None => None,
        }
    }

    /// O bloco de texto sob `p`.
    fn text_hit(&self, page: i32, p: (f32, f32)) -> Option<&PageTextBlock> {
        self.texts
            .get(&page)?
            .iter()
            .find(|b| b.rect.contains(p.0, p.1))
    }

    /// Excluir: a imagem selecionada sai (e a seleção com ela).
    pub fn delete_selected(&mut self) -> Option<EditAction> {
        let (page, name) = self.selected.take()?;
        Some(EditAction::Edit {
            page,
            name,
            edit: ImageEdit::Delete,
        })
    }

    /// Esc: solta o que estava a colocar (imagem ou texto), depois a seleção. Devolve se havia o
    /// que soltar.
    pub fn escape(&mut self) -> bool {
        self.placing.take().is_some()
            || std::mem::take(&mut self.adding_text)
            || self.selected.take().is_some()
    }

    /// A imagem selecionada, se estiver na página.
    fn selected_on(&self, page: i32) -> Option<&PageImage> {
        let (p, name) = self.selected.as_ref().filter(|(p, _)| *p == page)?;
        self.images.get(p)?.iter().find(|i| &i.name == name)
    }

    /// A edição pedida para a imagem selecionada.
    pub fn edit_selected(&self, edit: ImageEdit) -> Option<EditAction> {
        let (page, name) = self.selected.clone()?;
        Some(EditAction::Edit { page, name, edit })
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
    fn start_drag(&mut self, page: i32, p: (f32, f32), handle_size: f32) -> Option<EditAction> {
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

    /// Clique em `p`: confirma o texto em edição; depois abre o bloco de texto clicado (ou um novo,
    /// no Adicionar texto) ou seleciona a imagem (ou solta a seleção fora delas).
    fn click(&mut self, page: i32, p: (f32, f32)) -> Vec<EditAction> {
        let mut out: Vec<EditAction> = self.commit().into_iter().collect();
        if std::mem::take(&mut self.adding_text) {
            self.selected = None;
            self.editing = Some(TextEditing {
                page,
                rect: PdfRect::new(p.0, p.1, p.0 + 200.0, p.1 + NEW_TEXT_SIZE * 1.2),
                original: String::new(),
                buffer: String::new(),
                size: NEW_TEXT_SIZE,
                color: [0, 0, 0],
                new_at: Some(p),
                focus: true,
            });
            return out;
        }
        if let Some(b) = self.text_hit(page, p).cloned() {
            self.selected = None;
            if b.editable {
                self.editing = Some(TextEditing {
                    page,
                    rect: b.rect,
                    buffer: b.text.clone(),
                    original: b.text,
                    size: b.size,
                    color: b.color,
                    new_at: None,
                    focus: true,
                });
            } else {
                out.push(EditAction::Notice(
                    "Este texto não pode ser editado.".into(),
                ));
            }
            return out;
        }
        match self.hit(page, p).cloned() {
            Some(img) if img.editable => self.selected = Some((page, img.name)),
            Some(_) => {
                self.selected = None;
                out.push(locked_notice());
            }
            None => self.selected = None,
        }
        out
    }
}

fn locked_notice() -> EditAction {
    EditAction::Notice(
        "Esta imagem não pode ser editada: aparece mais de uma vez ou faz parte de um formulário."
            .into(),
    )
}

/// A página no modo Editar PDF: contornos, edição de texto, seleção, arrastos e colocação.
pub fn on_page(
    ui: &mut egui::Ui,
    st: &mut EditPdfState,
    worker: &Worker,
    page: i32,
    rect: egui::Rect,
    map: &PageMap,
) -> Vec<EditAction> {
    if !st.images.contains_key(&page) && st.requested.insert(page) {
        worker.send(Request::PageImages { page });
    }
    if !st.texts.contains_key(&page) && st.texts_requested.insert(page) {
        worker.send(Request::PageTextBlocks { page });
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
            actions.push(EditAction::Add { page, path, center });
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    } else if st.adding_text {
        if let Some(p) = pointer.filter(|_| response.clicked()) {
            actions.extend(st.click(page, p));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
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
                    actions.push(EditAction::Edit {
                        page,
                        name: d.name.clone(),
                        edit,
                    });
                }
                st.drag = None;
            }
        } else if response.clicked() {
            if let Some(p) = pointer {
                actions.extend(st.click(page, p));
            }
        }
        hover_cursor(ui, st, &response, page, map, handle_size);
    }
    paint(ui.painter(), st, page, map);
    actions.extend(text_editor(ui, st, page, rect, map));
    actions
}

/// O editor de texto no lugar, por cima da página; perder o foco (Esc, clicar fora) confirma.
fn text_editor(
    ui: &mut egui::Ui,
    st: &mut EditPdfState,
    page: i32,
    page_rect: egui::Rect,
    map: &PageMap,
) -> Option<EditAction> {
    let e = st.editing.as_mut().filter(|e| e.page == page)?;
    let r = screen_rect(&e.rect, map);
    let font = egui::FontId::proportional((e.size * map.zoom).max(6.0));
    let [cr, cg, cb] = e.color;
    // o bloco novo vai até a margem direita da página, como o pdf-core quebra
    let width = if e.new_at.is_some() {
        (page_rect.right() - 36.0 * map.zoom - r.left()).max(72.0 * map.zoom)
    } else {
        r.width()
    };
    let lost = egui::Area::new(ui.id().with(("editar-texto", page)))
        .order(egui::Order::Foreground)
        .fixed_pos(r.min)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(egui::Color32::WHITE)
                .stroke(egui::Stroke::new(1.0, OUTLINE))
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut e.buffer)
                            .font(font)
                            .text_color(egui::Color32::from_rgb(cr, cg, cb))
                            .desired_width(width)
                            .desired_rows(1)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO),
                    );
                    if std::mem::take(&mut e.focus) {
                        response.request_focus();
                    }
                    response.lost_focus()
                })
                .inner
        })
        .inner;
    if lost {
        st.commit()
    } else {
        None
    }
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
        None if st.text_hit(page, p).is_some_and(|b| b.editable) => egui::CursorIcon::Text,
        None if st.hit(page, p).is_some_and(|i| i.editable) => egui::CursorIcon::Move,
        None => return,
    };
    ui.ctx().set_cursor_icon(icon);
}

/// Contornos dos blocos de texto e das imagens; a imagem selecionada com alças (e a prévia do
/// arrasto).
fn paint(painter: &egui::Painter, st: &EditPdfState, page: i32, map: &PageMap) {
    let editing = st.editing.as_ref().filter(|e| e.page == page);
    for b in st.texts.get(&page).into_iter().flatten() {
        if editing.is_some_and(|e| e.rect == b.rect) {
            continue;
        }
        painter.rect_stroke(
            screen_rect(&b.rect, map),
            0.0,
            egui::Stroke::new(1.0, if b.editable { TEXT_OUTLINE } else { LOCKED }),
            egui::StrokeKind::Outside,
        );
    }
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
            &st.click(0, (250.0, 250.0))[..],
            [EditAction::Notice(_)]
        ));
        assert!(st.selected.is_none());
        assert!(matches!(
            st.start_drag(0, (250.0, 250.0), 6.0),
            Some(EditAction::Notice(_))
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

    fn text_state() -> EditPdfState {
        let mut st = state();
        st.on_text_blocks(
            0,
            vec![
                PageTextBlock {
                    rect: PdfRect::new(10.0, 100.0, 90.0, 115.0),
                    text: "Olá".into(),
                    size: 12.0,
                    color: [0, 0, 0],
                    font: "Helvetica".into(),
                    editable: true,
                },
                PageTextBlock {
                    rect: PdfRect::new(10.0, 400.0, 20.0, 480.0),
                    text: "Vertical".into(),
                    size: 12.0,
                    color: [0, 0, 0],
                    font: "Helvetica".into(),
                    editable: false,
                },
            ],
        );
        st
    }

    #[test]
    fn clicking_a_text_block_opens_it_and_clicking_out_commits() {
        let mut st = text_state();
        assert!(st.click(0, (50.0, 108.0)).is_empty());
        assert_eq!(st.editing.as_ref().unwrap().buffer, "Olá");
        st.editing.as_mut().unwrap().buffer = "Oi".into();
        let actions = st.click(0, (500.0, 700.0));
        assert!(st.editing.is_none());
        assert!(matches!(
            &actions[..],
            [EditAction::EditText { page: 0, text, .. }] if text == "Oi"
        ));
    }

    #[test]
    fn unchanged_text_commits_nothing() {
        let mut st = text_state();
        st.click(0, (50.0, 108.0));
        assert!(st.commit().is_none() && st.editing.is_none());
    }

    #[test]
    fn adding_text_opens_an_empty_editor_at_the_click() {
        let mut st = text_state();
        st.adding_text = true;
        st.click(0, (300.0, 600.0));
        assert!(!st.adding_text);
        st.editing.as_mut().unwrap().buffer = "Novo".into();
        assert!(matches!(
            st.commit(),
            Some(EditAction::AddText { page: 0, at: (300.0, 600.0), ref text }) if text == "Novo"
        ));
        st.adding_text = true;
        st.click(0, (300.0, 600.0));
        assert!(st.commit().is_none(), "vazio não acrescenta nada");
    }

    #[test]
    fn locked_text_explains_why() {
        let mut st = text_state();
        assert!(matches!(
            &st.click(0, (15.0, 440.0))[..],
            [EditAction::Notice(_)]
        ));
        assert!(st.editing.is_none());
    }

    #[test]
    fn another_tool_commits_the_open_editor() {
        let mut st = text_state();
        st.click(0, (50.0, 108.0));
        st.editing.as_mut().unwrap().buffer = "Trocado".into();
        assert!(matches!(st.reset(), Some(EditAction::EditText { .. })));
        assert!(st.editing.is_none());
    }

    #[test]
    fn delete_takes_the_selection() {
        let mut st = state();
        st.click(0, (50.0, 30.0));
        assert!(matches!(
            st.delete_selected(),
            Some(EditAction::Edit {
                edit: ImageEdit::Delete,
                ..
            })
        ));
        assert!(st.selected.is_none() && st.delete_selected().is_none());
    }
}
