//! Preencher e assinar: ferramentas, desenho da assinatura e assinatura/iniciais guardadas.
use pdf_core::{Mark, PathCmd, PdfRect, SignatureArt, VectorPath};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Ferramenta ativa na barra do Preencher e assinar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillTool {
    Text,
    Mark(Mark),
    Signature,
    Initials,
}

/// Tamanho padrão das marcas (pt) e largura padrão da linha, como no Acrobat.
pub const MARK_SIZE: f32 = 12.0;
pub const LINE_WIDTH: f32 = 40.0;
/// Tamanho padrão do texto e limites de A−/A+.
pub const TEXT_SIZE: f32 = 12.0;
pub const TEXT_SIZES: (f32, f32) = (6.0, 72.0);
/// Altura padrão da assinatura e das iniciais colocadas (pt).
pub const SIGNATURE_HEIGHT: f32 = 36.0;
pub const INITIALS_HEIGHT: f32 = 24.0;

/// Retângulo da marca centrado no clique, como no Acrobat.
pub fn mark_rect(mark: Mark, at: (f32, f32), size: f32) -> PdfRect {
    let (w, h) = if mark == Mark::Line {
        (LINE_WIDTH * size / MARK_SIZE, size)
    } else {
        (size, size)
    };
    PdfRect::new(
        at.0 - w / 2.0,
        at.1 - h / 2.0,
        at.0 + w / 2.0,
        at.1 + h / 2.0,
    )
}

/// Retângulo de altura `h` com a proporção `aspect`, centrado em `at`.
pub fn art_rect(at: (f32, f32), aspect: f32, h: f32) -> PdfRect {
    let w = h * aspect;
    PdfRect::new(
        at.0 - w / 2.0,
        at.1 - h / 2.0,
        at.0 + w / 2.0,
        at.1 + h / 2.0,
    )
}

/// Maior retângulo com a proporção `aspect` dentro de `field`, centrado (campo de assinatura).
pub fn fit_in(field: PdfRect, aspect: f32) -> PdfRect {
    let (fw, fh) = (field.width(), field.height());
    let (w, h) = if fw / fh > aspect {
        (fh * aspect, fh)
    } else {
        (fw, fw / aspect)
    };
    let (cx, cy) = ((field.x0 + field.x1) / 2.0, (field.y0 + field.y1) / 2.0);
    PdfRect::new(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// Novo tamanho do texto com A− (`grow` falso) ou A+, nos passos do Acrobat (2 pt), com limites.
pub fn step_text_size(size: f32, grow: bool) -> f32 {
    let next = if grow { size + 2.0 } else { size - 2.0 };
    next.clamp(TEXT_SIZES.0, TEXT_SIZES.1)
}

/// Deslocamento das setas do teclado (pontos da página): 1 pt, ou 10 pt com Shift.
pub fn nudge(dir: (i8, i8), shift: bool) -> (f32, f32) {
    let step = if shift { 10.0 } else { 1.0 };
    (dir.0 as f32 * step, dir.1 as f32 * step)
}

/// Distância (pixels) até a qual pontos vizinhos são tremido do mouse; além dela, o ponto é um
/// canto desenhado de propósito e fica.
const JITTER: f32 = 6.0;

/// Média móvel de 3 pontos (tira o tremido do mouse), mantendo o começo, o fim e os cantos.
pub fn smooth(stroke: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if stroke.len() < 3 {
        return stroke.to_vec();
    }
    let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1) <= JITTER;
    let mut out = vec![stroke[0]];
    for w in stroke.windows(3) {
        out.push(if near(w[0], w[1]) && near(w[1], w[2]) {
            (
                (w[0].0 + w[1].0 + w[2].0) / 3.0,
                (w[0].1 + w[1].1 + w[2].1) / 3.0,
            )
        } else {
            w[1]
        });
    }
    out.push(stroke[stroke.len() - 1]);
    out
}

/// Traços normalizados: altura 1 (y para baixo), com margem, e a proporção. `None` sem tinta.
/// Um traço quase reto não vira uma faixa gigante: a altura usada é no mínimo 1/4 da largura.
pub fn normalize(strokes: &[Stroke]) -> Option<(Vec<Stroke>, f32)> {
    let pts = strokes.iter().filter(|s| s.len() >= 2).flatten();
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for &(x, y) in pts {
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    if b[0] > b[2] {
        return None;
    }
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let span = h.max(w / 4.0).max(1.0);
    let pad = span * 0.1;
    let scale = span + 2.0 * pad;
    let (x0, y0) = (b[0] - pad, b[1] - pad - (span - h) / 2.0);
    let out = strokes
        .iter()
        .filter(|s| s.len() >= 2)
        .map(|s| {
            s.iter()
                .map(|&(x, y)| ((x - x0) / scale, (y - y0) / scale))
                .collect()
        })
        .collect();
    Some((out, (w + 2.0 * pad) / scale))
}

/// Arte vetorial de traços já normalizados (espessura relativa à altura da arte).
pub fn drawn_art(strokes: &[Vec<(f32, f32)>], aspect: f32) -> SignatureArt {
    let paths = strokes
        .iter()
        .map(|s| VectorPath {
            cmds: s
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| {
                    if i == 0 {
                        PathCmd::Move(x, y)
                    } else {
                        PathCmd::Line(x, y)
                    }
                })
                .collect(),
            fill: false,
            width: 0.045,
        })
        .collect();
    SignatureArt::Vector { paths, aspect }
}

/// Traço do desenho: pontos em sequência.
pub type Stroke = Vec<(f32, f32)>;

/// Assinatura (ou iniciais) guardada para reutilizar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Saved {
    Typed {
        text: String,
    },
    /// Traços normalizados e proporção.
    Drawn {
        strokes: Vec<Vec<(f32, f32)>>,
        aspect: f32,
    },
    /// Cópia da imagem na pasta de configuração.
    Image {
        file: PathBuf,
    },
}

