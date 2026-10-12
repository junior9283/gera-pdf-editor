# v6e3: detecção automática de campos e campo Código de barras

Data: 2026-10-12 · Etapa da avaliação para a 1.0 (lacunas F01 e o resto da F04) · Versão alvo
0.21.0

## Objetivo

1. **Detecção automática de campos (F01).** Num PDF sem formulário (escaneado com OCR, exportado de
   um editor de texto), abrir o Preparar formulário cria os campos sozinho, como o Acrobat:
   - linhas para escrever viram campos de texto;
   - caixas pequenas viram caixas de seleção;
   - células vazias de tabelas e caixas retangulares viram campos de texto;
   - o rótulo ao lado (ou acima) dá o nome do campo;
   - um rótulo com "assinatura" vira campo de assinatura, e um com "data" vira campo de data.
2. **Campo Código de barras (F04).** Um campo que mostra um código de barras 2D (PDF417, QR Code ou
   Data Matrix) com os valores de outros campos, atualizado a cada preenchimento, gravado como o
   Acrobat (/PMD).

## Fora desta etapa (v6e4)

- Importar e exportar dados (FDF, XFDF, XML, TXT).
- A ação Enviar formulário.
- Nivelar.
- Preenchimento automático.
- A codificação em XML do código de barras.

## Comportamento de referência (Acrobat Pro)

### Detecção

- Ao abrir o Preparar formulário num documento sem campos, o Acrobat analisa as páginas e acrescenta
  os campos que achou. Eles ficam selecionáveis para ajustes, e o desfazer tira todos.
- Num documento com campos, a detecção roda pelo menu **Organizar ▸ Detectar campos
  automaticamente**. Só entram campos que não se sobrepõem aos existentes.
- O que vira campo:
  - linha horizontal (traço ou retângulo fino) com 36 pt ou mais → campo de texto logo acima;
  - sequência de 5 ou mais "_" no texto → campo de texto sobre ela;
  - quadrado de 6 a 20 pt de lado → caixa de seleção;
  - retângulo vazio com 40 pt ou mais de largura e 12 a 80 pt de altura (célula de tabela ou
    caixa) → campo de texto (várias linhas acima de 30 pt de altura);
  - um retângulo com texto dentro não é campo (é um rótulo ou cabeçalho).
- Nome: o texto mais próximo à esquerda na mesma linha, ou logo acima, sem ":" no fim, com até 40
  caracteres. Sem rótulo, fica o nome automático. Nomes repetidos ganham sufixo.
- Tipo pelo rótulo: "assinatura" / "signature" → assinatura; "data" / "date" → data.

### Código de barras

- **Ferramenta Código de barras** no Preparar.
- **Propriedades:**
  - aba **Valor**: os campos codificados (todos ou os escolhidos) e "Incluir nomes dos campos";
  - aba **Opções**: simbologia (PDF417, QR Code, Data Matrix) e a correção de erros (PDF417 0 a 8;
    QR L, M, Q, H).
- **Valor**, como o "Delimitado por tabulação" do Acrobat:
  - com os nomes, a primeira linha traz os nomes dos campos separados por tabulação e a segunda, os
    valores;
  - sem os nomes, só os valores.
- **Atualização:** o campo recalcula e redesenha o código a cada preenchimento, como um Calcular
  (entra na ordem de cálculo).
- **Gravação, como o Acrobat:**
  - `/FT /Tx`, somente leitura, não exportar, várias linhas;
  - `/PMD` (`/Type /PaperMetaData`, `/Version 1`, `/Width`/`/Height` em polegadas, `/Symbology`,
    `/XSymWidth`, `/XSymHeight`, `/ECC`, `/DataPrep 0`, `/Resolution 300.0`);
  - um script de Calcular que monta o valor (reconhecido pelo app, como as chamadas AF).
- **Aparência:** os módulos do código desenhados em preto no /AP. O MuPDF não desenha códigos de
  barras.
- **Código de barras de outro programa:** o app desenha o /V gravado com a simbologia do /PMD; o
  script de quem gravou não roda.

## Decisões (Rulings)

1. **Detecção por regras sobre os traços e o texto da página** (dispositivo do MuPDF e texto
   estruturado), sem aprendizado de máquina.
   - Custo se errado: em formulários muito irregulares, o Acrobat (que usa um modelo treinado)
     acha mais campos. Os ajustes são feitos no próprio Preparar.
