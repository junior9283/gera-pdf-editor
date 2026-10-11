# v6e — Formatar, Validar e Calcular; campos Botão e Data; ordem de tabulação

Data: 2026-10-11 · Etapa da avaliação para a 1.0 (lacunas F02, F03, F06) · Versão alvo 0.19.0

## Objetivo

Fazer os formulários se comportarem como no Acrobat ao preencher:
- o valor aparece **formatado** (moeda, porcentagem, data, CEP, telefone…);
- a digitação é **filtrada** (só números num campo numérico);
- valores fora do intervalo são **recusados**;
- os **totais se calculam sozinhos**.

O Preparar formulário ganha os campos **Botão** e **Data**, as abas **Formatar**, **Validar** e
**Calcular** nas propriedades, a **ordem de tabulação** e **Destacar campos**.

## Fora desta etapa (v6e2 e v6e3)

- Detecção automática de campos (F01).
- Campos Imagem e Código de barras (F04).
- Ações de campo e de página (F05). O Botão desta etapa só tem rótulo e a ação Redefinir
  formulário.
- Alinhar e distribuir, seleção múltipla e duplicar (F07).
- Importar e exportar dados (F08).
- Nivelar (F09).
- Preenchimento automático (F10).
- Scripts JavaScript arbitrários: continuam sem executar, com o aviso atual.

## Comportamento de referência (Acrobat Pro)

### Formatar (aba Formatar das propriedades de um campo de texto)

| Categoria | Opções | Script gravado (/AA /F e /K) |
|-----------|--------|-------------------------------|
| Nenhum | — | sem /F nem /K |
| Número | casas decimais, estilo do separador (5 estilos), negativos (sinal, vermelho, parênteses), símbolo da moeda antes ou depois | `AFNumber_Format(nDec, sepStyle, negStyle, 0, "R$ ", true)` e `AFNumber_Keystroke(…)` |
| Porcentagem | casas decimais, separador | `AFPercent_Format(nDec, sepStyle)` e `AFPercent_Keystroke(…)` |
| Data | `dd/mm/aaaa` e as outras máscaras do Acrobat | `AFDate_FormatEx("dd/mm/yyyy")` e `AFDate_KeystrokeEx(…)` |
| Hora | `HH:MM`, `h:MM tt`, `HH:MM:ss`, `h:MM:ss tt` | `AFTime_Format(n)` e `AFTime_Keystroke(n)` |
| Especial | CEP (`99999-999`), Telefone, CPF, CNPJ, máscara arbitrária (`9`, `A`, `X`, `O`) | `AFSpecial_KeystrokeEx("99999-999")` |

- O valor fica gravado **cru** em /V (`1234.5`) e a aparência mostra o **formatado**
  (`R$ 1.234,50`).
- Ao digitar, o que não cabe no formato é recusado (com Número, letras não entram).
- Ao sair do campo, um valor que não se encaixa na data ou na máscara mostra o aviso do Acrobat:
  "O valor inserido não corresponde ao formato do campo [nome]".

### Validar
- "O valor deve estar entre `[mín]` e `[máx]`", com grava `AFRange_Validate(bMin, nMin, bMax, nMax)`.
- Fora do intervalo, o valor é recusado com o aviso do Acrobat: "O valor inserido deve ser
  maior ou igual a … e menor ou igual a …".

### Calcular
- "O valor é a **soma / média / produto / mínimo / máximo** dos campos a seguir": grava
  `AFSimple_Calculate("SUM", new Array ("a", "b"))`.
- **Notação simplificada de campos**: expressão com `+ - * /`, parênteses e nomes de campos
  (`Preco * Quantidade`). Grava o bloco `/** BVCALC … EVCALC **/` que o Acrobat usa.
- A ordem dos cálculos (/AcroForm /CO) é editável e é seguida ao recalcular. Mudar qualquer
  campo recalcula os que dependem dele.

### Campos novos
- **Botão:** rótulo e ação "Redefinir formulário". Clicar num botão com essa ação limpa o
  formulário.
- **Data:** campo de texto com Formatar = Data (`dd/mm/aaaa`). Ao preencher, aparece um seletor
  de calendário ao lado do campo.

### Ordem de tabulação
- No Preparar formulário, `Shift+N` mostra o número da ordem sobre cada campo.
- O menu "Ordem de tabulação" oferece:
  - **Ordenar por linha** (/Tabs /R);
  - **Ordenar por coluna** (/Tabs /C);
  - **Ordenar pela estrutura** (/Tabs /S, a ordem de /Annots);
  - **Ordenar manualmente**: mover um campo para cima ou para baixo na lista.
- O `Tab` ao preencher segue essa ordem.

### Destacar campos
- Barra do formulário: botão **Destacar campos** (atalho do app `Shift+L`). Os campos ficam com
  fundo azul-claro, e os obrigatórios com borda vermelha. A preferência é lembrada.

## Decisões (Rulings)

1. **Motor nativo dos scripts AF**, sem interpretador JavaScript.
   - O texto de /AA /F, /K, /V e /C é reconhecido pelas chamadas AF que o próprio Acrobat grava
     (e pelo bloco BVCALC da notação simplificada).
   - Script arbitrário não executa, e o aviso continua.
   - Custo se errado: formulários com JavaScript feito à mão não calculam. Executar JS de
     arquivo hostil sem limite de tempo seria pior.
