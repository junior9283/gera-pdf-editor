//! Carimbos personalizados: o catálogo do usuário (em cache para todas as abas) e os diálogos
//! Criar carimbo e Gerenciar carimbos, como no Acrobat.
use crate::edit::CustomStamp;
use app_state::custom_stamps::{self, Catalog};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CACHE: Mutex<Option<Catalog>> = Mutex::new(None);
/// Categoria sugerida quando ainda não há nenhuma.
const DEFAULT_CATEGORY: &str = "Personalizados";

/// O catálogo (lido do disco na primeira vez e depois de cada mudança).
pub fn catalog() -> Catalog {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(|| {
            custom_stamps::base_dir()
                .map(|d| Catalog::load(&d))
                .unwrap_or_default()
        })
        .clone()
}

fn invalidate() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// A escolha do menu para um carimbo do catálogo.
pub fn choice(name: &str, path: &Path) -> CustomStamp {
    CustomStamp {
        name: name.to_string(),
        path: path.to_path_buf(),
        size: pdf_core::stamp_image_size(path)
            .unwrap_or((pdf_core::STAMP_WIDTH, pdf_core::STAMP_HEIGHT)),
    }
}

/// Diálogo Criar carimbo personalizado.
pub struct CreateStamp {
    pub source: Option<PathBuf>,
    pages: i32,
    page: i32,
    category: String,
    name: String,
    error: String,
}

pub enum CreateAction {
    PickFile,
    Done(CustomStamp),
    Cancel,
}

impl Default for CreateStamp {
    fn default() -> Self {
        let category = catalog()
            .categories
            .first()
            .map_or_else(|| DEFAULT_CATEGORY.to_string(), |c| c.name.clone());
        Self {
            source: None,
            pages: 0,
            page: 1,
            category,
            name: String::new(),
            error: String::new(),
        }
    }
}

impl CreateStamp {
    /// Arquivo escolhido: conta as páginas e sugere o nome.
    pub fn set_source(&mut self, path: PathBuf) {
        match pdf_core::stamp_source_pages(&path) {
            Ok(n) => {
                self.pages = n;
                self.page = 1;
                self.error.clear();
                if self.name.trim().is_empty() {
                    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                    self.name = stem
                        .and_then(|s| custom_stamps::sanitize(&s))
                        .unwrap_or_default();
                }
                self.source = Some(path);
            }
            Err(e) => {
                self.error = e.to_string();
                self.source = None;
            }
        }
    }

    fn create(&self) -> Result<CustomStamp, String> {
        let src = self.source.as_ref().ok_or("escolha um arquivo")?;
        let category =
            custom_stamps::sanitize(&self.category).ok_or("nome de categoria inválido")?;
        let name = custom_stamps::sanitize(&self.name).ok_or("nome de carimbo inválido")?;
        let base = custom_stamps::base_dir().ok_or("pasta de configuração indisponível")?;
        let dir = crate::print_job::spool_dir().map_err(|e| e.to_string())?;
        let tmp = dir.join(crate::print_job::spool_file_name("gera-carimbo-", ".png"));
        let made = pdf_core::stamp_image_from_file(src, self.page - 1, &tmp)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                custom_stamps::create(&base, &category, &name, &tmp).map_err(|e| e.to_string())
            });
        let _ = std::fs::remove_file(&tmp);
        invalidate();
        Ok(choice(&name, &made?))
    }

    pub fn window(&mut self, ctx: &egui::Context) -> Option<CreateAction> {
        let mut action = None;
        let mut open = true;
        egui::Window::new("Criar carimbo personalizado")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Escolher arquivo…").clicked() {
                        action = Some(CreateAction::PickFile);
                    }
                    let shown = self
                        .source
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or_else(
                            || "Nenhum arquivo (PDF ou imagem)".to_string(),
                            |n| crate::links::safe_display(&n.to_string_lossy()),
                        );
                    ui.label(shown);
                });
                if self.pages > 1 {
                    ui.horizontal(|ui| {
                        ui.label("Página:");
                        ui.add(egui::DragValue::new(&mut self.page).range(1..=self.pages));
                        ui.label(format!("de {}", self.pages));
                    });
                }
                egui::Grid::new("carimbo-novo")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Categoria:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.category).desired_width(160.0),
                            );
                            ui.menu_button("⏷", |ui| {
                                for c in catalog().categories {
                                    if ui.button(&c.name).clicked() {
                                        self.category = c.name;
                                    }
                                }
                            });
                        });
                        ui.end_row();
                        ui.label("Nome:");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(190.0));
                        ui.end_row();
                    });
                if !self.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &self.error);
                }
                ui.horizontal(|ui| {
                    let ready = self.source.is_some() && !self.name.trim().is_empty();
                    if ui.add_enabled(ready, egui::Button::new("Criar")).clicked() {
                        match self.create() {
                            Ok(c) => action = Some(CreateAction::Done(c)),
                            Err(e) => self.error = e,
                        }
                    }
                    if ui.button("Cancelar").clicked() {
                        action = Some(CreateAction::Cancel);
                    }
                });
            });
        if !open {
            action = Some(CreateAction::Cancel);
        }
        action
    }
}

