//! Preenchimento automático dos formulários (Preferências ▸ Formulários do Acrobat): as entradas
//! já digitadas voltam como sugestão ao começar a digitar. Fica nas preferências do usuário,
//! nunca no PDF.
use serde::{Deserialize, Serialize};

/// Maior lista guardada, maior entrada e quantas sugestões mostrar.
pub const MAX_ENTRIES: usize = 500;
const MAX_LEN: usize = 200;
const MAX_SUGGESTIONS: usize = 8;

/// Desligado, Básico (por prefixo, em qualquer campo) ou Avançado (também usa o nome do campo:
/// as entradas do mesmo campo vêm primeiro e aparecem já com o campo vazio).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AutofillMode {
    Off,
    #[default]
    Basic,
    Advanced,
}

impl AutofillMode {
    pub const ALL: [AutofillMode; 3] = [Self::Off, Self::Basic, Self::Advanced];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Desligado",
            Self::Basic => "Básico",
            Self::Advanced => "Avançado",
        }
    }
}

/// Uma entrada lembrada: o valor e o campo onde foi digitado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Autofill {
    pub mode: AutofillMode,
    /// "Lembrar dados numéricos".
    pub numbers: bool,
    /// A mais recente primeiro.
    pub entries: Vec<Entry>,
}

impl Autofill {
    /// Lembra o que foi digitado em `field`. Senha nunca; só números, só com a opção.
    pub fn learn(&mut self, field: &str, value: &str, password: bool) {
        let value = value.trim();
        if self.mode == AutofillMode::Off
            || password
            || value.is_empty()
            || value.chars().count() > MAX_LEN
            || value.contains(['\n', '\r'])
            || (!self.numbers && is_numeric(value))
        {
            return;
        }
        self.entries
            .retain(|e| !(e.value == value && e.field == field));
        self.entries.insert(
            0,
            Entry {
                field: field.chars().take(MAX_LEN).collect(),
                value: value.to_string(),
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    /// Sugestões para o que já foi digitado (sem repetir e sem o próprio texto).
    pub fn suggestions(&self, field: &str, typed: &str) -> Vec<&str> {
        let advanced = self.mode == AutofillMode::Advanced;
        if self.mode == AutofillMode::Off || (typed.is_empty() && !advanced) {
            return Vec::new();
        }
        let typed_low = typed.to_lowercase();
        let matches =
            |e: &&Entry| e.value != typed && e.value.to_lowercase().starts_with(&typed_low);
        // Avançado: primeiro as do mesmo campo; com o campo vazio, só elas
        let same = self.entries.iter().filter(|e| advanced && e.field == field);
        let rest = self
            .entries
            .iter()
            .filter(|e| !typed.is_empty() && !(advanced && e.field == field));
        let mut out: Vec<&str> = Vec::new();
        for e in same.chain(rest).filter(matches) {
            if !out.contains(&e.value.as_str()) {
                out.push(&e.value);
                if out.len() == MAX_SUGGESTIONS {
                    break;
                }
            }
        }
        out
    }

    /// Lista lida do arquivo: até o teto, sem entradas inválidas.
    pub fn sanitized(mut self) -> Self {
        self.entries.retain(|e| {
            let n = e.value.chars().count();
            n > 0 && n <= MAX_LEN && e.field.chars().count() <= MAX_LEN
        });
        self.entries.truncate(MAX_ENTRIES);
        self
    }
}

/// Só algarismos e sinais de número (telefone, CPF, valores).
fn is_numeric(v: &str) -> bool {
    v.chars().any(|c| c.is_ascii_digit())
        && v.chars()
            .all(|c| c.is_ascii_digit() || " .,-+/()$%R".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(mode: AutofillMode) -> Autofill {
        let mut a = Autofill {
            mode,
            ..Default::default()
        };
        a.learn("cidade", "Campinas", false);
        a.learn("nome", "Carla Souza", false);
        a.learn("nome", "Ana Lima", false);
        a
    }

    #[test]
    fn suggestions_by_prefix() {
        let a = filled(AutofillMode::Basic);
        assert_eq!(a.suggestions("x", "ca"), ["Carla Souza", "Campinas"]);
        assert_eq!(
            a.suggestions("x", "Campinas"),
            Vec::<&str>::new(),
            "nada igual ao digitado"
        );
        assert!(
            a.suggestions("nome", "").is_empty(),
            "Básico: só ao digitar"
        );
        // Avançado: as do mesmo campo primeiro, e já com o campo vazio
        let a = filled(AutofillMode::Advanced);
        assert_eq!(a.suggestions("cidade", "ca"), ["Campinas", "Carla Souza"]);
        assert_eq!(a.suggestions("nome", ""), ["Ana Lima", "Carla Souza"]);
        assert!(filled(AutofillMode::Off).suggestions("x", "ca").is_empty());
        // repetir sobe para o começo, sem duplicar
        let mut a = filled(AutofillMode::Basic);
        a.learn("nome", "Carla Souza", false);
        assert_eq!(a.entries.len(), 3);
        assert_eq!(a.entries[0].value, "Carla Souza");
    }

    #[test]
    fn passwords_and_numbers() {
        let mut a = Autofill::default();
        a.learn("senha", "segredo123", true);
        a.learn("cpf", "123.456.789-00", false);
        a.learn("x", "   ", false);
        assert!(a.entries.is_empty());
        a.numbers = true;
        a.learn("cpf", "123.456.789-00", false);
        a.learn("senha", "segredo123", true);
        assert_eq!(a.entries.len(), 1);
        let mut off = Autofill {
            mode: AutofillMode::Off,
            ..Default::default()
        };
        off.learn("nome", "Ana", false);
        assert!(off.entries.is_empty());
    }

    #[test]
    fn list_is_capped() {
        let mut a = Autofill::default();
        for k in 0..MAX_ENTRIES + 50 {
            a.learn("f", &format!("valor {k}"), false);
        }
        assert_eq!(a.entries.len(), MAX_ENTRIES);
        assert_eq!(a.entries[0].value, format!("valor {}", MAX_ENTRIES + 49));
        a.learn("f", &"x".repeat(MAX_LEN + 1), false);
        assert_eq!(a.entries[0].value, format!("valor {}", MAX_ENTRIES + 49));
        assert_eq!(a.suggestions("f", "valor").len(), MAX_SUGGESTIONS);
        let mut big = a.clone();
        big.entries.extend(a.entries.clone());
        assert_eq!(big.sanitized().entries.len(), MAX_ENTRIES);
    }
}
