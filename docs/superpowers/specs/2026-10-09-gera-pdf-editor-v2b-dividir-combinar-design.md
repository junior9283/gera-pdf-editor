# Gera PDF Editor — Subprojeto 2b: Dividir PDF e Combinar arquivos

Data: 2026-10-09 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

As ferramentas **Dividir PDF** e **Combinar arquivos** do Acrobat Pro.

### Critérios de sucesso

- **Dividir PDF** (hub ou Organizar páginas → Dividir): diálogo "Dividir documento" com
  **Dividir por**: número de páginas (N por arquivo), tamanho máximo do arquivo (MB) ou marcadores
  de primeiro nível; **pasta de saída** (padrão: a do documento; "Escolher pasta…") e **nome**:
  `<nome>_Parte1.pdf`, `_Parte2`… (nomes que já existem ganham " (2)"). Status: "N arquivos criados
  em <pasta>". O documento aberto não muda.
- Partes levam páginas, comentários e os marcadores que caem nelas; cada parte que começa por um
  marcador de primeiro nível leva o título dele no nome (`<nome>_<Título>.pdf`, caracteres
  inválidos trocados por "_").
- **Combinar arquivos** (hub): aba "Combinar arquivos" com a lista (nome, páginas, tamanho),
  **Adicionar arquivos…**, **Mover para cima/baixo**, **Remover**, senha quando o arquivo pedir;
  **Combinar** pergunta onde salvar e abre o resultado numa aba nova.
- O arquivo combinado tem um marcador por arquivo (título = nome do arquivo) com os marcadores
  originais dentro, comentários preservados; campos de formulário e links internos seguem a regra
  da v2a (não copiados).

### Fora de escopo

Dividir por intervalos personalizados, combinar arquivos que não sejam PDF (v5: converter).

## 2. Arquitetura

- **pdf-core**: `SplitRule { Pages(u32), MaxBytes(u64), TopBookmarks }`;
  `PdfDoc::split(rule, dir, stem) -> Vec<PathBuf>`; `combine(inputs: &[(PathBuf, Option<String>)],
  dest) -> Result<()>` (função livre). Reaproveitam `copy_pages` e `write_atomic` da v2a; os
  marcadores de cada parte/arquivo são refeitos com `set_outlines` (destinos deslocados).
- **worker**: `Request::Split { rule, dir }` → `Response::Split(Vec<PathBuf>)`; combinar roda numa
  thread própria da interface (não depende de documento aberto).
- **gera-ui**: diálogo Dividir (na aba), aba especial "Combinar arquivos" (como Início e
  Ferramentas) com a lista; hub liga "dividir" e "combinar".

## 3. Erros

Pasta sem permissão, disco cheio, senha errada, arquivo não-PDF na lista: status/linha da lista
explica; nada é gravado pela metade (gravação atômica); combinar sem nenhum arquivo: botão desativado.

## 4. Testes

- **pdf-core**: dividir por páginas (contagem e ordem das partes), por tamanho (todas as partes ≤
  limite, salvo página única maior), por marcadores (títulos e intervalos), nomes livres; combinar
  (ordem, contagem, marcadores por arquivo com os originais dentro, comentários, senha).
- **E2E (Xvfb)**: diálogo Dividir e aba Combinar desenhados; ações cobertas pelos testes do núcleo
  (o seletor de arquivos do sistema não existe no Xvfb).
