# Gera PDF Editor — Subprojeto 3d: Certificados (assinatura digital)

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Certificados** do Acrobat: assinar digitalmente com um ID digital (certificado
X.509 + chave privada em PKCS#12), validar as assinaturas de um documento e ver a situação de cada uma
no painel **Assinaturas**, com o mesmo formato que o Acrobat lê e valida.

### Critérios de sucesso

- **IDs digitais**: importar um arquivo `.pfx`/`.p12` (senha pedida ao usar, nunca guardada) e
  **criar um ID digital autoassinado** (nome, e-mail, organização; RSA 2048; gravado como `.pfx` com
  a senha escolhida na pasta de configuração). A lista de IDs fica nas preferências (só caminhos e o
  nome do titular).
- **Assinar digitalmente** (linha Certificados): arrastar um retângulo na página ou clicar num
  campo de assinatura vazio → janela "Assinar com um ID digital" (ID, senha, motivo, local, prévia da
  aparência) → **Assinar** grava o documento (pendências antes, de forma incremental) com a
  assinatura. Aparência como a do Acrobat: nome grande à esquerda; à direita "Assinado digitalmente
  por <nome>", "Data: AAAA.MM.DD hh:mm:ss -03'00'", motivo e local quando houver.
- Formato: `/Filter /Adobe.PPKLite`, `/SubFilter /adbe.pkcs7.detached`, CMS SignedData destacado com
  atributos assinados (content-type, message-digest SHA-256, signing-time) e a cadeia do certificado;
  `/ByteRange` cobrindo o arquivo todo menos `/Contents`.
- **Salvar um PDF assinado** passa a ser incremental (como o Acrobat), para não invalidar
  assinaturas existentes; o mesmo vale para os documentos assinados de outros programas.
- **Validar**: para cada assinatura — integridade (resumo do intervalo confere com o assinado e a
  assinatura RSA/ECDSA confere com a chave do certificado), se o documento foi alterado depois
  (revisões posteriores) e a identidade (confiável se o certificado — ou um da cadeia — está na lista
  de confiança do usuário; senão "desconhecida", como o Acrobat sem confiança configurada).
- **Painel Assinaturas** (trilho à esquerda): cada assinatura com ícone de situação (válida,
  desconhecida, inválida), titular, data, motivo, local, "alterado depois de assinado", e a ação
  **Confiar neste certificado**. **Barra azul** no topo de documentos assinados: "Assinado e todas as
  assinaturas são válidas" / "Pelo menos uma assinatura tem problemas" / "Validade da assinatura
  desconhecida", com o botão **Painel de assinaturas**.

### Fora de escopo

Carimbo de tempo (TSA), LTV/DSS, certificação com permissões MDP, tokens/cartões (PKCS#11),
revogação (OCSP/CRL), assinatura com ECDSA de IDs criados aqui (importados ECDSA P-256 assinam).

## 2. Arquitetura

- **pdf-core** `digital_id.rs`: `DigitalId { name, email, cert_der, chain, key }` lido de PKCS#12
  (`p12-keystore`); `create_self_signed(name, email, org, password, path)`.
- **pdf-core** `sign.rs`: `PdfDoc::sign_digitally(page, rect | field id, &DigitalId, reason,
  location)` — campo de assinatura, dicionário `/Sig` com reserva de 16 KB em `/Contents`, gravação
  incremental, `/ByteRange` corrigido no arquivo, CMS (`cms` crate) e escrita do `/Contents`.
- **pdf-core** `verify.rs`: `PdfDoc::signatures() -> Vec<SignatureInfo>` com `Integrity`,
  `modified_after`, titular, data, motivo, local, impressão digital SHA-256 do certificado.
- Salvar: incremental quando o documento tem assinaturas.
- **app-state**: lista de IDs (`digital_ids.json`) e lista de confiança (`trusted_certs.json`).
- **gera-ui**: linha Certificados, janela Assinar, janela Criar ID, painel Assinaturas e barra azul.

## 3. Testes

- pdf-core: criar ID autoassinado e reabrir com a senha (senha errada falha); assinar, reabrir e
  validar (íntegra; identidade desconhecida; confiável com a impressão digital na lista); alterar um
  byte assinado → inválida; editar e salvar depois → a assinatura continua íntegra e "alterado depois
  de assinado"; segunda assinatura mantém a primeira válida; PDF protegido por senha;
  `/Contents` grande demais recusado.
- E2E: criar ID, assinar, ver painel e barra.
