# Gera PDF Editor — Subprojeto 1.1-B2a: Anotações editáveis (modelo novo + edição)

Data: 2026-10-09 · Status: aguardando revisão

## 1. Objetivo

Tornar as anotações **objetos editáveis**, como no Acrobat Pro DC, inclusive as já salvas no PDF
e as criadas por outros programas: selecionar, mover, redimensionar, apagar, mudar cor, opacidade
e espessura, editar o texto de notas — com desfazer/refazer. Isto troca o modelo atual
("pendentes desenhadas pela interface e aplicadas ao salvar") por **edição direta no documento em
memória**, base para os tipos novos da v1.1-B2b (caixa de texto, formas, linha/seta, carimbos,
respostas).

### Critérios de sucesso

- Com a ferramenta Selecionar, **clicar** numa anotação a seleciona (contorno com alças);
  clicar fora tira a seleção.
- **Arrastar** uma nota ou um desenho move; as **alças** redimensionam o desenho (os traços
  acompanham a escala). Destaque, sublinhado e riscado ficam presos ao texto (não se movem nem
  redimensionam), como no Acrobat.
- **Delete** (ou menu de contexto → Excluir) apaga a anotação selecionada.
- **Menu de contexto** sobre uma anotação: Propriedades… · Excluir. **Propriedades** abre uma
  barra/janela com **cor** (paleta), **opacidade** (0–100 %) e **espessura** (desenho), aplicadas na
  hora.
- **Duplo clique** numa nota abre a edição do texto.
- **Ctrl+Z / Ctrl+Shift+Z** desfazem/refazem criar, mover, redimensionar, estilo, texto e apagar —
  inclusive de anotações que vieram de outros programas, sem perder nada delas.
- O que aparece na tela é o que vai para o arquivo (a aparência vem do MuPDF, não de desenho
  próprio da interface).
- Salvar grava o documento em memória (com a gravação segura e a criptografia mantida da v1).
- Metas da v1 continuam (abertura < 300 ms, memória < 150 MB, binário < 25 MB).

### Fora de escopo (v1.1-B2b)

Tipos novos (caixa de texto, retângulo, elipse, linha, seta, carimbos), respostas a comentários,
bloquear anotação, autor/data editáveis, propriedades padrão por tipo.

## 2. Arquitetura

### Modelo (pdf-core)

- Cada anotação ganha **identidade estável**: o número do objeto PDF (`AnnotId(i32)`), mais a
  página. `AnnotInfo` passa a ter `id`, `kind`, `page`, `rect`, `color` (RGB), `opacity`,
  `width` (espessura, quando houver), `contents`, `movable`, `resizable`.
- **Operações** aplicadas imediatamente no documento em memória:
  `Create(NewAnnot)` · `Move { id, dx, dy }` · `Resize { id, rect }` ·
  `Style { id, color, opacity, width }` · `SetContents { id, text }` · `Delete { id }`.
- **Histórico próprio** no worker (`History`): cada operação aplicada empilha a sua **inversa**:
  - Create → apagar de verdade a anotação criada (é nossa, pode ser recriada no refazer com os
    mesmos dados);
  - Move/Resize/Style/SetContents → os valores anteriores;
  - **Delete → só marca a anotação como escondida** (flag *Hidden* do PDF) e guarda o id num
    conjunto "apagadas"; desfazer tira a marca — a anotação volta **idêntica**, mesmo que venha de
    outro programa. Ao **salvar**, as "apagadas" são removidas de verdade.
  - Refazer reaplica a operação original.
- Anotações escondidas não aparecem na tela nem na impressão (o MuPDF não desenha *Hidden*);
  as que já eram *Hidden* no arquivo original não são tocadas.
- **Mover/redimensionar desenho (Ink):** transforma os pontos da lista de traços (translação ou
  escala do retângulo antigo para o novo) e o `Rect`. Nota (Text): move o `Rect` (tamanho fixo).
- **Sujo (dirty):** há alterações desde a última gravação (histórico com passos depois da marca
  de "salvo"). Salvar limpa o histórico (o estado salvo vira a nova base).
- **Impressão:** a cópia temporária passa a sair do documento em memória (já com as edições; as
  apagadas, escondidas, não aparecem); "Documento" (sem marcações) continua removendo todas.

### Protocolo do worker

- `Request::Annot(AnnotOp)`, `Request::Undo`, `Request::Redo`, `Request::Save { dest }` (sem
  pendentes), `Request::ExportForPrint { dest, with_annots }` (sem pendentes).
- Resposta única de mudança: `Response::AnnotsChanged { annots: Vec<AnnotInfo>, pages: Vec<i32>,
  can_undo: bool, can_redo: bool, dirty: bool }` — `pages` são as páginas a renderizar de novo;
  `annots` é a lista completa (visíveis), para o painel e a seleção.
- Falha numa operação: `Response::AnnotFailed(String)` (nada muda; status mostra o erro).

### Interface (gera-ui)

- `EditState` fica só com ferramenta, gestos e seleção (de texto e de **anotação**: `selected:
  Option<AnnotId>`). As anotações "pendentes" e o desenho delas pela interface saem (a página
  re-renderizada já mostra o resultado). Durante um arrasto, a interface desenha só a
  **pré-visualização** do gesto (contorno da anotação sendo movida/redimensionada, traço sendo
  desenhado).
- `app-state`: `PendingAnnots` sai; entra `annot_hit(annots, page, point) -> Option<AnnotId>`
  (a de cima que contém o ponto, com tolerância) e `handle_at(rect, point, zoom) ->
  Option<Handle>` (alças nos cantos), puros e testados.
- Ao receber `AnnotsChanged`: atualiza lista, `can_undo/can_redo/dirty`, invalida o cache das
  `pages` (re-render) e mantém a seleção se o id ainda existir.
- **Barra de propriedades**: aparece com uma anotação selecionada (na linha "Comentar" ou como
  janela "Propriedades"): cor (8 cores do Acrobat: amarelo, vermelho, laranja, verde, azul, roxo,
  preto, cinza), opacidade (slider) e espessura (desenho: 1–12 pt).
- Painel Anotações: lista do worker; clicar seleciona e leva à anotação.
- Fechar aba/app: "precisa confirmar" passa a usar o `dirty` do worker.

## 3. Erros

- Operação sobre id inexistente (anotação removida por outra operação): `AnnotFailed`, status
  informa, lista é reenviada.
- Falha ao salvar: como na v1 (documento recarregado do disco, histórico descartado, status
  com o erro).
- Anotação de tipo desconhecido (de outro programa): selecionável, apagável e com cor/opacidade
  quando o MuPDF permitir; não movível/redimensionável.

## 4. Testes

- **pdf-core (unitários/integração):** cada operação + desfazer + refazer deixa o documento igual
  (rect, cor, opacidade, contents, número de anotações); apagar → desfazer de anotação "externa"
  (criada direto no PDF de teste com propriedades extras) volta idêntica; salvar remove as
  apagadas e mantém as outras; mover/redimensionar ink transforma os pontos; dirty/can_undo após
  cada passo; impressão sai do documento em memória.
- **app-state:** `annot_hit` (sobreposição, tolerância, fora), `handle_at` (cada canto, zoom).
- **E2E (Xvfb, pelo Claude):** selecionar nota e mover; desenhar e redimensionar pela alça;
  apagar com Delete e desfazer; mudar cor/opacidade de um destaque; duplo clique editar nota;
  salvar, reabrir e conferir; fechar com alterações pergunta.