impl Saved {
    /// Arte para gravar; `None` se não der mais (imagem apagada, texto sem glifos).
    pub fn art(&self) -> Option<SignatureArt> {
        match self {
            Saved::Typed { text } => {
                let (paths, aspect) = pdf_core::signature::typed(text).ok()?;
                (!paths.is_empty()).then_some(SignatureArt::Vector { paths, aspect })
            }
            Saved::Drawn { strokes, aspect } => {
                (!strokes.is_empty()).then(|| drawn_art(strokes, *aspect))
            }
            Saved::Image { file } => file
                .exists()
                .then(|| SignatureArt::Image { path: file.clone() }),
        }
    }
}

/// Proporção largura/altura da arte.
pub fn aspect_of(art: &SignatureArt) -> f32 {
    match art {
        SignatureArt::Vector { aspect, .. } => *aspect,
        SignatureArt::Image { path } => pdf_core::signature::image_aspect(path).unwrap_or(1.0),
    }
}

/// Assinatura e iniciais guardadas (uma de cada, como no Acrobat).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedSignatures {
    pub signature: Option<Saved>,
    pub initials: Option<Saved>,
}

pub const FILE: &str = "assinaturas.json";

impl SavedSignatures {
    pub fn load(path: &Path) -> Self {
        crate::store::load_json(path)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::store::save_json(path, self)
    }

    pub fn get(&self, initials: bool) -> Option<&Saved> {
        if initials {
            self.initials.as_ref()
        } else {
            self.signature.as_ref()
        }
    }

    pub fn set(&mut self, initials: bool, saved: Option<Saved>) {
        *if initials {
            &mut self.initials
        } else {
            &mut self.signature
        } = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_centred_on_the_click_and_lines_are_wide() {
        let r = mark_rect(Mark::Check, (100.0, 50.0), 12.0);
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (94.0, 44.0, 106.0, 56.0));
        let l = mark_rect(Mark::Line, (100.0, 50.0), 12.0);
        assert_eq!(l.width(), LINE_WIDTH);
    }

    #[test]
    fn signatures_fit_inside_fields_keeping_proportions() {
        let f = PdfRect::new(0.0, 0.0, 200.0, 40.0);
        let r = fit_in(f, 2.0);
        assert_eq!((r.width(), r.height()), (80.0, 40.0));
        assert_eq!(r.x0, 60.0);
        let r = fit_in(f, 10.0);
        assert_eq!((r.width(), r.height()), (200.0, 20.0));
    }

    #[test]
    fn text_size_steps_and_arrows() {
        assert_eq!(step_text_size(12.0, true), 14.0);
        assert_eq!(step_text_size(6.0, false), 6.0);
        assert_eq!(step_text_size(72.0, true), 72.0);
        assert_eq!(nudge((1, 0), false), (1.0, 0.0));
        assert_eq!(nudge((0, -1), true), (0.0, -10.0));
    }

    #[test]
    fn smoothing_keeps_the_ends_and_softens_the_middle() {
        let s = smooth(&[(0.0, 0.0), (1.0, 3.0), (2.0, 0.0), (3.0, 3.0)]); // tremido
        assert_eq!(s.first(), Some(&(0.0, 0.0)));
        assert_eq!(s.last(), Some(&(3.0, 3.0)));
        assert!(s[1].1 < 3.0 && s[1].1 > 0.0);
    }

    #[test]
    fn smoothing_keeps_deliberate_corners() {
        // pontos esparsos (traço rápido): o vértice do "A" fica onde está
        let s = smooth(&[(0.0, 80.0), (30.0, 0.0), (60.0, 80.0)]);
        assert_eq!(s[1], (30.0, 0.0));
    }

    #[test]
    fn drawing_is_normalized_and_a_dot_is_not_ink() {
        let (n, aspect) = normalize(&[vec![(10.0, 10.0), (110.0, 60.0)]]).unwrap();
        let ys = n[0].iter().map(|p| p.1);
        assert!(ys.clone().all(|y| (0.0..=1.0).contains(&y)));
        assert!(aspect > 1.5 && aspect < 2.0, "{aspect}");
        // traço reto não vira faixa gigante
        let (_, flat) = normalize(&[vec![(0.0, 5.0), (400.0, 5.0)]]).unwrap();
        assert!(flat < 4.0, "{flat}");
        assert!(normalize(&[vec![(1.0, 1.0)]]).is_none());
        assert!(normalize(&[]).is_none());
    }

    #[test]
    fn saved_signatures_round_trip_and_survive_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let mut s = SavedSignatures::default();
        s.set(false, Some(Saved::Typed { text: "Ana".into() }));
        s.set(
            true,
            Some(Saved::Image {
                file: dir.path().join("sumiu.png"),
            }),
        );
        s.save(&path).unwrap();
        let back = SavedSignatures::load(&path);
        assert_eq!(back, s);
        assert!(back.get(false).unwrap().art().is_some());
        assert!(back.get(true).unwrap().art().is_none(), "imagem apagada");
        std::fs::write(&path, b"{lixo").unwrap();
        assert_eq!(SavedSignatures::load(&path), SavedSignatures::default());
    }
}
