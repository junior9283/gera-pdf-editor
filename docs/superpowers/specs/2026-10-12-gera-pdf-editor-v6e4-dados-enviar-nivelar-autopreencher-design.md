# v6e4: dados do formulário, Enviar, nivelar e preenchimento automático

Data: 2026-10-12 · Etapa da avaliação para a 1.0 (lacunas F08, F09 e F10, e a ação Enviar da F05) ·
Versão alvo 0.22.0. Fecha a linha de formulários.

## Objetivo

1. **Dados do formulário (F08)**, como em Preparar formulário ▸ Mais ▸ Gerenciar dados do
   formulário do Acrobat:
   - **Exportar dados** em FDF, XFDF, XML ou TXT;
   - **Importar dados** de FDF, XFDF, XML ou TXT, preenchendo os campos pelo nome, numa etapa do
     desfazer;
   - **Mesclar arquivos de dados numa planilha**: vários PDF de formulário, FDF ou XFDF viram um
     CSV, com uma coluna por campo e uma linha por arquivo.
2. **Ação Enviar formulário (F05)**:
   - aba Ações com "Enviar um formulário" (URL e formato: FDF, XFDF, HTML ou o PDF inteiro);
   - ao clicar, o app pede confirmação mostrando o endereço;
   - http(s) é enviado por POST;
   - mailto: abre o programa de e-mail com os dados no corpo.
3. **Nivelar (F09)**: campos e (opcionalmente) comentários viram parte da página, numa etapa do
   desfazer. O comando fica no menu Organizar do Preparar e em Ferramentas.
4. **Preenchimento automático (F10)**, como o "Preenchimento automático" do Acrobat
   (Preferências ▸ Formulários):
   - Desligado, Básico ou Avançado;
   - "Lembrar dados numéricos";
   - Editar a lista e Apagar.
   - Ao digitar num campo de texto, as entradas já usadas que começam com o que foi digitado
     aparecem numa lista; Enter ou clique aceita.

## Comportamento de referência (Acrobat Pro)

### Formatos de dados

- **FDF**: `%FDF-1.2`, `/FDF << /Fields [ << /T (nome) /V (valor) >> … ] /F (arquivo.pdf) >>`. Os
  nomes hierárquicos viram `/Kids`.
- **XFDF**: `<xfdf><fields><field name="a"><field name="b"><value>…</value></field></field></fields>
  </xfdf>`.
- **XML** ("Arquivos XML" do Acrobat): `<fields xmlns:xfdf="http://ns.adobe.com/xfdf-transition/">`,
  com um elemento por campo. Nomes que não são XML válido levam `xfdf:original`.
- **TXT**: delimitado por tabulação. A primeira linha traz os nomes; a segunda, os valores. Ao
  importar um TXT com várias linhas, vale a primeira linha de dados.
- **Botões**: caixas e opções exportam o estado (/V); valor vazio não é exportado.

### Enviar formulário

- `/S /SubmitForm /F (url) /Flags n`:
  - bit 3 (4) = HTML;
  - bit 6 (32) = XFDF;
  - bit 9 (256) = PDF inteiro;
  - sem esses bits, FDF.
- `/Fields` limita aos campos listados.

### Nivelar

- O Acrobat nivela por Imprimir para PDF ou pelo Preflight. Aqui o comando é direto: "Nivelar
  campos" ou "Nivelar campos e comentários". O MuPDF passa as aparências para o conteúdo da
  página.

## Decisões (Rulings)

1. **Importar TXT com várias linhas** usa a primeira linha de dados. O Acrobat pede a linha numa
   janela. Custo: para outra linha, apagar as anteriores do arquivo.
2. **Enviar:**
   - pede confirmação toda vez (como os links), mostrando o endereço;
   - só http, https e mailto; o resto é recusado;
   - tempo-limite de 30 s;
   - resposta acima de 1 MB é cortada;
   - a resposta não é mostrada como página: só "Enviado" ou o erro.
3. **E-mail (mailto)**: os dados vão no corpo como `nome=valor`, porque o mailto não leva anexo.
   Custo: o destinatário recebe texto, não um FDF anexado.
4. **Nivelar** é uma etapa do desfazer (instantâneo). Ao nivelar com campos, o /AcroForm fica sem
   campos, como no Acrobat.
5. **Preenchimento automático**:
   - lista de até 500 entradas, gravada nas preferências do usuário (não no PDF);
   - campos de senha nunca entram;
   - números só com "Lembrar dados numéricos";
   - padrão Básico, como o Acrobat.

## Arquitetura

