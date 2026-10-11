//! Proteger PDF (como no Acrobat): senha para abrir, senha de permissões com restrições
//! (imprimir, copiar, alterar…), remover a segurança, e as permissões de quem abriu o documento.
use crate::doc::PdfDoc;
use crate::error::{PdfError, Result};
use mupdf::pdf::{Encryption, PdfWriteOptions, Permission};

/// O que o documento deixa fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub print: bool,
    /// Imprimir em alta resolução (sem ele, só baixa).
    pub print_hq: bool,
    /// Copiar texto e imagens (e extrair páginas).
    pub copy: bool,
    /// Alterar o documento (organizar, editar).
    pub modify: bool,
    /// Comentar.
    pub annotate: bool,
    /// Preencher formulários e assinar.
    pub forms: bool,
    /// Montar: inserir, excluir, girar páginas.
    pub assemble: bool,
}

impl Permissions {
    pub const ALL: Permissions = Permissions {
        print: true,
        print_hq: true,
        copy: true,
        modify: true,
        annotate: true,
        forms: true,
        assemble: true,
    };

    /// Do /P do dicionário /Encrypt (bits da especificação: 3 imprimir, 4 alterar, 5 copiar,
    /// 6 comentar, 9 formulários, 11 montar, 12 imprimir em alta).
    fn from_p(p: i32) -> Self {
        let bit = |n: u32| p & (1 << (n - 1)) != 0;
        Self {
            print: bit(3),
            print_hq: bit(3) && bit(12),
            copy: bit(5),
            modify: bit(4),
            annotate: bit(6),
            forms: bit(9) || bit(6),
            assemble: bit(11) || bit(4),
        }
    }

    fn to_mupdf(self) -> Permission {
        // acessibilidade sempre liberada (o Acrobat também liga por padrão)
        let mut p = Permission::ACCESSIBILITY;
        for (on, bit) in [
            (self.print, Permission::PRINT),
            (self.print && self.print_hq, Permission::PRINT_HQ),
            (self.copy, Permission::COPY),
            (self.modify, Permission::MODIFY),
            (self.annotate, Permission::ANNOTATE),
            (self.forms || self.annotate, Permission::FORM),
            (self.assemble, Permission::ASSEMBLE),
        ] {
            if on {
                p |= bit;
            }
        }
        p
    }
}

/// Proteção a aplicar ao salvar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    /// Senha para abrir (vazia: abre sem pedir).
    pub open_password: String,
    /// Senha de permissões (obrigatória se houver restrição).
    pub owner_password: String,
    pub permissions: Permissions,
}

/// Situação de segurança do documento aberto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityInfo {
    pub encrypted: bool,
    pub needs_user_password: bool,
    /// Permissões de quem abriu (todas, se não está protegido ou foi desbloqueado).
    pub permissions: Permissions,
    /// A senha de permissões foi informada.
    pub unlocked: bool,
}

impl SecurityInfo {
    /// Protegido com restrições e sem a senha de permissões informada.
    pub fn locked(&self) -> bool {
        self.encrypted && !self.unlocked && self.permissions != Permissions::ALL
    }
}

impl Default for SecurityInfo {
    /// Documento sem proteção: tudo permitido.
    fn default() -> Self {
        Self {
            encrypted: false,
            needs_user_password: false,
            permissions: Permissions::ALL,
            unlocked: false,
        }
    }
}

fn err(msg: &str) -> PdfError {
    PdfError::InvalidPages(msg.into())
}

impl PdfDoc {
    fn is_encrypted(&self) -> Result<bool> {
        Ok(self.inner.trailer()?.get_dict("Encrypt")?.is_some())
    }
}

/// O arquivo de outro programa pode ter as páginas copiadas (Combinar, Inserir)? Sem restrição de
/// montar e copiar, ou com a senha de permissões.
pub(crate) fn pages_can_be_copied(
    doc: &mupdf::pdf::PdfDocument,
    password: Option<&str>,
) -> Result<bool> {
    let Some(e) = crate::owner_password::encrypt_dict(doc)? else {
        return Ok(true);
    };
    if password.is_some_and(|p| !p.is_empty() && crate::owner_password::is_owner_password(&e, p)) {
        return Ok(true);
    }
    let p = Permissions::from_p(e.p);
    Ok(p.copy && p.assemble)
}

/// Erro de quem tenta copiar páginas de um arquivo restrito.
pub(crate) fn restricted_source(path: &std::path::Path) -> PdfError {
    err(&format!(
        "o arquivo {} está protegido contra cópia de páginas: informe a senha de permissões",
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    ))
}

