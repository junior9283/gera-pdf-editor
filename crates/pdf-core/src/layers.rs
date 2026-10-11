//! Camadas (conteúdo opcional, OCG), como no painel Camadas do Acrobat.
//!
//! O olho é estado de vista: não marca o documento como alterado, não entra no desfazer e nunca
//! chega ao arquivo. O MuPDF 0.8 não expõe a configuração de camadas, e ele a lê ao abrir. Por
//! isso o olho grava o estado efetivo em `/OCProperties /D` (`/ON`, `/OFF`) na memória e reabre o
//! documento dos bytes. O estado padrão do arquivo fica guardado à parte e volta antes de cada
//! gravação e de cada instantâneo do histórico. Nome e estado padrão (Propriedades da camada) são
//! edições normais, desfazíveis.
use crate::doc::{reopen, PdfDoc};
use crate::error::{refused, Result};
use crate::pdf_obj::{get, name_of, text};
use mupdf::pdf::PdfObject;
use std::collections::HashSet;

/// Tetos da leitura.
const MAX_LAYERS: usize = 10_000;
const MAX_ORDER_DEPTH: usize = 32;

/// Item da árvore do painel Camadas.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerNode {
    Layer {
        /// Número do objeto da camada (OCG).
        id: i32,
        name: String,
        /// Visível agora (o olho).
        visible: bool,
        /// Visível ao abrir o arquivo (estado padrão).
        default_on: bool,
        /// Travada: o olho não muda.
        locked: bool,
        kids: Vec<LayerNode>,
    },
    /// Rótulo de grupo (texto no `/Order`, sem camada).
    Label { text: String, kids: Vec<LayerNode> },
}

/// `/D /BaseState`, `/ON` e `/OFF` como estão no arquivo (`None`: a chave não existe).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LayerDefault {
    base: Option<String>,
    on: Option<Vec<i32>>,
    off: Option<Vec<i32>>,
}

impl LayerDefault {
    /// Consulta rápida (conjuntos) do estado padrão de cada camada.
    fn lookup(&self) -> DefaultLookup {
        let set = |l: &Option<Vec<i32>>| l.iter().flatten().copied().collect();
        DefaultLookup {
            base_off: self.base.as_deref() == Some("OFF"),
            on: set(&self.on),
            off: set(&self.off),
        }
    }

    /// Muda o estado padrão de `id` mantendo o `/BaseState`: com base desligada, as ligadas
    /// ficam em `/ON`; senão, as desligadas ficam em `/OFF`.
    fn set(&mut self, id: i32, on: bool) {
        let base_off = self.base.as_deref() == Some("OFF");
        let list = if base_off {
            &mut self.on
        } else {
            &mut self.off
        };
        let list = list.get_or_insert_with(Vec::new);
        list.retain(|&x| x != id);
        if on == base_off {
            list.push(id);
        }
    }
}

/// O estado padrão em conjuntos (milhares de camadas sem custo quadrático).
struct DefaultLookup {
    base_off: bool,
    on: HashSet<i32>,
    off: HashSet<i32>,
}

impl DefaultLookup {
    fn is_on(&self, id: i32) -> bool {
        if self.base_off {
            self.on.contains(&id)
        } else {
            !self.off.contains(&id)
        }
    }
}

/// Números dos objetos indiretos de uma lista (com teto).
fn ids(list: Option<PdfObject>) -> Option<Vec<i32>> {
    let list = list.filter(|l| l.is_array().unwrap_or(false))?;
    let len = list.len().unwrap_or(0).min(MAX_LAYERS) as i32;
    Some(
        (0..len)
            .filter_map(|k| list.get_array(k).ok().flatten()?.as_indirect().ok())
            .filter(|&id| id != 0)
            .collect(),
    )
}

impl PdfDoc {
    fn oc_properties(&self) -> Option<PdfObject> {
        self.inner
            .catalog()
            .ok()
            .and_then(|c| get(&c, "OCProperties"))
            .filter(|p| p.is_dict().unwrap_or(false))
    }

    fn oc_config(&self) -> Option<PdfObject> {
        self.oc_properties()
            .and_then(|p| get(&p, "D"))
            .filter(|d| d.is_dict().unwrap_or(false))
    }

    /// As camadas do documento (`/OCGs`), na ordem do arquivo.
    fn ocgs(&self) -> Vec<i32> {
        ids(self.oc_properties().and_then(|p| get(&p, "OCGs"))).unwrap_or_default()
    }

