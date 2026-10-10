# Gera PDF Editor — Subprojeto 3b: Preencher e assinar

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Preencher e assinar** do Acrobat: preencher formulários "planos" (sem campos) com
texto e marcas e assinar com assinatura e iniciais desenhadas, digitadas ou de imagem.

Referência de comportamento (Adobe Help, "Fill and sign PDF forms"): barra com **Adicionar texto**,
**Xis**, **Visto**, **Círculo**, **Linha**, **Ponto** e **Assinar** (Adicionar assinatura / Adicionar
iniciais). Clicar na página coloca o item; a marca seguinte usa o tamanho da última redimensionada;
setas do teclado movem o item selecionado; a assinatura é feita uma vez (Digitar, Desenhar ou Imagem),
fica guardada para reutilizar e é colocada com um clique.

### Critérios de sucesso

- Central de Ferramentas: "Preencher e assinar" pronta; abre a barra da ferramenta (na linha de
  ferramentas, como Comentar) e a casca mostra o botão **Assinar** no alto ativo.
- **Adicionar texto**: clique cria texto sem borda (tamanho padrão 12 pt, preto) e já entra em edição;
  **A−/A+** na barra mudam o tamanho do texto selecionado (e o padrão dos próximos).
- **Marcas**: Xis, Visto, Linha (traços vetoriais), Círculo (contorno) e Ponto (círculo cheio), pretos;
  clique coloca com o tamanho atual da marca (padrão 12 pt; a última marca redimensionada define o
  próximo tamanho; linha padrão 40 pt de largura).
- **Assinar**: menu com a assinatura e as iniciais guardadas (clique para colocar) e "Adicionar
  assinatura…"/"Adicionar iniciais…" (ou "Alterar…" quando já existe) e "Apagar assinatura guardada".
- **Janela de assinatura** com três abas: **Digitar** (fonte manuscrita, nome da Identidade como
  sugestão), **Desenhar** (traço com o mouse, Limpar) e **Imagem** (escolher PNG/JPEG); prévia; "Aplicar"
  guarda e entra no modo de colocar (o item segue o mouse como prévia; clique coloca; Esc cancela).
- A assinatura no PDF é **vetorial** (digitada e desenhada) — nítida em qualquer zoom e na impressão,
  como no Acrobat — ou a imagem escolhida. Mover e redimensionar mantêm as proporções.
- Clicar num **campo de assinatura** (v3a) coloca a assinatura guardada ajustada ao campo (ou abre a
  janela se não houver).
- Tudo com desfazer/refazer e Salvar; itens são selecionáveis, movíveis (inclusive com setas: 1 pt,
  Shift = 10 pt) e apagáveis como as demais anotações.
- Assinatura e iniciais guardadas ficam só no computador do usuário (pasta de configuração, como o
  restante das preferências).

### Fora de escopo

Assinatura digital com certificado (v3d), "Solicitar assinaturas" por e-mail/nuvem, achatar
("Salvar como cópia somente leitura" fica para v3d/v5), reconhecimento automático de campos em
formulários planos.

## 2. Decisões

- **Formato no arquivo**: texto = FreeText sem borda (`/IT /FreeTextTypeWriter`, igual ao
  Typewriter do Acrobat); Xis/Visto/Linha = Ink; Círculo = Circle; Ponto = Circle com
  preenchimento; assinatura/iniciais = **Stamp** com aparência vetorial própria (form XObject com os
  caminhos) ou imagem; `/Subj (Assinatura)`/`(Iniciais)` e `/Name /GeraSignature` para reconhecer.
- **Fonte manuscrita**: Great Vibes (SIL OFL 1.1, embutida no binário; licença em
  `crates/pdf-core/fonts/OFL.txt`). Os glifos viram contornos (sem fonte embutida no PDF).
- **Desenho**: traços suavizados (média móvel de 3 pontos), espessura 2 pt na escala da caixa,
  pontas e junções arredondadas.
- **Guardar**: `assinaturas.json` na pasta de configuração com os caminhos vetoriais normalizados
  (0..1 na largura) ou o caminho de uma cópia da imagem na mesma pasta.

## 3. Arquitetura

- **pdf-core**: `VectorPath { points: Vec<PathCmd>, fill: bool, width: f32 }`
  (`PathCmd::Move/Line/Curve/Close`), `NewAnnot::Signature { page, rect, art: SignatureArt, initials }`
  com `SignatureArt::Vector { paths, aspect }` | `Image { path }`; `NewAnnot::FillText { page, at,
  text, size }`; `signature::typed(text) -> (Vec<VectorPath>, aspect)` com a fonte embutida.
  Marcas usam `NewAnnot::Ink` e `NewAnnot::Shape` (+ `AnnotOp::Fill` para o ponto, no mesmo passo).
- **app-state**: `fill_sign.rs` — `FillTool`, geometria das marcas (`mark_strokes`), suavização,
  normalização dos traços e o arquivo de assinaturas (`SavedSignatures`).
- **gera-ui**: `fill_sign.rs` (barra, colocação com prévia), `shell/signature_dialog.rs` (janela),
  setas movendo a seleção em `edit.rs`.

## 4. Testes

- pdf-core: assinatura vetorial criada, salva e reaberta (Stamp com /AP próprio, pixels escuros na
  área ao renderizar); imagem; texto digitado gera caminhos e proporção cresce com o texto; FillText
  sem borda com typewriter; mover/redimensionar mantém a aparência; desfazer.
- app-state: geometria das marcas dentro do retângulo; suavização; ida e volta do arquivo de
  assinaturas (e arquivo corrompido = vazio).
- E2E (Xvfb): abrir PDF, Preencher e assinar, texto, xis, visto, assinatura digitada colocada,
  salvar, reabrir e conferir.