**pdf-core:**
- `form_data.rs`:
  - `DataFormat::{Fdf, Xfdf, Xml, Txt}`;
  - `PdfDoc::export_form_data(path, format)`;
  - `PdfDoc::import_form_data(path) -> ImportReport`;
  - `merge_form_data(files, dest)`;
  - leitura com tetos: 2 MB, 10 000 campos, profundidade 32.
- `submit.rs`:
  - `ActionKind::Submit { url, format }`;
  - `PdfDoc::submit_payload(format, fields) -> (bytes, content-type)`.
- `PdfDoc::flatten(comments: bool)`.

**gera-ui:**
- menu Organizar ▸ Dados do formulário (Exportar, Importar, Mesclar);
- Nivelar;
- execução do Enviar com confirmação (thread);
- Preferências ▸ Formulários com o preenchimento automático;
- a lista de sugestões no campo de texto.

**app-state:** `autofill.rs` (lista, sugestões, aprender).

## Testes

- **Integração (form_data):**
  - exportar e importar ida e volta nos 4 formatos (texto, caixa, opção, lista, nome hierárquico,
    acentos);
  - arquivo hostil (enorme, profundidade, nomes inválidos);
  - mesclar 3 arquivos em CSV;
  - importar numa etapa do desfazer.
- **Submit:**
  - corpo FDF, XFDF e HTML gerado;
  - flags;
  - URL recusada.
- **Nivelar:** campos viram conteúdo (não restam widgets), e desfazer volta.
- **app-state:** sugestões por prefixo, sem senha, números com a opção, teto de 500.
- **E2E:**
  - exportar XFDF, limpar, importar;
  - nivelar;
  - sugestão ao digitar.

## Decisões tomadas na execução

1. **Enviar e a rede.** O endereço passa pelo mesmo analisador de URL do envio (crate `url`), e
   o que sai é a forma canônica.
   - É recusado:
     - usuário e senha no endereço;
     - localhost e nomes sem domínio;
     - `.local`, `.internal`, `.lan` e `.home`;
     - IPs de loopback, privados, link-local ou CGNAT, em qualquer grafia.
   - O envio resolve o nome só para IP público (contra DNS rebinding) e não segue
     redirecionamentos: um 3xx conta como falha.
   - Custo: um formulário legítimo de intranet não envia. A alternativa é exportar os dados e
     mandar o arquivo.
2. **mailto.**
   - Só passam os destinatários, `subject`, `cc` e `bcc`. `attach`, `body` e o resto saem,
     porque alguns programas de e-mail anexam um arquivo local pelo endereço.
   - A confirmação mostra todos os que recebem, com o bcc marcado como oculto.
3. **Envio gravado como o Acrobat.** A ação grava `/F << /FS /URL /F (endereço) >>`. Na leitura,
   valem as duas formas.
4. **Importar.**
   - Recusado sem a permissão de preencher formulários.
   - Campos somente leitura contam como "nome desconhecido" e não mudam. O Acrobat importa neles;
     aqui segue a regra do preenchimento.
   - Um `/V` em lista (seleção múltipla) no FDF fica de fora.
5. **Tetos de leitura**, além dos 2 MB, 10 000 campos e profundidade 32 do plano: nome completo de
   até 4 KB e total lido (nomes mais valores) de até 8 MB. Um arquivo pequeno com nome pai enorme
   ou valor compartilhado não multiplica a memória.
6. **Planilha.**
   - Células com `;` ou tabulação vão entre aspas, porque o Excel em pt-BR separa por `;`.
   - O destino precisa ser `.csv` e nunca pode ser um dos arquivos mesclados. O núcleo recusa.
7. **Nivelar** exige documento modificável (sem assinatura e com permissão de alteração). O
   comando fica no menu Organizar do Preparar, onde estão as outras ações do formulário.
8. **Preenchimento automático.**
   - A lista guarda o campo de cada entrada.
   - No Avançado, as entradas do mesmo campo vêm primeiro e aparecem já com o campo vazio.
   - Só em campos de uma linha. O clique na sugestão vale no aperto do botão, porque o clique tira
     o foco do campo antes de soltar.
   - Cada aba tem uma cópia da lista. Uma entrada aprendida vai às preferências e às outras abas.
   - O OK das Preferências mantém o que foi aprendido com a janela aberta, se a lista não foi
     editada nela.
9. **Exportar dados** usa a extensão para escolher o formato. Sem extensão vira `.fdf`, como o
   Acrobat; outra extensão (um `.pdf`, por engano) é recusada.