impl PdfDoc {
    /// Permissões gravadas no arquivo (o `permissions()` do binding cai em "todas" quando o /P tem
    /// os bits reservados ligados: o `from_bits` dele falha com bits desconhecidos).
    fn file_permissions(&self) -> Result<Permissions> {
        let p = self
            .inner
            .trailer()?
            .get_dict("Encrypt")?
            .and_then(|e| e.get_dict("P").ok().flatten())
            .map(|p| p.as_int())
            .transpose()?;
        Ok(p.map_or(Permissions::ALL, Permissions::from_p))
    }

    pub fn security(&self) -> Result<SecurityInfo> {
        let encrypted = self.is_encrypted()?;
        let permissions = if !encrypted || self.owner_unlocked {
            Permissions::ALL
        } else {
            self.file_permissions()?
        };
        Ok(SecurityInfo {
            encrypted,
            // aberto com senha não vazia = o arquivo pede senha de usuário. Não perguntar ao
            // MuPDF (`needs_password` testa a senha vazia, e a tentativa que falha estraga a chave
            // da cifra: os textos gravados depois saíam ilegíveis)
            needs_user_password: encrypted
                && self.password.as_deref().is_some_and(|p| !p.is_empty()),
            permissions,
            unlocked: encrypted && self.owner_unlocked,
        })
    }

    /// Informa a senha de permissões: libera tudo. Conferida pelos algoritmos da especificação
    /// (a senha de abrir, ou uma variante dela que o MuPDF aceite, não serve).
    pub fn unlock(&mut self, owner_password: &str) -> Result<()> {
        let Some(e) = crate::owner_password::encrypt_dict(&self.inner)? else {
            return Ok(());
        };
        if owner_password.is_empty()
            || !crate::owner_password::is_owner_password(&e, owner_password)
        {
            return Err(PdfError::WrongPassword);
        }
        self.owner_unlocked = true;
        Ok(())
    }

    /// Proteção (ou `None`: remover a segurança) aplicada no próximo Salvar.
    pub fn set_protection(&mut self, protection: Option<Protection>) -> Result<()> {
        if self.signed_on_disk || self.has_signatures()? {
            return Err(err(
                "o documento está assinado: mudar a segurança invalidaria as assinaturas",
            ));
        }
        let restricted = self.is_encrypted()? && self.file_permissions()? != Permissions::ALL;
        if restricted && !self.owner_unlocked {
            return Err(err("informe a senha de permissões para mudar a segurança"));
        }
        if let Some(p) = &protection {
            if p.open_password.is_empty() && p.owner_password.is_empty() {
                return Err(err("informe uma senha"));
            }
            if p.permissions != Permissions::ALL && p.owner_password.is_empty() {
                return Err(err("as restrições precisam de uma senha de permissões"));
            }
            if !p.owner_password.is_empty() && p.owner_password == p.open_password {
                return Err(err(
                    "a senha de permissões tem de ser diferente da de abrir",
                ));
            }
        }
        self.pending_protection = Some(protection);
        self.mark_unsaved();
        Ok(())
    }

    /// Opções de criptografia da gravação: a proteção pendente, ou manter a do arquivo.
    pub(crate) fn encryption_options(&self, opts: &mut PdfWriteOptions) {
        match &self.pending_protection {
            None => {
                opts.set_encryption(Encryption::Keep);
            }
            Some(None) => {
                opts.set_encryption(Encryption::None);
            }
            Some(Some(p)) => {
                // sem senha de permissões, a de abrir faz as vezes (nunca vazia: daria dono a todos)
                let owner = if p.owner_password.is_empty() {
                    &p.open_password
                } else {
                    &p.owner_password
                };
                opts.set_encryption(Encryption::Aes256)
                    .set_user_password(&p.open_password)
                    .set_owner_password(owner)
                    .set_permissions(p.permissions.to_mupdf());
            }
        }
    }

    /// Depois de gravar com a proteção pendente: o documento passa a ser o do disco, com a senha
    /// nova.
    pub(crate) fn after_protection_saved(&mut self) -> Result<()> {
        let Some(applied) = self.pending_protection.take() else {
            return Ok(());
        };
        match applied {
            Some(p) => {
                self.password = (!p.open_password.is_empty()).then_some(p.open_password);
                self.owner_unlocked = true; // quem protegeu sabe a senha de permissões
            }
            None => {
                self.password = None;
                self.owner_unlocked = false;
            }
        }
        self.discard_changes()
    }

    /// Há proteção (ou remoção) esperando o Salvar.
    pub fn protection_pending(&self) -> bool {
        self.pending_protection.is_some()
    }
}
