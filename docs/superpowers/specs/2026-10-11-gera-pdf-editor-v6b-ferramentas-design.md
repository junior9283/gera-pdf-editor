# v6b: Ferramentas de tela, preferências, menus e busca (design)

Segunda etapa da avaliação para a 1.0 (`2026-10-10-gera-pdf-editor-avaliacao-1.0.md`). Cobre os itens V09,
V10, V19, M02, M03, M04, M09 e M11, além das pendências menores da v6a (M6, M8, M10, M11 e M12 do ledger).

As Propriedades editáveis (D01/D02) passam para a v6c, junto com os painéis (estrutura do documento), para
a etapa caber num ciclo. A spec foi aprovada pelo próprio Claude (autonomia total, ver CLAUDE.md).

## O que o Acrobat faz (referência)

- **Mão** (`H`; `Espaço` segurado vira Mão temporária): arrastar move a página; o clique segue links; não
  seleciona texto.
- **Zoom com marca** (`Z`):
  - arrastar um retângulo amplia até ele preencher a janela;
  - clicar amplia um nível em torno do ponto;
  - `Ctrl` + clique reduz.
- **Aceleradores de tecla única** (Preferências → Geral, "Usar aceleradores de tecla única", desligado de
  fábrica): `V` Selecionar, `H` Mão, `Z` Zoom com marca, `S` Nota, `U` Destacar (`Shift+U` alterna entre
  destacar e sublinhar), `D` Desenhar, `X` Caixa de texto, `K` Carimbo.
- **Preferências** (`Ctrl+K`): uma janela com a lista de categorias à esquerda.
- **Editar:**
  - Recortar (`Ctrl+X`), Copiar e Colar (`Ctrl+V`) de comentários;
  - colar uma imagem da área de transferência cria um carimbo, e colar texto cria uma caixa de texto;
  - Desmarcar tudo (`Ctrl+Shift+A`).
- **Arquivo:** Reverter, Criar PDF, Exportar PDF e Proteger com senha.
- **Localizar:**
  - opções Palavras inteiras, Diferenciar maiúsculas de minúsculas, Incluir marcadores e Incluir
    comentários;
  - `Enter` e `F3`/`Ctrl+G` vão ao próximo resultado; `Shift+F3`/`Ctrl+Shift+G`, ao anterior.
- **Menu de contexto na página** (botão direito fora de comentário ou seleção): Selecionar, Mão, Zoom com
  marca, Página anterior, Próxima página, Vista anterior, Girar no sentido horário, Imprimir, Localizar e
  Propriedades do documento.
- **Ajuda** (`F1`): abre a ajuda.

## Decisões

1. **Ferramentas de tela** (`app_state::tools::Tool`): entram `Hand` e `Zoom`, fora da linha Comentar. A
   barra do documento ganha os botões Selecionar, Mão e Zoom com marca, no começo, como no Acrobat.
   - **Mão:** arrastar rola a área (`scroll_delta`) nos dois eixos, com o cursor `Grab`/`Grabbing`. O
     clique segue o link (mesma rotina da v6a). Não cria nem seleciona nada.
   - **Espaço segurado** (fora de campo de texto): Mão temporária enquanto a tecla está pressionada; ao
     soltar, volta a ferramenta anterior. Nos modos página a página, o Espaço deixa de virar a página,
     como no Acrobat (a roda e as setas continuam).
   - **Zoom com marca:** arrastar desenha o retângulo tracejado. Ao soltar, o zoom faz o retângulo caber
     na janela, limitado a 6400 %, e a rolagem o centraliza. O clique amplia ao próximo nível centrado no
     ponto; `Ctrl` + clique reduz.
   - A rolagem nos dois eixos usa uma única porta, `DocTab::scroll_to(x, y)`, o canal `jump_x`/`jump_to`
     da v6a.
