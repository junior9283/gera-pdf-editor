# Gera PDF Editor — Subprojeto 1: Visualizador leve com anotações

Data: 2026-10-09 · Status: aguardando revisão

## 1. Objetivo

App desktop em Rust, **Linux**, chamado **Gera PDF Editor**. A longo prazo cobre os recursos do
Adobe Acrobat Reader; este spec trata apenas do **subprojeto 1: ler e anotar**.

Princípio central: **leve**. Os editores genéricos de PDF são pesados; este deve abrir rápido,
usar pouca memória e gerar um binário pequeno.

### Critérios de sucesso

| Métrica | Meta |
|---|---|
| Abertura (cold start até 1ª página visível) | < 300 ms |
| Memória com PDF típico de 100 páginas | < 150 MB, sem crescimento ilimitado ao rolar |
| Tamanho do binário (release) | < 25 MB, sem webview/runtime externo |
| Rolagem em PDFs grandes | sem travar a interface |

### Fora de escopo (subprojetos futuros)

Formulários (AcroForms), assinatura, organização de páginas (juntar/dividir/girar/reordenar),
edição de texto e imagens, OCR, impressão avançada. Windows e macOS.

### Decisões já tomadas

- Distribuição: uso pessoal / open source → licença **AGPL-3.0** (exigida pelo MuPDF).
- Plataforma: somente Linux; código escrito de forma portável.
- Nenhum código ou binário da Adobe é usado; o app é original.

## 2. Arquitetura

Workspace Cargo com três crates:

```
gera-pdf-editor/
├── crates/
│   ├── pdf-core/   # única crate que conhece o MuPDF
│   ├── app-state/  # modelo do documento, ferramentas, desfazer/refazer
│   └── gera-ui/    # egui/eframe (backend glow): janela, canvas, painéis
```

- **Motor:** crate `mupdf` 0.8 (bindings seguros do MuPDF), compilado estaticamente. Recursos
  não usados (JavaScript, EPUB/HTML, OCR/Tesseract etc.) desligados via feature flags.
- **UI:** `eframe` com backend `glow` (OpenGL), não `wgpu`, para reduzir binário e tempo de início.
- **`pdf-core`** expõe API própria e pequena: `open`, `render_page(n, zoom) -> Bitmap`, `search`,
  `text_in_rect`, `add_highlight / add_underline / add_note / add_ink`, `list_annotations`, `save`.
  Nenhum tipo do MuPDF vaza para as outras crates; o motor pode ser trocado ou simulado em testes.
- **`app-state`**: sem UI e sem MuPDF direto. Documento aberto, página/zoom, ferramenta ativa,
  pilha de desfazer/refazer, política do cache.
- **`gera-ui`**: desenha o estado e traduz mouse/teclado em ações.

## 3. Funcionalidades da v1

- Abrir por arrastar, linha de comando e "abrir com"; abas ou janelas por documento.
- Navegação: rolagem contínua, ir para página, zoom (largura/página), girar a visualização,
  índice (outline), miniaturas.
- Busca de texto com destaque; seleção e cópia de texto.
- Anotações: destacar, sublinhar, nota, desenho à mão livre; salvar no PDF; listar na barra lateral.
- Modo escuro para o fundo da página.

## 4. Fluxo de dados

```
UI (thread principal) ──pede "pág 12 @ 1.5x"──► RenderQueue ──► worker(s) pdf-core ──► Pixmap
        ▲                                                                                │
        └────────────── cache LRU de texturas (limite em MB) ◄───────────────────────────┘
```

- A UI nunca chama o MuPDF diretamente; envia pedidos e recebe resultados prontos.
- Renderiza só páginas visíveis e vizinhas; pedidos de páginas que saíram da tela são cancelados.
- Mostra primeiro versão em baixa resolução, substituída pela nítida quando pronta.
- Busca e extração de texto em outra thread, entregando resultados em lotes.
- Anotações ficam "pendentes" em memória e só entram no PDF ao salvar.

## 5. Erros e segurança

- Um tipo de erro por crate (`thiserror`); mensagens claras na UI (senha necessária, arquivo
  corrompido/reparo, sem permissão para salvar).
- Falha ao abrir um PDF fecha só aquele documento; o app nunca cai por causa de um PDF ruim.
- **Salvar com segurança:** grava em arquivo temporário na mesma pasta e renomeia; o original só
  é substituído se tudo der certo.
- PDF é entrada não confiável: JavaScript desligado na compilação, nenhuma ação/link embutido é
  executado automaticamente, limite de tamanho para decodificação em memória.

## 6. Testes

- **Unitários** (`app-state`): desfazer/refazer, zoom, ferramentas, política do cache LRU.
- **Integração** (`pdf-core`, fixtures pequenas em `tests/fixtures`): abrir, renderizar, buscar,
  criar anotação, salvar, reabrir e confirmar persistência.
- **Robustez:** PDF corrompido, protegido por senha e vazio falham de forma controlada, sem panic.
- **Desempenho:** teste que mede abertura, memória e tamanho do binário contra as metas.
- Sem testes visuais/pixel a pixel.

## 7. Empacotamento

- `cargo build --release` com `lto`, `codegen-units = 1`, `strip`, `panic = "abort"`.
- **AppImage** (`appimagetool`) com ícone e `.desktop` (associação com `.pdf`); Flatpak opcional depois.
- Licença AGPL-3.0 com avisos de terceiros. CI no GitHub Actions: fmt, clippy, testes, build.
- O repositório não contém material proprietário de terceiros.

## 8. Riscos e spikes antes do plano

1. Confirmar no `mupdf` 0.8 a criação de destaque, sublinhado, nota e tinta, e o salvamento incremental.
2. Medir o tamanho real do binário com os recursos desligados.
3. Confirmar que egui + glow mantém rolagem fluida em PDFs grandes.

Se o spike 1 falhar para algum tipo de anotação, esse tipo sai da v1 e vira item do próximo subprojeto.

## 9. Subprojetos seguintes (ordem proposta)

2. Organização de páginas · 3. Formulários e assinatura · 4. Edição avançada (texto/imagem, OCR).