    /// Camadas travadas (`/D /Locked`).
    fn locked(&self) -> HashSet<i32> {
        self.oc_config()
            .and_then(|d| ids(get(&d, "Locked")))
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    /// Estado padrão lido do `/D` como está agora no documento.
    fn read_default(&self) -> LayerDefault {
        let Some(d) = self.oc_config() else {
            return LayerDefault::default();
        };
        LayerDefault {
            base: get(&d, "BaseState").and_then(|b| name_of(&b)),
            on: ids(get(&d, "ON")),
            off: ids(get(&d, "OFF")),
        }
    }

    /// O estado padrão do arquivo (o guardado, quando o olho mudou o `/D` da memória).
    fn file_default(&self) -> LayerDefault {
        self.layer_default
            .clone()
            .unwrap_or_else(|| self.read_default())
    }

    fn visible(&self, def: &DefaultLookup, id: i32) -> bool {
        self.layer_view
            .get(&id)
            .copied()
            .unwrap_or_else(|| def.is_on(id))
    }

    /// Grava `/BaseState`, `/ON` e `/OFF` no `/D`.
    pub(crate) fn write_layer_config(&self, def: &LayerDefault) -> Result<()> {
        let Some(mut d) = self.oc_config() else {
            return Ok(());
        };
        match &def.base {
            Some(b) => d.dict_put("BaseState", PdfObject::new_name(b)?)?,
            None => d.dict_delete("BaseState")?,
        }
        for (key, list) in [("ON", &def.on), ("OFF", &def.off)] {
            match list {
                Some(list) => {
                    let mut arr = self.inner.new_array()?;
                    for &id in list {
                        arr.array_push(self.inner.new_indirect(id, 0)?)?;
                    }
                    d.dict_put(key, arr)?;
                }
                None => d.dict_delete(key)?,
            }
        }
        Ok(())
    }

    /// O `/D` com o que o olho mostra agora.
    fn effective_config(&self) -> LayerDefault {
        let def = self.file_default().lookup();
        let (on, off) = self
            .ocgs()
            .into_iter()
            .partition(|&id| self.visible(&def, id));
        LayerDefault {
            base: Some("ON".into()),
            on: Some(on),
            off: Some(off),
        }
    }

    /// Reabre o documento dos próprios bytes (o MuPDF relê as camadas).
    fn reopen_current(&mut self) -> Result<()> {
        let bytes = self.memory_bytes()?;
        self.inner = reopen(&bytes, self.password.as_deref())?;
        Ok(())
    }

    /// Aplica o olho: sem escolhas, o `/D` volta ao do arquivo; com escolhas, o `/D` da memória
    /// passa a ser o efetivo. Reabre para o MuPDF ler.
    pub(crate) fn apply_layer_view(&mut self) -> Result<()> {
        if self.layer_view.is_empty() {
            let Some(def) = self.layer_default.take() else {
                return Ok(());
            };
            self.write_layer_config(&def)?;
        } else {
            if self.layer_default.is_none() {
                self.layer_default = Some(self.read_default());
            }
            self.write_layer_config(&self.effective_config())?;
        }
        self.reopen_current()
    }

    /// Depois de trocar o documento da memória (desfazer, reverter, salvar): o `/D` lido é o do
    /// arquivo, e as escolhas do olho voltam a valer.
    pub(crate) fn reapply_layer_view(&mut self) -> Result<()> {
        self.layer_default = None;
        if self.layer_view.is_empty() {
            return Ok(());
        }
        self.apply_layer_view()
    }

    /// Grava com o `/D` das camadas do arquivo e depois volta ao do olho: o olho nunca chega a um
    /// arquivo, a um instantâneo do desfazer ou a uma cópia (Comprimir, Assinar). A impressão
    /// não passa por aqui: imprime o que se vê, como no Acrobat.
    pub(crate) fn with_file_layers<T>(&self, write: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let Some(def) = &self.layer_default else {
            return write(self);
        };
        self.write_layer_config(def)?;
        let result = write(self);
        self.write_layer_config(&self.effective_config())?;
        result
    }

    /// Árvore do painel Camadas (como o `/Order`; sem ele, a lista das camadas).
    pub fn layers(&self) -> Vec<LayerNode> {
        let ocgs = self.ocgs();
        if ocgs.is_empty() {
            return Vec::new();
        }
        let def = self.file_default().lookup();
        let locked = self.locked();
        let all: HashSet<i32> = ocgs.iter().copied().collect();
        let node = |id: i32| -> Option<LayerNode> {
            let obj = self.inner.new_indirect(id, 0).ok()?;
            Some(LayerNode::Layer {
                id,
                name: text(&obj, "Name"),
                visible: self.visible(&def, id),
                default_on: def.is_on(id),
                locked: locked.contains(&id),
                kids: Vec::new(),
            })
        };
        let order = self.oc_config().and_then(|d| get(&d, "Order"));
        let Some(order) = order.filter(|o| o.is_array().unwrap_or(false)) else {
            return ocgs.into_iter().filter_map(node).collect();
        };
        let mut reading = OrderReading {
            all: &all,
            layers: HashSet::new(),
            lists: HashSet::new(),
            budget: 4 * MAX_LAYERS,
        };
        read_order(&order, 0, &mut reading, &node)
    }

    /// Olho de uma camada (só a vista).
    pub fn set_layer_view(&mut self, id: i32, on: bool) -> Result<()> {
        if !self.ocgs().contains(&id) {
            return Err(refused("camada não encontrada"));
        }
        if self.locked().contains(&id) {
            return Err(refused("a camada está travada"));
        }
        self.layer_view.insert(id, on);
        self.apply_layer_view()
    }

    /// Redefinir para visibilidade inicial.
    pub fn reset_layer_view(&mut self) -> Result<()> {
        self.layer_view.clear();
        self.apply_layer_view()
    }

    /// Propriedades da camada: nome e estado padrão (uma etapa do desfazer).
    pub fn edit_layer(&mut self, id: i32, name: &str, default_on: bool) -> Result<()> {
        if !self.ocgs().contains(&id) {
            return Err(refused("camada não encontrada"));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(refused("a camada precisa de um nome"));
        }
        self.require_modifiable("alterar as camadas")?;
        self.with_snapshot(|d| {
            let mut ocg = d.inner.new_indirect(id, 0)?;
            ocg.dict_put("Name", PdfObject::new_string(name)?)?;
            let mut def = d.file_default();
            def.set(id, default_on);
            d.write_layer_config(&def)?;
            d.layer_default = None;
            if d.layer_view.is_empty() {
                d.reopen_current()
            } else {
                d.apply_layer_view()
            }
        })
    }
}

/// Estado da leitura do `/Order`: cada camada e cada lista indireta uma vez, e um teto de itens
/// no total (listas que apontam para si mesmas não explodem).
struct OrderReading<'a> {
    all: &'a HashSet<i32>,
    layers: HashSet<i32>,
    lists: HashSet<i32>,
    budget: usize,
}