2. **Aceleradores** (`prefs.single_key`, padrão `false`, como no Acrobat):
   - com eles ligados, a tabela de atalhos ganha as teclas acima, mais `Shift+U`;
   - nunca valem dentro de campo de texto;
   - com eles desligados, `V`/`H`/`Z` não fazem nada, mas o Espaço segurado funciona sempre (também no
     Acrobat).
3. **Preferências** (`Ctrl+K`, Editar → Preferências…): uma janela com a lista de categorias à esquerda e
   OK/Cancelar.
   - **Geral:** aceleradores de tecla única e "Abrir com o painel de ferramentas visível".
   - **Exibição de página:**
     - layout padrão (Automático ou um dos quatro modos);
     - zoom padrão (Automático = Ajustar à largura, Ajustar à página, Ajustar à altura ou um nível da
       lista);
     - valem para documentos novos, e a visualização inicial do arquivo prevalece.
   - **Documentos:**
     - "Restaurar a última vista ao reabrir documentos" (ligado);
     - número de recentes (1 a 50, padrão 20);
     - "Esquecer os documentos recentes".
   - **Identidade:** o nome (o que já existia).
   - **Segurança:** a lista dos sites lembrados no aviso de links, com "Esquecer" em cada um e "Esquecer
     todos".
   - **Aparência:** modo escuro.
   - Cancelar descarta; OK grava as preferências.
   - Restaurar a última vista guarda página, zoom e ajuste por arquivo
     (`prefs::last_views: BTreeMap<caminho, ViewEntry>`, até 200 arquivos), na saída e ao fechar a aba.
4. **Recortar, Copiar e Colar** (`Ctrl+X`, `Ctrl+C`, `Ctrl+V`):
   - **Comentário selecionado:**
     - Copiar guarda a anotação na área interna do app (`AnnotClipboard`: tipo, retângulo, cores, texto,
       desenho);
     - Recortar faz o mesmo e a apaga (uma operação desfazível);
     - na área de transferência do sistema vai o texto do comentário.
   - **Colar**, nesta ordem:
     1. **Comentário copiado no app**, se o texto da área de transferência ainda for o dele (assim um
        texto copiado em outro programa depois tem preferência). O comentário entra na página atual, na
        mesma posição, deslocado 10 pt para baixo e à direita se colado na mesma página. Fica
        selecionado e entra no desfazer.
     2. **Imagem do sistema**, lida com o `arboard`, que já está na árvore de dependências pelo egui, com
        teto de 40 megapixels: vira um carimbo de imagem no centro da área visível, do tamanho natural
        limitado a 80 % da página. Usa o mesmo caminho dos carimbos de imagem da assinatura.
     3. **Texto do sistema:** vira uma Caixa de texto no centro da área visível, com até 10 000
        caracteres.
   - Sem permissão de comentar (documento protegido), Colar avisa como os outros comandos (`deny`).
   - Desmarcar tudo (`Ctrl+Shift+A`) limpa a seleção de texto e a de comentário.
   - Recortar e colar campos de formulário ficam para a v6e (formulários).
5. **Arquivo:** Criar PDF…, Exportar PDF…, Proteger com senha… e **Reverter**.
   - Reverter só fica ativo com alterações não salvas. Pergunta "Reverter para a versão salva? As
     alterações serão perdidas." e reabre o arquivo na mesma página.
6. **Busca:**
   - **Opções no ⏷ ao lado do campo:** Palavras inteiras, Diferenciar maiúsculas/minúsculas, Incluir
     marcadores e Incluir comentários.
   - **Núcleo:** `Request::Search { needle, options }` passa a usar o mesmo motor da redação (texto
     estruturado, linha a linha, com a caixa de cada letra). O padrão é diferenciar maiúsculas e
     minúsculas desligado, como hoje. A ordem dos resultados fica: marcadores, depois página a página
     o texto e os comentários dessa página.
     - **Palavras inteiras:** a regex ganha `\b` nas pontas.
     - **Comentários:** casam pelo conteúdo, e o resultado é o retângulo da anotação.
     - **Marcadores:** um resultado por título que casa, com a página de destino e o retângulo no topo
       da página.
   - **Teclas:**
     - `Enter` no campo: busca da primeira vez e depois vai ao próximo; `Shift+Enter` volta;
     - `F3`/`Ctrl+G` e `Shift+F3`/`Ctrl+Shift+G` funcionam mesmo fora do campo.
     - Os saltos entram no histórico (v6a).