2. **Valor cru em /V e aparência formatada.** A aparência é gerada com o texto formatado e
   o /V volta ao valor cru, como no Acrobat. Custo se errado: outro leitor que regenera a
   aparência mostra o valor cru.
3. **Separadores do Acrobat:**
   - estilo 0 `1,234.56`;
   - estilo 1 `1234.56`;
   - estilo 2 `1.234,56`;
   - estilo 3 `1234,56`.
   - Ao criar, o padrão é o estilo 2 (português do Brasil) com "R$ ".
4. **Calcular** só com números. Campo vazio vale 0. Divisão por zero dá vazio. O resultado é
   formatado pelo Formatar do próprio campo. Teto de 64 passos na cadeia de dependências.
5. **Ordem manual** reordena o /Annots da página e grava /Tabs /S.

## Arquitetura

**pdf-core:**
- `form_scripts.rs` (puro e testável):
  - `FieldFormat`, `FieldValidate`, `FieldCalc`;
  - `parse_format(js)`, `parse_validate(js)`, `parse_calc(js)` e a escrita do script
    equivalente;
  - `format_value`, `keystroke_ok`, `validate`, `evaluate`.
- `forms.rs`:
  - `FieldInfo` ganha `display` (o valor formatado) e `format`, `validate` e `calc`;
  - `set_field` valida, grava o valor cru com aparência formatada e recalcula na ordem /CO, numa
    etapa do desfazer;
  - falha de validação devolve `PdfError::Refused` com o aviso.
- `prepare.rs`:
  - `NewFieldKind::{Button, Date}`;
  - `FieldProps` com `format`, `validate`, `calc` e `button_label`;
  - `PrepareOp::SetCalcOrder` e `PrepareOp::SetTabOrder { page, order: TabOrder }`.
- `forms.rs` lê /Tabs e devolve a ordem de tabulação de cada página.

**gera-ui:**
- Propriedades com as abas Formatar, Validar e Calcular.
- Ferramentas Botão e Data no Preparar.
- Seletor de data ao preencher.
- Filtro de teclas.
- Números da ordem (`Shift+N`) e menu de ordem.
- Destacar campos.

## Erros e limites

- Scripts maiores que 64 KB não são reconhecidos (contam como script arbitrário).
- Números com mais de 15 casas decimais pedidas são limitados a 10.
- Datas de 1 a 9999.
- Valores não numéricos num campo de cálculo valem 0.
- Ciclo de cálculo (A soma B que soma A) é cortado pelo teto de passos.

## Testes

- **Unidade (form_scripts):**
  - formatar cada categoria e estilo (incluindo negativos e arredondamento);
  - filtro de teclas;
  - leitura e escrita dos scripts AF com os textos do Acrobat;
  - expressões da notação simplificada;
  - intervalo.
- **Integração (pdf-core):**
  - criar um campo com formato e preencher (/V cru, aparência formatada);
  - validação recusada não muda nada;
  - total recalculado ao mudar uma parcela, com uma etapa do desfazer;
  - criar Botão e Data;
  - ordem por linha, coluna e manual;
  - ler um PDF feito no Acrobat com AFNumber_Format.
- **E2E:** formulário com preço, quantidade e total; data pelo seletor; números da ordem;
  Destacar campos.

## Decisões tomadas na execução

6. A máscara do Especial não muda maiúsculas e minúsculas, como no Acrobat.
7. Porcentagem grava o número digitado e mostra ×100 (0,15 → 15%), como no Acrobat.
8. O bloco BVCALC só vale quando o código depois dele é a tradução que o Acrobat grava para a
   expressão (números comparados pelo valor, nomes ao pé da letra). Senão o comentário poderia dizer
   uma conta e o código fazer outra.
9. Uma ação encadeada (/Next) ou de outro tipo não é confiável. Um script em stream é lido com teto.
   O intervalo recusa texto que não é número.
10. O recálculo não relê o documento a cada passada e tem teto de 2000 gravações. O que já mudou
    entra no desfazer mesmo se um cálculo falhar no meio.
11. O botão usa a aparência do MuPDF (/MK /CA, /BG e /BC). O Redefinir respeita /Fields e /Flags
    da ação.
12. `fields(page)` devolve os campos na ordem de tabulação. Ordem de tabulação e de cálculo ficam
    no menu "Ordem" da barra.
13. Destacar campos é o "Realçar campos" que já existia; `Shift+L` alterna (sem preferência
    gravada).
14. O tema escuro só escurece a interface: as páginas ficam sempre brancas (pedido do dono).
15. Números:
    - o valor cru em /V é lido direto;
    - o digitado segue o estilo do campo, com milhar só em grupos de 3 ("1.234" no estilo 2 é 1234; "1,5" no estilo 0 é recusado);
    - letras são recusadas;
    - Calcular e Validar aceitam vírgula decimal (como o AFMakeNumber);
    - ao editar, o rascunho mostra o cru com o decimal do estilo.
16. A aparência formatada é refeita ao limpar, mover ou redimensionar e ao trocar o Formatar.
17. No AFSimple_Calculate, um nome de grupo soma os campos de baixo. Na notação simplificada, um
    grupo vale 0.