/// Diálogo Gerenciar carimbos: renomear, mudar de categoria e excluir.
#[derive(Default)]
pub struct ManageStamps {
    /// Renomeando: (categoria, nome atual, nome novo).
    renaming: Option<(String, String, String)>,
    /// Nome de uma categoria nova para o "Mover para".
    new_category: String,
    error: String,
}

impl ManageStamps {
    /// Desenha; devolve `false` quando fecha.
    pub fn window(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let Some(base) = custom_stamps::base_dir() else {
            return false;
        };
        let cat = catalog();
        let categories: Vec<String> = cat.categories.iter().map(|c| c.name.clone()).collect();
        let mut result: Option<std::io::Result<()>> = None;
        egui::Window::new("Gerenciar carimbos")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                if cat.categories.is_empty() {
                    ui.label("Nenhum carimbo personalizado. Use Criar… no menu do Carimbo.");
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for c in &cat.categories {
                            ui.strong(&c.name);
                            for s in &c.stamps {
                                ui.horizontal(|ui| {
                                    match self
                                        .renaming
                                        .as_mut()
                                        .filter(|r| r.0 == c.name && r.1 == s.name)
                                    {
                                        Some(r) => {
                                            let field = ui.add(
                                                egui::TextEdit::singleline(&mut r.2)
                                                    .desired_width(160.0),
                                            );
                                            if field.lost_focus() || ui.button("OK").clicked() {
                                                if let Some((cat, from, to)) = self.renaming.take()
                                                {
                                                    if to.trim() != from {
                                                        result = Some(custom_stamps::rename(
                                                            &base, &cat, &from, &to,
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                        None => {
                                            ui.label(crate::links::safe_display(&s.name));
                                            if ui.small_button("Renomear").clicked() {
                                                self.renaming = Some((
                                                    c.name.clone(),
                                                    s.name.clone(),
                                                    s.name.clone(),
                                                ));
                                            }
                                        }
                                    }
                                    ui.menu_button("Mover para", |ui| {
                                        for other in categories.iter().filter(|o| **o != c.name) {
                                            if ui.button(other).clicked() {
                                                result = Some(custom_stamps::move_to(
                                                    &base, &c.name, &s.name, other,
                                                ));
                                            }
                                        }
                                        ui.separator();
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.new_category)
                                                    .hint_text("Nova categoria")
                                                    .desired_width(120.0),
                                            );
                                            if ui.button("Mover").clicked() {
                                                result = Some(custom_stamps::move_to(
                                                    &base,
                                                    &c.name,
                                                    &s.name,
                                                    &self.new_category,
                                                ));
                                            }
                                        });
                                    });
                                    if ui.small_button("Excluir").clicked() {
                                        result =
                                            Some(custom_stamps::remove(&base, &c.name, &s.name));
                                    }
                                });
                            }
                            ui.add_space(4.0);
                        }
                    });
                if !self.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &self.error);
                }
            });
        match result {
            Some(Ok(())) => {
                invalidate();
                self.error.clear();
            }
            Some(Err(e)) => self.error = format!("Não foi possível: {e}"),
            None => {}
        }
        open
    }
}
