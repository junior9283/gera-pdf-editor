# Gera PDF Editor — Subprojeto 5b: Comprimir PDF

Data: 2026-10-10 · Status: aprovado pelo Claude (autonomia dada pelo dono — ver CLAUDE.md)

## 1. Objetivo

A ferramenta **Comprimir PDF** do Acrobat: reduzir o tamanho do arquivo escolhendo o nível
(Baixa, Média ou Alta compactação), sem mexer no documento aberto.

### Critérios de sucesso

- Janela com o tamanho atual, os três níveis e **Comprimir**; o resultado vai para
  "<nome> (comprimido).pdf" ao lado do original (nome livre, sem sobrescrever), abre numa aba nova e
  a barra de status mostra "de X para Y (−Z%)".
- **Baixa**: só estrutura — objetos repetidos unidos e não usados removidos (coleta nível 4),
  fluxos comprimidos (Flate), fontes e imagens sem compressão passam a ter.
- **Média**: Baixa + imagens acima de 225 ppp reduzidas a 150 ppp, JPEG qualidade 75.
- **Alta**: Baixa + imagens acima de 150 ppp reduzidas a 100 ppp, JPEG qualidade 60.
- Imagem só é trocada se ficar menor; ficam como estão: máscaras (/ImageMask), imagens com /Decode,
  CMYK/Lab/indexadas e imagens com alfa embutido (a /SMask é mantida e continua valendo).
- Documento protegido: o resultado mantém a proteção. Documento assinado: comprimir é recusado
  com aviso (a regravação invalidaria as assinaturas).

### Fora de escopo

Otimizador avançado (remover objetos invisíveis, achatar transparência, subconjunto de fontes),
JPEG 2000, JBIG2.

## 2. Arquitetura

- **pdf-core** `compress.rs`: `CompressLevel`, `CompressReport { before, after, images }`,
  `PdfDoc::compress_to(dest, level)`: cópia em memória (bytes, mantendo a criptografia), troca das
  imagens, gravação com coleta 4 + compressão.
- **worker**: `Request::Compress { level }` → `Response::Compressed { dest, report }`.
- **gera-ui**: janela Comprimir PDF; abre o resultado numa aba.

## 3. Testes

- pdf-core: PDF com imagem RGB grande sem compressão → Média menor que Baixa e Baixa menor que o
  original; imagem reduzida tem as dimensões esperadas; o documento aberto não muda; máscara e
  /Decode ficam; assinado recusa; protegido continua protegido.
