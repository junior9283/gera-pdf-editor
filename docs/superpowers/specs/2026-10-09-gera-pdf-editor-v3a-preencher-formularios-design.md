# Gera PDF Editor — Subprojeto 3a: Preencher formulários

Data: 2026-10-09 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

Preencher formulários PDF (AcroForm) como no Acrobat: o documento com campos mostra a barra roxa
"Este documento contém campos de formulário" com **Realçar campos** e **Limpar formulário**; os
campos ficam realçados; clicar edita; Tab/Shift+Tab percorre; o valor vai para o arquivo ao Salvar.

### Critérios de sucesso

- **Texto**: clicar abre a edição no lugar (uma ou várias linhas, senha mascarada, limite de
  caracteres); Enter confirma (campo de uma linha), Esc cancela, Tab confirma e vai para o próximo.
- **Caixa de seleção**: clicar marca/desmarca. **Botões de opção**: clicar escolhe um do grupo.
- **Lista suspensa** e **lista**: clicar abre as opções (rótulos de exibição) e escolher grava o
  valor de exportação.
- Campos somente leitura não editam; campos obrigatórios têm borda vermelha no realce.
- **Limpar formulário** volta os valores padrão (com confirmação na barra).
- Desfazer/refazer de cada valor; o que aparece é o que vai para o arquivo (aparência do MuPDF).
- Campos de formulário não aparecem como comentários (lista, painel Comentários, seleção).
- Campos de assinatura mostram "Assinar" (a assinatura chega na v3b/v3d; aqui o status explica).

### Fora de escopo

Cálculos/validações em JavaScript, formulários XFA (aviso na barra), criar campos (v3c),
assinatura (v3b/v3d).

## 2. Arquitetura

- **pdf-core**: `FieldInfo { id, page, name, kind: FieldKind, rect, value, readonly, required,
  tooltip }`, `FieldKind { Text { multiline, password, max_len }, Checkbox { on }, Radio { on },
  Combo { options, editable }, List { options }, Signature, Button }`; `PdfDoc::fields(page)`,
  `set_field(page, id, value)` (histórico `Step::Field` com o valor anterior), `reset_form()`
  (instantâneo); `annotations()` ignora widgets; `DocSummary.fields`.
- **worker**: `Request::SetField { page, id, value }`, `Request::ResetForm`;
  `Response::FieldsChanged { page, fields, history }`.
- **gera-ui**: `fields.rs` — realce, hit-test, edição de texto no lugar, menus de opções, ordem de
  Tab; barra roxa na aba.

## 3. Testes

- pdf-core: listar os seis campos do formulário de amostra com tipo e valor; preencher cada tipo,
  salvar, reabrir e conferir; desfazer volta o valor anterior; limpar formulário; widgets fora da
  lista de anotações; somente leitura recusado.
- gera-ui (puro): ordem de Tab, máscara de senha e limite de caracteres.
- E2E (Xvfb): abrir o formulário de amostra, preencher texto, marcar a caixa, escolher opção e
  item da lista suspensa, Tab, salvar e reabrir.