/// Itens do `/Order`: camada; lista logo depois de uma camada = filhos dela; lista que começa com
/// texto = grupo com rótulo. Cada camada aparece uma vez.
fn read_order(
    list: &PdfObject,
    depth: usize,
    r: &mut OrderReading,
    node: &dyn Fn(i32) -> Option<LayerNode>,
) -> Vec<LayerNode> {
    let mut out: Vec<LayerNode> = Vec::new();
    if depth > MAX_ORDER_DEPTH {
        return out;
    }
    if let Ok(id) = list.as_indirect() {
        if id != 0 && !r.lists.insert(id) {
            return out;
        }
    }
    let len = list.len().unwrap_or(0).min(MAX_LAYERS) as i32;
    for k in 0..len {
        if r.budget == 0 {
            break;
        }
        r.budget -= 1;
        let Ok(Some(item)) = list.get_array(k) else {
            continue;
        };
        if item.is_array().unwrap_or(false) {
            let label = item
                .get_array(0)
                .ok()
                .flatten()
                .filter(|f| f.is_string().unwrap_or(false))
                .and_then(|f| f.as_string().ok());
            // o rótulo (texto) é pulado na leitura dos itens; os demais são o grupo
            let kids = read_order(&item, depth + 1, r, node);
            match (label, out.last_mut()) {
                (Some(text), _) => out.push(LayerNode::Label { text, kids }),
                (None, Some(LayerNode::Layer { kids: k, .. })) => k.extend(kids),
                (None, _) => out.extend(kids),
            }
            continue;
        }
        let Ok(id) = item.as_indirect() else { continue };
        if r.all.contains(&id) && r.layers.insert(id) {
            out.extend(node(id));
        }
    }
    out
}
