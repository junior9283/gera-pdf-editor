# Histórico de versões

Todas as mudanças relevantes do projeto. O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/)
e as versões seguem o [Versionamento Semântico](https://semver.org/lang/pt-BR/): enquanto a versão for
0.x, cada etapa do roteiro concluída sobe o número do meio.

## [Não lançado]

- Redigir e Comparar arquivos (rumo à 1.0.0).

## [0.10.0] — 2026-10-10 · Marca d'água e Cabeçalho e rodapé (v2c)

### Adicionado
- **Marca d'água**: texto (fonte, negrito, tamanho, cor), imagem ou página de outro PDF; rotação,
  opacidade, escala relativa à página, atrás ou na frente do conteúdo, posição e páginas (todas,
  intervalo, pares, ímpares), com prévia. Adicionar (pergunta se substitui a existente),
  Atualizar e Remover — remove também as marcas feitas pelo Acrobat.
- **Cabeçalho e rodapé**: seis caixas, fonte e margens, número da página ("1", "1 de n",
  "Página 1 de n"), data e **numeração Bates** (prefixo, sufixo, dígitos, início), com prévia,
  Atualizar e Remover.
- A marca é um único objeto no arquivo, desenhado em todas as páginas; o caminho do arquivo de
  origem não é gravado no PDF.

## [0.9.0] — 2026-10-10 · Editar PDF: texto (v4b)

### Adicionado
- **Editar PDF** mostra o contorno dos blocos de texto; clicar abre a edição no lugar (Esc ou clicar
  fora confirmam, Ctrl+Z desfaz).
- O texto novo sai na mesma posição, tamanho, cor e direção (também em páginas giradas). A fonte
  original é reaproveitada quando tem todas as letras; senão, a padrão equivalente (Helvetica, Times
  ou Courier, com negrito e itálico).
- Parágrafos refluem na própria largura, com o recuo da primeira linha; linha solta (título) cresce
  até a margem.
- **Adicionar texto** cria um bloco novo onde se clica.
- Só as letras do bloco saem: linhas vizinhas, imagens, desenhos, links e comentários ficam; texto
  invisível de OCR continua invisível. Texto que não dá para trocar com segurança (formulário, fonte
  especial) é recusado sem mexer no documento.

## [0.8.0] — 2026-10-10 · Editar PDF: imagens (v4a)

### Adicionado
- Ferramenta **Editar PDF** com contorno das imagens da página.
- Mover (arrastar ou setas; `Shift` move 10 pt), redimensionar mantendo a proporção, girar 90°,
  substituir, adicionar e excluir imagens, com desfazer e refazer.
- A ordem de empilhamento é preservada; a imagem excluída ou substituída sai do arquivo ao salvar.
- Imagens repetidas, dentro de formulários ou embutidas aparecem com contorno, mas bloqueadas.
- Recusa em documentos assinados ou sem permissão de alteração.

## [0.7.0] — 2026-10-10 · Converter, OCR, proteger e comprimir (v5)

### Adicionado
- **Proteger PDF**: senha de abertura e de permissões (AES-256); as permissões de documentos
  protegidos são respeitadas em todo o app.
- **Comprimir PDF**: níveis baixa, média e alta.
- **Criar PDF** a partir de imagens (respeitando a orientação EXIF) e **Exportar PDF** para Word
  (.docx), texto e imagens.
- **Digitalizar e OCR** com Tesseract (português e inglês), deixando o texto pesquisável.

## [0.6.0] — 2026-10-10 · Formulários e assinaturas (v3)

### Adicionado
- Preencher formulários (texto, caixas de seleção, botões de opção, listas).
- **Preencher e assinar**: texto, marcas, assinatura e iniciais (digitadas, desenhadas ou de imagem).
- **Preparar formulário**: criar e editar campos.
- **Certificados**: assinatura digital com PKCS#12 ou ID digital criado no app, e validação.

## [0.5.0] — 2026-10-09 · Organizar páginas (v2)

### Adicionado
- **Organizar páginas**: reordenar, girar, excluir, extrair; inserir páginas; recortar margens.
- **Dividir PDF** por número de páginas, tamanho máximo ou marcadores; **Combinar arquivos**.

## [0.4.0] — 2026-10-09 · Anotações e comentários (v1.1-B2)

### Adicionado
- Anotações editáveis (mover, redimensionar, cor, espessura, opacidade).
- Caixa de texto, retângulo, elipse, linha, seta e carimbos.
- Painel **Comentários** com respostas, status, filtro, ordem e busca.

## [0.3.0] — 2026-10-09 · Texto, impressão e propriedades (v1.1-B1)

### Adicionado
- Seleção e cópia de texto, impressão e propriedades do documento.

## [0.2.0] — 2026-10-09 · Interface no estilo do Acrobat (v1.1-A)

### Adicionado
- Abas de documentos, Início, central de Ferramentas, painéis laterais, menus e atalhos do Acrobat.

## [0.1.0] — 2026-10-09 · Visualizador (v1)

### Adicionado
- Visualizador leve: abrir, rolar, zoom, miniaturas, busca, destaque, sublinhado, notas e desenho.

[Não lançado]: https://github.com/junior9283/gera-pdf-editor/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/junior9283/gera-pdf-editor/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/junior9283/gera-pdf-editor/releases/tag/v0.1.0
