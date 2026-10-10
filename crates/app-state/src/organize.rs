//! Grade de Organizar páginas: seleção (clique, Ctrl, Shift, seletores do Acrobat), colunas e o
//! destino de um arrasto. Páginas a partir de 0.
use std::collections::BTreeSet;

/// Páginas selecionadas e a âncora do Shift+clique.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageSet {
    pages: BTreeSet<i32>,
    anchor: Option<i32>,
}

/// "Selecionar ▾" da barra (como no Acrobat). Pares/ímpares pela numeração humana (1, 2, 3…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    All,
    Even,
    Odd,
    Portrait,
    Landscape,
}

impl Picker {
    pub const ALL: [Picker; 5] = [
        Self::All,
        Self::Even,
        Self::Odd,
        Self::Portrait,
        Self::Landscape,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Todas as páginas",
            Self::Even => "Páginas pares",
            Self::Odd => "Páginas ímpares",
            Self::Portrait => "Páginas em retrato",
            Self::Landscape => "Páginas em paisagem",
        }
    }
}

impl PageSet {
    pub fn contains(&self, p: i32) -> bool {
        self.pages.contains(&p)
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Em ordem crescente.
    pub fn to_vec(&self) -> Vec<i32> {
        self.pages.iter().copied().collect()
    }

    /// Clique simples: só esta.
    pub fn click(&mut self, p: i32) {
        self.pages = BTreeSet::from([p]);
        self.anchor = Some(p);
    }

    /// Ctrl+clique: alterna esta.
    pub fn ctrl_click(&mut self, p: i32) {
        if !self.pages.remove(&p) {
            self.pages.insert(p);
        }
        self.anchor = Some(p);
    }

    /// Shift+clique: da âncora até esta.
    pub fn shift_click(&mut self, p: i32) {
        let a = self.anchor.unwrap_or(p);
        self.pages = (a.min(p)..=a.max(p)).collect();
    }

    /// Seletor da barra; `sizes` = (largura, altura) já com a rotação da página.
    pub fn pick(&mut self, picker: Picker, sizes: &[(f32, f32)]) {
        self.pages = (0..sizes.len() as i32)
            .filter(|&p| {
                let (w, h) = sizes[p as usize];
                match picker {
                    Picker::All => true,
                    Picker::Even => (p + 1) % 2 == 0,
                    Picker::Odd => (p + 1) % 2 == 1,
                    Picker::Portrait => h >= w,
                    Picker::Landscape => w > h,
                }
            })
            .collect();
        self.anchor = self.pages.first().copied();
    }

    /// Depois de mover a seleção para antes de `to`: as páginas movidas continuam selecionadas,
    /// agora juntas na posição nova.
    pub fn after_move(&mut self, to: i32) {
        let len = self.pages.len() as i32;
        let start = to - self.pages.iter().filter(|&&p| p < to).count() as i32;
        self.pages = (start..start + len).collect();
        self.anchor = Some(start);
    }

    /// Depois de uma mudança de páginas: tira as que não existem mais.
    pub fn clamp(&mut self, count: i32) {
        self.pages.retain(|&p| p < count);
        if self.anchor.is_some_and(|a| a >= count) {
            self.anchor = None;
        }
    }
}

/// Onde inserir páginas: antes da primeira selecionada ou depois da última. Sem seleção:
/// "antes" vai para o começo e "depois" para o fim.
pub fn insert_at(selected: &[i32], after: bool, total: usize) -> i32 {
    match (after, selected.first(), selected.last()) {
        (false, Some(&first), _) => first,
        (true, _, Some(&last)) => last + 1,
        (false, None, _) => 0,
        (true, _, None) => total as i32,
    }
}

/// Excluir é permitido? O documento precisa ficar com ao menos uma página.
pub fn can_delete(selected: usize, total: usize) -> bool {
    selected > 0 && selected < total
}

/// Quantas miniaturas cabem por linha.
pub fn grid_columns(width: f32, cell: f32) -> usize {
    ((width / cell.max(1.0)).floor() as usize).max(1)
}

/// Onde cai o arrasto: o índice antes do qual as páginas ficam (0..=count). `pointer` é a posição
/// na grade (x, y a partir do canto da grade), `cell` = (largura, altura) de cada célula.
pub fn drop_target(pointer: (f32, f32), cell: (f32, f32), cols: usize, count: usize) -> i32 {
    let col = (pointer.0 / cell.0).floor().max(0.0) as usize;
    let row = (pointer.1 / cell.1).floor().max(0.0) as usize;
    let col = col.min(cols - 1);
    let index = row * cols + col;
    if index >= count {
        return count as i32;
    }
    // metade direita da célula: depois dela
    let right_half = pointer.0 - col as f32 * cell.0 > cell.0 / 2.0;
    (index + usize::from(right_half)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_ctrl_and_shift_select_like_a_file_manager() {
        let mut s = PageSet::default();
        s.click(2);
        s.ctrl_click(5);
        assert_eq!(s.to_vec(), [2, 5]);
        s.ctrl_click(2);
        assert_eq!(s.to_vec(), [5]);
        s.click(1);
        s.shift_click(4);
        assert_eq!(s.to_vec(), [1, 2, 3, 4]);
        s.shift_click(0);
        assert_eq!(s.to_vec(), [0, 1], "a âncora continua no primeiro clique");
    }

    #[test]
    fn pickers_use_human_numbering_and_orientation() {
        let sizes = [
            (600.0, 800.0),
            (800.0, 600.0),
            (600.0, 800.0),
            (600.0, 800.0),
        ];
        let mut s = PageSet::default();
        s.pick(Picker::Even, &sizes);
        assert_eq!(s.to_vec(), [1, 3], "páginas 2 e 4");
        s.pick(Picker::Odd, &sizes);
        assert_eq!(s.to_vec(), [0, 2]);
        s.pick(Picker::Landscape, &sizes);
        assert_eq!(s.to_vec(), [1]);
        s.pick(Picker::All, &sizes);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn moved_pages_stay_selected_at_their_new_place() {
        let mut s = PageSet::default();
        s.click(4);
        s.after_move(0);
        assert_eq!(s.to_vec(), [0]);
        s.click(0);
        s.ctrl_click(2);
        s.after_move(5);
        assert_eq!(s.to_vec(), [3, 4], "duas páginas para o fim de 5");
    }

    #[test]
    fn insertion_point_follows_the_selection() {
        assert_eq!(insert_at(&[2, 4], false, 9), 2);
        assert_eq!(insert_at(&[2, 4], true, 9), 5);
        assert_eq!(insert_at(&[], true, 9), 9, "sem seleção, depois: no fim");
        assert_eq!(insert_at(&[], false, 9), 0, "sem seleção, antes: no começo");
    }

    #[test]
    fn cannot_delete_every_page() {
        assert!(!can_delete(3, 3));
        assert!(!can_delete(0, 3));
        assert!(can_delete(2, 3));
    }

    #[test]
    fn drop_target_uses_the_half_of_the_cell() {
        let cell = (100.0, 150.0);
        assert_eq!(drop_target((10.0, 10.0), cell, 3, 7), 0);
        assert_eq!(drop_target((60.0, 10.0), cell, 3, 7), 1);
        assert_eq!(
            drop_target((260.0, 160.0), cell, 3, 7),
            6,
            "linha 2, coluna 3, metade direita"
        );
        assert_eq!(
            drop_target((10.0, 999.0), cell, 3, 7),
            7,
            "abaixo de tudo: fim"
        );
        assert_eq!(
            drop_target((990.0, 10.0), cell, 3, 7),
            3,
            "à direita da linha: última coluna"
        );
    }

    #[test]
    fn columns_never_drop_below_one_and_selection_follows_page_count() {
        assert_eq!(grid_columns(50.0, 120.0), 1);
        assert_eq!(grid_columns(500.0, 120.0), 4);
        let mut s = PageSet::default();
        s.click(1);
        s.ctrl_click(4);
        s.clamp(3);
        assert_eq!(s.to_vec(), [1]);
    }
}