2. A detecção é **uma etapa do desfazer** e só acrescenta: nunca muda nem apaga campos existentes.
3. **Biblioteca `rxing`** (Apache-2.0, porte do ZXing) só com os codificadores de PDF417, QR Code e
   Data Matrix. Os decodificadores entram só nos testes, para ler o código desenhado.
4. **O script do código de barras é escrito pelo app** e reconhecido só quando é exatamente o que
   o app gera (como o BVCALC). Script de outro programa conta como arbitrário: o código mostra o /V
   gravado.
5. **Dados acima da capacidade da simbologia:** o campo mostra um aviso no lugar do código ("dados
   demais para o código"), e o /V fica gravado.

## Arquitetura

**pdf-core:**
- `detect.rs`:
  - coleta de segmentos e retângulos (um `NativeDevice` que recebe `stroke_path` e `fill_path`)
    e de palavras (texto estruturado);
  - `detect_candidates(page_box, shapes, words, existing) -> Vec<Candidate>` (puro e testável);
  - `PdfDoc::detect_fields(pages)`, com `PrepareOp::Detect { pages }`.
- `barcode.rs`:
  - `BarcodeSpec { symbology, ecc, fields, names }`;
  - leitura e escrita do `/PMD` e do script;
  - `barcode_value(spec, lookup)`;
  - `draw_barcode` (rxing → módulos → /AP);
  - `NewFieldKind::Barcode`, `FieldProps.barcode`;
  - o recálculo de `forms.rs` passa a incluir os códigos de barras.

**gera-ui:**
- detecção automática ao entrar no Preparar sem campos, e o item de menu;
- ferramenta Código de barras;
- abas Valor e Opções do código de barras.

## Erros e limites

- Detecção:
  - no máximo 400 campos por página;
  - páginas com mais de 20 000 traços são puladas;
  - um campo detectado nunca se sobrepõe a outro.
- Código de barras:
  - no máximo 2 000 caracteres de valor;
  - simbologia desconhecida no /PMD → PDF417.

## Testes

- **Unidade (detect):**
  - linha com rótulo à esquerda → campo de texto com nome;
  - "____" → campo;
  - quadrado → caixa;
  - retângulo com texto dentro → nada;
  - "Assinatura:" → assinatura;
  - "Data" → data;
  - sem sobreposição com campos existentes.
- **Integração:**
  - PDF desenhado com linhas, caixas e rótulos → os campos certos numa etapa do desfazer;
  - código de barras criado, preenchido (o valor e o /AP mudam);
  - código lido de volta pelo decodificador do rxing (as três simbologias);
  - /PMD como o Acrobat;
  - código de barras de outro programa desenhado a partir do /V.
- **E2E:**
  - abrir o Preparar num PDF sem campos → campos detectados;
  - código de barras QR com dois campos, preencher e ver o código mudar.

## Decisões tomadas na execução

6. **Detecção:**
   - o rótulo é procurado à esquerda e depois acima (na caixa de seleção, à direita primeiro);
   - só o conteúdo da página entra (comentários não);
   - nenhuma sobreposição com campos existentes: o candidato encolhe até o campo de cima;
   - a assinatura cresce até o que estiver acima;
   - caixas com mais de 30 pt viram campos de várias linhas.
7. **Tetos da detecção:** traços (também dentro de um caminho), palavras, consultas ao texto e
   campos existentes. Uma página hostil termina em menos de 1 s.
8. **Valor do código de barras:**
   - é um Calcular reconhecido (`FieldCalc::Barcode`), que entra na ordem de cálculo e no
     desfazer;
   - o script é conferido por igualdade exata com o que o app escreve.
9. **Aparência do código:**
   - desenhada pelo app, com zona de silêncio e codificação UTF-8;
   - uma chave (valor, opções e tamanho) evita recodificar;
   - o /V é gravado sem o texto do MuPDF.
10. **Orçamento:** 50 códigos codificados por operação inteira (abrir, Preparar, preencher,
    redefinir, desfazer e refazer). Fora do orçamento, o código fica como estava, ou só com a borda.
11. **Data Matrix com teto de 500 caracteres.** O codificador entra em pânico perto de 1 558
    palavras-código, e o release aborta com pânico.
12. **Ao abrir:**
    - documento assinado não é tocado;
    - com /NeedAppearances, as aparências são geradas e a marca é tirada;
    - Reverter redesenha os códigos.
13. **Código de outro programa:** o OK nas propriedades sem mexer na aba Valor mantém o script
    dele.
14. **Barra do Preparar:** rótulos curtos (Texto, Caixa, Opção…), com o nome completo na dica.