7. **Menu de contexto da página:** botão direito sobre a página, sem comentário e sem seleção sob o
   ponteiro.
   - Itens: Selecionar, Mão e Zoom com marca (marcando a atual); Página anterior, Próxima página e Vista
     anterior; Girar no sentido horário; Imprimir…; Localizar; Propriedades do documento….
   - Usa os mesmos `Command` dos menus.
8. **Ajuda:** `F1` e Ajuda → Ajuda do Gera PDF Editor abrem o README do projeto no navegador padrão
   (`ctx.open_url`; endereço fixo do app, sem aviso). A janela de atalhos ganha os itens novos.
9. **Pendências da v6a:**
   - **M6:** arrastar o número da página não empilha vistas; só a mudança confirmada entra no
     histórico.
   - **M8:** com o aviso de link, a pergunta de tela cheia ou outra janela aberta, o `Esc` é delas antes
     de sair da tela cheia ou do modo leitura.
   - **M10:** na tela cheia, a aba que vem para a frente também entra em apresentação.
   - **M11:** a pergunta de tela cheia guarda a aba que pediu e some se a aba sair da frente.
   - **M12:** `Ctrl+Y` com o campo de zoom escondido não deixa o foco pendente.

## Segurança

- A imagem colada é lida com teto de pixels, e o texto colado com teto de caracteres.
- Colar respeita a permissão de comentar.
- O endereço da Ajuda é uma constante do app (não vem de PDF).
- A lista de sites nas Preferências mostra as chaves com o mesmo `safe_display` do aviso.

## Fora do escopo (registrado)

- Propriedades editáveis (D01/D02): v6c.
- Zoom dinâmico, Lupa, Panorâmica (`Shift+Z`): v6j.
- Recortar e colar campos: v6e.
- Preferências de tela cheia, unidades e réguas: v6j.
- `Enter`/`Shift+Enter` fora do campo de busca: continua adiado (ruling da v6a).

## Testes

- **app-state:**
  - `tools`: Mão e Zoom fora da linha Comentar;
  - `zoom::fit_rect` (retângulo para zoom e rolagem);
  - `prefs` (aceleradores, recentes, `last_views` com teto, sanitize);
  - `search_options` para regex (palavras inteiras, maiúsculas, escape).
- **pdf-core** (`tests/search.rs`):
  - palavras inteiras e maiúsculas;
  - comentários e marcadores;
  - ordem dos resultados;
  - texto com caracteres de regex.
- **gera-ui:**
  - atalhos novos (`Ctrl+K`, `Ctrl+X`, `Ctrl+V`, `Ctrl+Shift+A`, `F3`, `Ctrl+G`, `F1`), e as teclas
    únicas só com a preferência;
  - `AnnotClipboard` (copiar, recortar e colar com deslocamento, e a preferência pelo texto externo
    mais novo);
  - colar texto vira caixa de texto, com teto;
  - imagem acima do teto é recusada;
  - Reverter só com alterações;
  - pendências M6, M8, M10, M11 e M12.
- **E2E em Xvfb:**
  - Mão arrastando e Espaço segurado;
  - Zoom com marca;
  - `Ctrl+K` e ligar os aceleradores, depois `H`/`V`;
  - copiar e colar um comentário;
  - colar texto;
  - busca com Palavras inteiras e `F3`;
  - menu de contexto;
  - Reverter.
