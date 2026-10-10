# Gera PDF Editor — Subprojeto 5a: Proteger PDF

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Proteger PDF** do Acrobat (proteger com senha, restringir edição, remover segurança)
e respeitar as permissões de documentos protegidos — as duas pendências anotadas ("não imprimir",
"extração não permitida").

### Critérios de sucesso

- **Proteger usando senha** (janela): senha para **abrir** (opcional) e senha de **permissões**
  (opcional; obrigatória se houver restrição); impressão (não permitida / baixa resolução / alta),
  alterações (nenhuma / preencher e assinar / comentar, preencher e assinar / tudo menos extrair
  páginas), permitir copiar texto e imagens; criptografia AES-256 (Acrobat 9+). Vale ao Salvar
  (como no Acrobat); a barra de status avisa "a proteção será aplicada ao salvar".
- **Remover segurança**: grava sem criptografia (exige a senha de permissões se o documento tem
  restrições e ela não foi informada).
- **Respeitar permissões** de quem abriu sem a senha de permissões: Imprimir, Copiar, Organizar
  (inserir/excluir/girar/recortar/extrair/dividir), Comentar, Preencher e assinar, Preparar
  formulário e Certificados ficam indisponíveis conforme as permissões, com o motivo na dica e na
  barra de status. **Desbloquear** com a senha de permissões libera tudo.
- Documento assinado: mudar a segurança é recusado (a regravação quebraria as assinaturas).

### Decisão: reconhecer a senha de permissões

O MuPDF do binding só diz se a senha foi aceita (não se foi a de usuário ou a de dono). Regra:
sem senha de abrir, a senha aceita só pode ser a de permissões; com senha de abrir, a senha de
permissões precisa ser diferente da de abrir e ser aceita.

## 2. Arquitetura

- **pdf-core** `security.rs`: `Permissions { print, print_hq, copy, modify, annotate, forms,
  assemble }`, `SecurityInfo { encrypted, needs_user_password, permissions, unlocked }`,
  `Protection { open_password, owner_password, permissions }`; `PdfDoc::security()`,
  `unlock(owner)`, `set_protection(Some|None)` (pendente até Salvar; `write()` usa).
  `DocSummary.permissions`.
- **gera-ui**: janela Proteger, linha Proteger PDF (Proteger usando senha, Remover segurança,
  Desbloquear), bloqueios por permissão.

## 3. Testes

- pdf-core: proteger e reabrir (sem senha falha; com a de abrir ok, sem imprimir); desbloquear com
  a de permissões; senha errada recusa; remover segurança; documento assinado recusa; só senha de
  permissões (abre sem senha, com restrições).
- E2E: proteger com restrição de impressão, salvar, reabrir, Imprimir indisponível.
