# Avaliação para a 1.0.0: lacunas entre o Acrobat Pro e o Gera PDF Editor

Data: 10/10/2026 · Base: `main` em `24ccba3` (0.12.0) · Acrobat Pro: interface nova (2023–2026) e menus clássicos.

## Como ler

- **Estado:** *ausente*, ou *parcial*, com o motivo e o `arquivo:linha` do que já existe. Os caminhos são
  relativos a `crates/`.
- **P1:** é comum no uso diário ou aparece na interface principal; sem isso não dá para dizer "igual ao
  Acrobat". **P2:** importante, mas de nicho. **P3:** raro ou dependente de serviço online da Adobe;
  é *fora de escopo sugerido*, com o motivo.
- **Fonte:** as siglas entre colchetes levam às URLs da seção [Fontes](#fontes). As páginas
  `helpx.adobe.com` recusam acesso automatizado (HTTP 403), então elas foram lidas pelas cópias do
  web.archive.org (2025–2026).
- Ausências conferidas por grep. Exemplos: não há tratamento de `/Link` em `pdf-core/src/doc.rs`,
  `gera-ui/src/view.rs` e `doc_tab.rs`; não existem `Caret`, `Polygon`, `PageLabels`, `DocMDP`,
  `EmbeddedFile` (fora da redação) nem verificação ortográfica.

O que já existe e não entra como lacuna: abas e Início, central Ferramentas com as 19 ferramentas
(`app-state/src/tools_catalog.rs:84-104`), os 4 modos de página com capa (`app-state/src/layout.rs:8-33`),
giro da vista, tela cheia, miniaturas, marcadores (só leitura), painéis Anotações e Assinaturas,
busca, impressão, comentários editáveis com painel e respostas, organizar/dividir/combinar,
formulários, preencher e assinar, certificados (com lista de confiáveis), proteger, comprimir,
criar a partir de imagens, exportar para Word/texto/imagens, OCR, redigir e limpar, marca d'água,
cabeçalho/rodapé/Bates, comparar e editar texto e imagens.

---

## 1. Interface: menus, barras e janelas

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| M01 | Itens de menu Arquivo → Exportar PDF e Assinar → Preencher e assinar / Certificados | **Parcial (defeito):** aparecem desativados, com "·" vazio, embora as ferramentas estejam prontas. O motivo: `soon_tool` é chamado para ferramentas `Ready` (`gera-ui/src/shell/menus.rs:46-53`, `:79`, `:160-161`) | P1 | [ws] |
| M02 | Editar → Recortar, Colar, Desmarcar tudo (`Ctrl+X`, `Ctrl+V`, `Ctrl+Shift+A`); colar comentários, campos, imagens (vira carimbo) e texto | **Ausente:** o menu Editar só tem Copiar, Selecionar tudo e Excluir (`shell/menus.rs:87-96`) | P1 | [ks] |
| M03 | Preferências (`Ctrl+K`): Geral (aceleradores de tecla única, Mão seleciona texto), Exibição de página (layout e zoom padrão, suavização), Documentos (restaurar a última vista, número de recentes), Tela cheia, Comentários, Formulários, Unidades e guias, Identidade | **Parcial:** só Modo escuro e Identidade (`shell/menus.rs:97-100`) | P1 | [view] |
| M04 | Arquivo → Reverter, Criar PDF, Exportar para…, Proteger com senha | **Parcial:** o menu tem Abrir, Recentes, Salvar, Salvar como, Propriedades, Imprimir, Fechar e Sair (`shell/menus.rs:60-86`) | P1 | [ws] |
| M05 | Arquivo → Salvar como outro (PDF otimizado, tamanho reduzido, PDF/A, PDF/X) | **Ausente** | P2 | [optim] |
| M06 | Exibir → Navegação de páginas (primeira, anterior, próxima, última, Ir para página, Vista anterior e seguinte) | **Ausente no menu:** Home/End/PgUp/PgDn funcionam só pelo teclado (`gera-ui/src/doc_tab.rs:911-937`) | P1 | [nav] |
| M07 | Exibir → Mostrar/ocultar barra de menus, barras, painéis (`F4`, `Shift+F4`), réguas e grades, coordenadas do cursor | **Parcial:** só o submenu Painéis (`shell/menus.rs:144-155`) | P2 | [view] |
| M08 | Janela → Nova janela, Dividir, Divisão de planilha, Remover divisão | **Ausente:** Janela lista as abas e Fechar todas (`shell/menus.rs:163-176`) | P2 | [adj] |
| M09 | Ajuda → Ajuda do Acrobat (`F1`), Tutoriais, Relatório do sistema | **Parcial:** só Atalhos e Sobre (`shell/menus.rs:177-180`), e a janela de atalhos não lista as teclas tratadas fora da tabela (`gera-ui/src/shell/help.rs:4-22`) | P2 | [ks] |
| M10 | Barra de ações rápidas flutuante e personalizável (Selecionar, Mão, comentar, destacar, desenhar, preencher) | **Parcial:** barra fixa (`gera-ui/src/shell/toolbar.rs:13-58`), sem personalização | P2 | [qa] |
| M11 | Menu de contexto na página vazia (Selecionar, Mão, Zoom, Girar, Imprimir, Propriedades, Localizar) | **Parcial:** só sobre a seleção ou os comentários (`gera-ui/src/edit.rs:617-660`) e sobre as páginas no Organizar (`gera-ui/src/organize.rs:369`) | P2 | [ks] |
| M12 | Painéis à direita na interface nova, personalizáveis e com "Redefinir" | **Diferente:** os painéis ficam à esquerda, em conjunto fixo (`app-state/src/prefs.rs:11-25`) | P3 (*fora de escopo sugerido*: o arranjo clássico do Acrobat é equivalente; não falta função) | [side] |
| M13 | Assistente de IA, Resumo generativo, PDF Spaces, Adobe Express, notificações e conta Adobe | **Ausente** | P3 (*fora de escopo*: dependem da nuvem da Adobe) | [ai] |

## 2. Visualização e navegação

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| V01 | Seguir links: para página/destino, para URL (com aviso de segurança) e para outro PDF | **Ausente:** não há leitura de `/Link` (nem em `pdf-core/src/doc.rs` nem em `gera-ui/src/view.rs`) | P1 | [links] [secwarn] |
| V02 | Ir para página (`Ctrl+Shift+N`) e rótulos de página no campo ("iv (4 de 20)") | **Parcial:** campo numérico (`gera-ui/src/doc_tab.rs:1669-1687`), sem atalho e sem rótulos | P1 | [nav] [ks] |
| V03 | Vista anterior e seguinte (`Alt+←`, `Alt+→`), com histórico de saltos | **Ausente** | P1 | [nav] |
| V04 | `←`/`→` e `Ctrl+PgUp`/`Ctrl+PgDn` mudam de página em qualquer modo; `Enter`/`Shift+Enter` avançam e voltam uma tela; `Shift+Ctrl+PgUp/PgDn` vão à primeira e à última | **Parcial:** as setas só funcionam nos modos página a página (`doc_tab.rs:940-955`) | P1 | [ks] |
| V05 | Tela cheia com `Ctrl+L`; `Esc` sai; clicar avança e o botão direito volta; barra de navegação | **Parcial:** só `F11` (`gera-ui/src/shell/commands.rs:124-127`) e `Esc` (`gera-ui/src/app.rs:832-836`) | P1 | [view] |
| V06 | Preferências de tela cheia: transições, avanço automático a cada N s, repetir após a última, cor de fundo, cursor | **Ausente** | P2 | [view] |
| V07 | Modo leitura (`Ctrl+H`), com barra flutuante | **Ausente** | P1 | [view] |
| V08 | Zoom: lista de níveis (8,33 % a 6400 %), porcentagem digitada, Zoom para… (`Ctrl+Y`), Ajustar à altura, Ajustar ao conteúdo visível (`Ctrl+3`) | **Parcial:** o menu ⏷ só tem largura, página e tamanho real; a porcentagem é só um rótulo (`shell/toolbar.rs:25-40`) | P1 | [adj] |
| V09 | Ferramenta Mão (`H`; `Espaço` temporário) que arrasta a página | **Ausente:** só há Selecionar (`app-state/src/tools.rs:4`); o `Espaço` vira página nos modos paginados (`doc_tab.rs:946`) | P1 | [ks] [adj] |
| V10 | Zoom com marca (`Z`): arrastar um retângulo para ampliar | **Ausente** | P1 | [adj] |
| V11 | Zoom dinâmico, Lupa e Panorâmica e zoom (`Shift+Z` alterna) | **Ausente** | P2 | [adj] |
| V12 | Rolagem automática (`Ctrl+Shift+H`) | **Ausente** | P2 | [nav] |
| V13 | Réguas, grade, guias, Encaixar na grade, coordenadas do cursor | **Ausente** | P2 | [grid] |
| V14 | Espessura de linhas, caixas de arte/corte/sangria, grade de transparência, visualização de sobreimpressão | **Ausente** | P3 (*fora de escopo sugerido*: só produção gráfica usa) | [view] |
| V15 | Refluir (`Ctrl+4`) | **Ausente** | P2 | [ks] [a11yfeat] |
| V16 | Ler em voz alta (`Shift+Ctrl+Y/V/B/C/E`) | **Ausente** (no Linux daria para usar speech-dispatcher ou espeak-ng) | P2 | [ks] |
| V17 | Artigos: painel e leitura das sequências | **Ausente** | P2 | [nav] |
| V18 | Respeitar a Visualização inicial do arquivo (`/PageMode`, `/PageLayout`, `/OpenAction`: abrir com marcadores, em duas páginas, em tela cheia) | **Ausente:** essas chaves só aparecem na limpeza (`pdf-core/src/redact.rs`) | P2 | [props] |
| V19 | Localizar: opções (palavras inteiras, maiúsculas/minúsculas, incluir marcadores e comentários); `F3`/`Ctrl+G` vão ao próximo resultado; `Enter` avança | **Parcial:** `Enter` reinicia a busca, sem opções, e só há ⏶⏷ (`doc_tab.rs:1690-1714`) | P1 | [search] [ks] |
| V20 | Pesquisa avançada (`Ctrl+Shift+F`) em vários PDFs ou numa pasta, com lista de resultados (`Alt+Shift+←/→`) | **Ausente** | P2 | [search] |
| V21 | Índice de catálogo (.pdx) | **Ausente** | P3 (*fora de escopo sugerido*: recurso corporativo legado; V20 cobre o uso real) | [index] |
| V22 | Ferramenta Instantâneo: copiar uma área como imagem | **Ausente** | P2 | [view] |
| V23 | Selecionar texto pelo teclado (`Shift+setas`, `Shift+Ctrl+→`) | **Ausente:** as setas só movem objetos (`doc_tab.rs:2262-2265`) | P2 | [ks] |

## 3. Painéis laterais

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| P01 | Miniaturas: arrastar para reordenar ou copiar, menu (girar, excluir, extrair, inserir, substituir, recortar, imprimir), tamanho da miniatura, caixa de vista arrastável | **Parcial:** clicar só leva à página (`gera-ui/src/sidebar.rs:88-131`); as operações ficam só no Organizar | P1 | [thumbs] [adj] |
| P02 | Marcadores: novo (`Ctrl+B`, também a partir do texto selecionado), renomear, excluir, arrastar e aninhar, recolher e expandir (`→`, `←`, `Shift+*`, `/`), propriedades (cor, estilo, ação) | **Parcial:** lista só de leitura, sem recolher (`sidebar.rs:134-160`) | P1 | [thumbs] [ks] |
| P03 | Anexos: listar, abrir, salvar, adicionar, excluir, descrição | **Ausente:** os anexos só são removidos pelo Limpar documento (`pdf-core/src/redact.rs`) | P1 | [attach] [openattach] |
| P04 | Camadas (OCG): mostrar e ocultar, propriedades, mesclar, achatar, imprimir conforme a visibilidade | **Ausente** | P2 | [printlayers] |
| P05 | Destinos nomeados: listar, criar, ir, ordenar | **Ausente** | P2 | [dest] |
| P06 | Tags, Conteúdo e Ordem (`F2` edita a tag) | **Ausente** (ver também A03) | P2 | [tags] |
| P07 | Árvore de modelos (3D) | **Ausente** | P3 (*fora de escopo sugerido*: conteúdo 3D é raro e o MuPDF não o desenha) | [media] |

## 4. Comentar

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| C01 | Inserir texto e Substituir texto (cursor de inserção, `Caret`, mais riscado agrupado) | **Ausente:** `AnnotKind` não tem `Caret` (`pdf-core/src/types.rs:95-111`) | P1 | [insert] [replace] |
| C02 | Riscar como ferramenta da barra (`Shift+U` alterna entre destacar, sublinhar e riscar) | **Parcial:** riscar só pelo menu de contexto da seleção (`gera-ui/src/edit.rs:669`); falta na linha Comentar (`app-state/src/tools.rs:25-37`) | P1 | [ks] [markups] |
| C03 | Nuvem (`Q`), polígono e polilinha | **Ausente:** só há retângulo, elipse, linha e seta (`app-state/src/tools.rs:2-22`) | P1 | [shapes] [ks] |
| C04 | Texto explicativo (callout) | **Ausente** | P1 | [callout] |
| C05 | Borracha do lápis | **Ausente** | P2 | [shapes] |
| C06 | Anexar arquivo como comentário (`Shift+J` alterna) | **Ausente** | P2 | [attcom] |
| C07 | Gravar áudio como comentário | **Ausente** | P3 (*fora de escopo sugerido*: uso raro; exige captura e reprodução de áudio) | [ks] |
| C08 | Carimbos personalizados (de imagem ou PDF), dinâmicos personalizados, categorias próprias | **Parcial:** catálogo fixo (`app-state/src/stamps.rs:51-96`) | P2 | [stamps] |
| C09 | Exportar e importar comentários (FDF/XFDF), Resumir comentários (PDF), imprimir com o resumo | **Ausente** | P2 | [comment] |
| C10 | Verificar a ortografia de comentários e caixas de texto (`F7`) | **Ausente** | P2 | [spellc] |
| C11 | Agrupar e desagrupar comentários | **Ausente** | P2 | [group] |
| C12 | Reações, revisão compartilhada, publicar comentários, rastreador de revisões, comentários em vídeo | **Ausente** | P3 (*fora de escopo*: dependem da nuvem ou de servidor da Adobe) | [reactions] [trackrev] |

## 5. Formulários

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| F01 | Detecção automática de campos ao abrir Preparar formulário | **Ausente** | P1 | [toforms] |
| F02 | Campos Botão e Data (com seletor de data) | **Ausente:** só há 6 tipos (`app-state/src/prepare.rs:10-28`) | P1 | [fieldprops] |
| F03 | Formatar, Validar e Calcular (número, moeda, %, data, máscaras; soma, média, produto, mín./máx., notação simplificada), executados ao preencher | **Ausente:** as propriedades não têm essas abas (`gera-ui/src/shell/field_props.rs:70-120`), e os scripts dos campos não rodam, só um aviso (`gera-ui/src/fields.rs:451`) | P1 | [fieldfmt] [calc] |
| F04 | Campos Imagem e Código de barras | **Ausente** | P2 | [barcode] |
| F05 | Ações de campo e de página (ir para página, abrir link, mostrar/ocultar campo, redefinir, enviar, JavaScript) | **Ausente** | P2 | [fieldprops] |
| F06 | Ordem de tabulação (`Shift+N`) e Destacar campos (`Shift+L`) | **Parcial:** `Tab` navega (`fields.rs:237-238`), mas não dá para ver nem editar a ordem, nem destacar | P2 | [navfield] [ks] |
| F07 | Alinhar, centralizar, distribuir, igualar tamanho (`L R T B H V`); seleção múltipla; duplicar em páginas; várias cópias; guias (`G`); visualizar (`P`) | **Ausente** | P2 | [align] [copyf] [ks] |
| F08 | Importar e exportar dados (FDF, XFDF, XML, TXT); mesclar dados num arquivo de planilha | **Ausente** | P2 | [formsov] |
| F09 | Nivelar campos e comentários | **Ausente** | P2 | [preflight] |
| F10 | Preenchimento automático (sugestões a partir do que já foi digitado) | **Ausente** | P2 | [autofill] |
| F11 | Formulários XFA dinâmicos | **Ausente:** o MuPDF não suporta | P3 (*fora de escopo sugerido*: formato legado, descontinuado no PDF 2.0) | [xfa] |
| F12 | Habilitar direitos do Reader; distribuir e rastrear formulários; enviar para e-assinatura | **Ausente** | P3 (*fora de escopo*: são específicos do Adobe Reader ou da nuvem) | [shareforms] |

## 6. Editar PDF

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| E01 | Painel Formatar texto: fonte, tamanho, cor, negrito/itálico/sublinhado, alinhamento, entrelinha, espaçamento, escala horizontal | **Ausente:** pendente no `ROADMAP.md:36-37` | P1 | [format] |
| E02 | Adicionar e editar links (ferramenta `L`): área, aparência, ação (página, URL, arquivo), mover e redimensionar; Criar links a partir de URLs | **Ausente** | P1 | [links] [linkfmt] [linkmove] |
| E03 | Verificar a ortografia ao editar (`F7`), com dicionário de palavras aceitas | **Ausente** | P2 | [spell] |
| E04 | Listas numeradas e com marcadores | **Ausente** | P2 | [lists] |
| E05 | Adicionar objetos (linhas, retângulos e elipses como conteúdo); organizar (frente e trás); inverter, recortar e girar imagens livremente | **Parcial:** as imagens giram só de 90° (`CHANGELOG.md:94-97`), e não há formas de conteúdo | P2 | [objs] [modimg] |
| E06 | Plano de fundo (cor ou arquivo; atualizar e remover) | **Ausente:** a marca d'água "atrás" cobre só parte do caso (`gera-ui/src/shell/watermark_dialog.rs`) | P2 | [bg] |
| E07 | Fonte padrão para o texto novo | **Ausente** | P2 | [deffont] |
| E08 | Editar a imagem num aplicativo externo | **Ausente** | P3 (*fora de escopo sugerido*: raro; depende do aplicativo de imagens da máquina) | [extedit] |

## 7. Organizar páginas

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| O01 | Numerar páginas (rótulos lógicos: i, ii, 1, A-1) | **Ausente** | P2 | [renum] |
| O02 | Mover ou copiar páginas entre dois PDFs arrastando as miniaturas | **Ausente** | P2 | [movebetween] |
| O03 | Inserir da área de transferência, de página web ou do scanner | **Ausente:** só há Inserir de arquivo e página em branco (`gera-ui/src/organize.rs:165-170`) | P2 | [insclip] [insweb] |
| O04 | Diálogo Girar páginas (`Ctrl+Shift+R`: intervalo, pares/ímpares, retrato/paisagem) e Excluir páginas (`Ctrl+Shift+D`) | **Parcial:** gira e exclui a seleção do Organizar (`organize.rs:110`, `:133-137`), sem atalho nem diálogo | P2 | [rotate] [ks] |

## 8. Criar, exportar, digitalizar e otimizar

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| X01 | Criar PDF a partir de Word, Excel, PowerPoint, ODF, RTF ou TXT (no Linux: LibreOffice headless) | **Ausente:** só a partir de imagens (`gera-ui/src/create.rs`) | P1 | [convert] |
| X02 | Exportar para Excel e PowerPoint | **Ausente:** os formatos são Word, texto, PNG e JPEG (`pdf-core/src/create_export.rs:188-196`) | P1 | [toxls] [toppt] |
| X03 | Exportar para HTML, RTF, PostScript/EPS, TIFF, texto acessível e XML | **Ausente** | P2 | [tohtml] [tortf] [tops] |
| X04 | Criar PDF a partir de página web ou URL | **Ausente** | P2 | [web] |
| X05 | Criar PDF da área de transferência; PDF em branco | **Ausente** | P2 | [clip] [scratch] |
| X06 | Digitalizar do scanner (SANE), com predefinições e várias páginas | **Ausente** | P2 | [scan] |
| X07 | Corrigir o texto reconhecido (palavras suspeitas do OCR) | **Ausente** | P2 | [fixocr] |
| X08 | Melhorar fotos de câmera (perspectiva, contraste, fundo) | **Ausente** | P2 | [enhance] |
| X09 | Otimizador de PDF (imagens, fontes, transparência, descartar objetos e dados, limpar) e Auditar o uso de espaço | **Parcial:** Comprimir com 3 níveis (`CHANGELOG.md:106`) | P2 | [optim] [audit] |
| X10 | Salvar para exibição rápida na Web (linearizar) | **Ausente** | P2 | [view] |
| X11 | Distiller, pastas monitoradas, impressora Adobe PDF | **Ausente** | P3 (*fora de escopo sugerido*: são fluxos do Windows; no Linux, a impressão para PDF já existe no CUPS) | [distiller] |

## 9. Proteger, assinar e certificados

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| S01 | Certificar o documento (DocMDP, com ou sem assinatura visível; alterações permitidas) | **Ausente** | P2 | [certify] |
| S02 | Carimbo de tempo RFC 3161 (servidor TSA) | **Ausente** | P2 | [tsa] |
| S03 | Validação de longo prazo (LTV): adicionar as informações de verificação (OCSP/CRL, DSS) | **Ausente** | P2 | [ltv] |
| S04 | Criptografar com certificado (destinatários) | **Ausente** | P2 | [certenc] |
| S05 | ID digital em token ou smart card (PKCS#11) | **Ausente:** só PKCS#12 e ID próprio (`README.md:58`) | P2 | [ids] |
| S06 | Códigos de redação e conjuntos de códigos | **Ausente** | P2 | [codes] |
| S07 | Avisos de segurança e modo protegido para links, anexos e JavaScript | **Ausente:** hoje não é preciso, mas V01, P03 e F03 passam a exigir | P1 (junto com V01, P03 e F03) | [secwarn] [pview] |
| S08 | Pedir assinaturas (Adobe Acrobat Sign), envio em massa, acordos; IDs roaming e servidores de diretório | **Ausente** | P3 (*fora de escopo*: serviço online da Adobe) | [esign] [roaming] |

## 10. Imprimir

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| I01 | Livreto (frente e verso, encadernação, folhas) | **Ausente:** o diálogo oferece Ajustar, Tamanho real, Reduzir, Escala personalizada e Páginas por folha (`gera-ui/src/shell/print_dialog.rs:160-215`) | P2 | [booklet] |
| I02 | Pôster ou faixa (lado a lado, sobreposição, marcas de corte) | **Ausente** | P2 | [poster] |
| I03 | Imprimir os marcadores selecionados, as camadas visíveis e o resumo de comentários; escolher o papel pelo tamanho da página do PDF | **Ausente** | P2 | [printbm] [printlayers] [mixed] |
| I04 | Predefinições de impressão gravadas no documento (escala, frente e verso, origem do papel) | **Ausente** | P3 (*fora de escopo sugerido*: raro) | [presets] |

## 11. Propriedades do documento

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| D01 | Editar título, autor, assunto e palavras-chave; metadados XMP | **Ausente:** a janela é só de leitura (`gera-ui/src/shell/properties.rs:41-90`) | P1 | [props] [meta] |
| D02 | Abas Segurança, Fontes, Visualização inicial e Personalizado; Avançado com URL base, idioma e opções de impressão | **Parcial:** só Descrição e um Avançado reduzido (`properties.rs:59-90`) | P1 | [props] [createprops] |

## 12. Acessibilidade

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| A01 | Verificação completa, com relatório, correção e explicação | **Ausente** | P2 | [a11y] |
| A02 | Marcar o documento automaticamente (autotag) | **Ausente** | P2 | [a11y] |
| A03 | Ferramenta Ordem de leitura (`Shift+Ctrl+U`); editar Tags e Conteúdo | **Ausente** | P2 | [tags] [ks] |
| A04 | Texto alternativo das figuras; idioma e título do documento | **Ausente** | P2 | [a11y] |
| A05 | Opções de leitura (`Shift+Ctrl+5`); substituir as cores do documento (alto contraste) | **Ausente** | P2 | [a11yfeat] [ks] |
| A06 | Foco pelo teclado entre as áreas (`F6`, `Shift+F6`, `F5`, `Ctrl+Shift+F5`, `Shift+F10`) | **Ausente** | P2 | [ks] |

## 13. Padrões, Preflight e produção gráfica

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| R01 | Converter para PDF/A (1b, 2b, 3b), PDF/X e PDF/UA e validar; modo de exibição PDF/A | **Ausente** | P2 | [std] |
| R02 | Preflight: perfis, relatório, correções | **Ausente** | P2 | [preflight] |
| R03 | Definir as caixas de página (corte, sangria, arte) | **Ausente:** as caixas só são preservadas ao copiar páginas (`pdf-core/src/pages.rs:14-24`) | P2 | [printprod] |
| R04 | Visualização de saída e separações (`~`) | **Ausente** | P3 (*fora de escopo sugerido*: nicho gráfico) | [printprod] [ks] |
| R05 | Converter cores, achatar transparência, corrigir linhas finas, marcas de impressora, trapping, JDF | **Ausente** | P3 (*fora de escopo sugerido*: nicho gráfico) | [printprod] |

## 14. Medir, automação, JavaScript, mídia e serviços

| # | Acrobat | Estado no app | P | Fonte |
|---|---|---|---|---|
| J01 | Medir: distância, perímetro e área; escala e precisão; encaixe em caminhos e pontos; painel de informações; exportar para CSV | **Ausente** | P2 | [grid] |
| J02 | Ações guiadas (Assistente de ações): criar e rodar sequências em vários arquivos | **Ausente** | P2 | [actions] |
| J03 | JavaScript: scripts do documento, console e depurador (`Ctrl+J`), ações de página | **Ausente:** a execução nos formulários é tratada em F03 | P2 | [jsapi] [ks] |
| J04 | Rich media: vídeo, som, 3D, SWF | **Ausente** | P3 (*fora de escopo sugerido*: o Flash está morto; vídeo e 3D em PDF são raros e o MuPDF não os reproduz) | [media] |
| J05 | Portfólio PDF: ver e criar | **Ausente** | P3 (*fora de escopo sugerido*: formato legado; com P03 os arquivos já ficam acessíveis) | [portfolio] |
| J06 | Compartilhar link, enviar para revisão, armazenamento na nuvem, enviar por e-mail | **Ausente** | P3 (*fora de escopo*: dependem do Document Cloud) | [share] |
| J07 | Personalizar ferramentas e criar conjuntos de ferramentas | **Ausente** | P3 (*fora de escopo sugerido*: conveniência; M10 cobre o essencial) | [qa] |

---

## 15. Atalhos de teclado que faltam ou diferem

A tabela do app está em `gera-ui/src/shell/commands.rs:83-129`, e a navegação, em `gera-ui/src/doc_tab.rs:911-955`.
Fonte de todos os itens: [ks]. A exceção é `Ctrl+L`, que vem de [view]. No Acrobat, os atalhos de tecla única
dependem de "Usar aceleradores de tecla única" nas Preferências.

**Diferentes**

| Ação | Acrobat | App |
|---|---|---|
| Tela cheia | `Ctrl+L` (e `Esc` sai) | `F11` (e `Esc` sai) |
| Página anterior e seguinte | `←`/`→` em todos os modos | só nos modos página a página |
| `Espaço` | Mão temporária e rolar | vira a página nos modos paginados |
| `Enter` na caixa de busca | vai ao próximo resultado | refaz a busca |

**Faltam: comandos com Ctrl, Alt ou F**

| Atalho | Ação | Atalho | Ação |
|---|---|---|---|
| `Ctrl+Shift+N` | Ir para página | `Ctrl+3` | Ajustar ao conteúdo visível |
| `Alt+←` / `Alt+→` | Vista anterior / seguinte | `Ctrl+4` | Refluir |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Página anterior / seguinte | `Ctrl+Y` | Zoom para… |
| `Shift+Ctrl+PgUp/PgDn`, `Shift+Ctrl+↑/↓` | Primeira / última página | `Ctrl+H` | Modo leitura |
| `Enter` / `Shift+Enter` | Tela seguinte / anterior | `Ctrl+Shift+H` | Rolagem automática |
| `F3` / `Ctrl+G` | Próximo resultado da busca | `Ctrl+K` | Preferências |
| `Ctrl+Shift+F` | Pesquisa avançada | `Ctrl+R` / `Ctrl+U` | Réguas / grade |
| `Ctrl+X` / `Ctrl+V` | Recortar / colar | `Ctrl+B` | Novo marcador |
| `Ctrl+Shift+A` | Desmarcar tudo | `Ctrl+J` | Console JavaScript |
| `Ctrl+Shift+T` | Inserir página em branco | `F7` | Verificar ortografia |
| `Ctrl+Shift+I` | Inserir arquivos | `F1` | Ajuda |
| `Ctrl+Shift+D` | Excluir páginas | `F4` / `Shift+F4` | Painel de navegação / de ferramentas |
| `Ctrl+Shift+R` | Girar páginas | `F6` / `Shift+F6` | Foco na próxima / anterior área |
| `Ctrl+F6` / `Ctrl+Shift+F6` | Próximo / anterior documento | `F5`, `Ctrl+Shift+F5` | Foco no documento / nos painéis |
| `Ctrl+F4` | Fechar documento | `Shift+F10` | Menu de contexto |
| `Shift+Ctrl+U` | Ordem de leitura | `F2` | Editar tag |
| `Shift+Ctrl+5` | Opções de leitura | `Shift+Ctrl+Y/V/B/C/E` | Ler em voz alta (ligar, página, documento, pausar, parar) |

**Faltam: teclas únicas (ferramentas, comentários, formulários e painéis)**

| Tecla | Ação | Tecla | Ação |
|---|---|---|---|
| `V` | Selecionar | `S` | Nota |
| `H` / `Espaço` | Mão / Mão temporária | `U` / `Shift+U` | Destaque atual / alternar destacar, sublinhar, riscar |
| `Z` / `Shift+Z` | Zoom com marca / alternar Zoom dinâmico e Lupa | `D` / `Shift+D` | Desenho atual / alternar desenhos |
| `Ctrl+Espaço` | Ampliar temporariamente | `Q` | Nuvem |
| `R` | Selecionar objeto | `X` | Caixa de texto |
| `O` | Editar objeto | `K` (ou `J`) / `Shift+J` | Carimbo / alternar carimbo, anexo, áudio |
| `T` / `Shift+T` | Editar texto / alternar retoques | `Shift+K` | Marcar o comentário com visto |
| `A` | Entrar e sair da edição de formulário | `R` (na lista) | Responder ao comentário |
| `F` / `Shift+F` | Campo de texto / alternar tipos de campo | `Tab` / `Shift+Tab` | Próximo comentário, link ou campo no documento |
| `C` | Recortar página | `P`, `G` | Visualizar o formulário, guias |
| `L` | Link | `L R T B H V`, `Shift+H/V` | Alinhar e centralizar campos |
| `Shift+Y` | Redigir | `Shift+L` / `Shift+N` / `Shift+D` | Destacar campos / ordem de tabulação / JS do documento |
| `M` / `Shift+M` | 3D e multimídia (P3) | `→` `←` `Shift+*` `/` | Expandir, recolher e expandir tudo nos marcadores |
| `~` | Visualização de saída (P3) | | |

---

## 16. Contagem

| Prioridade | Lacunas |
|---|---|
| P1 | 32 |
| P2 | 71 |
| P3 | 19 (*fora de escopo sugerido*) |
| **Total** | **122** |

Os atalhos da seção 15 não entram na contagem: cada um pertence à etapa da funcionalidade que ele aciona.

## 17. Etapas propostas (P1 e P2)

Cada etapa cabe num ciclo spec → plano → TDD. As de P1 vêm primeiro, e a ordem segue o uso diário e as
dependências. Por exemplo, os links (v6a) vêm antes dos avisos de segurança e da edição de links (v6f);
a ferramenta Mão e os aceleradores (v6b) vêm antes das ferramentas de comentário com tecla única (v6d).

| Ordem | Etapa | Conteúdo | Itens |
|---|---|---|---|
| 1 | **v6a — Navegar e exibir** | Seguir links, com aviso para URL e outro arquivo; Ir para página; Vista anterior e seguinte; setas e `Ctrl+PgUp/PgDn` em qualquer modo; `Enter`/`Shift+Enter`; Exibir → Navegação de páginas; tela cheia com `Ctrl+L`, cliques e barra; modo leitura; lista e digitação de zoom, Zoom para, Ajustar altura e Ajustar visível; respeitar a Visualização inicial; defeito dos menus desativados | V01–V05, V07, V08, V18, M01, M06, S07 (links) |
| 2 | **v6b — Ferramentas de tela, preferências e menus** | Ferramenta Mão e `Espaço`; Zoom com marca; aceleradores de tecla única; diálogo Preferências (`Ctrl+K`); Recortar e Colar (comentários, campos, imagens); Arquivo → Reverter, Criar e Exportar; busca com opções, `F3` e `Ctrl+G`; menu de contexto da página; Ajuda `F1`; Propriedades editáveis (com XMP) e as abas que faltam | V09, V10, V19, M02–M04, M09, M11, D01, D02 |
| 3 | **v6c — Painéis laterais** | Miniaturas com arrastar e menu; marcadores editáveis com teclas; painel Anexos (com aviso ao abrir); Camadas; Destinos; rótulos de página (com o campo de página); mover e copiar páginas entre PDFs | P01–P05, O01, O02, V02 (rótulos), S07 (anexos) |
| 4 | **v6d — Comentários completos** | Inserir e substituir texto; Riscar na barra; nuvem, polígono e polilinha; texto explicativo; borracha; anexar arquivo; carimbos personalizados; agrupar; exportar e importar FDF/XFDF; Resumir comentários | C01–C06, C08, C09, C11 |
| 5 | **v6e — Formulários avançados** | Detecção automática de campos; Botão e Data; Formatar, Validar e Calcular, com o subconjunto AFxxx do Acrobat num interpretador JS embutido e em sandbox (ex.: QuickJS ou Boa, compatíveis com a AGPL); ações de campo; ordem de tabulação; destacar campos; alinhar e distribuir; seleção múltipla; duplicar; importar e exportar dados; nivelar; Imagem e Código de barras; preenchimento automático | F01–F10, S07 (JS) |
| 6 | **v6f — Editar PDF: formatar, links e objetos** | Painel Formatar texto; ferramenta Link (criar, editar e criar a partir de URLs); fonte padrão; listas; objetos e formas; inverter e recortar imagens; plano de fundo; ferramenta Instantâneo | E01, E02, E04–E07, V22 |
| 7 | **v6g — Converter** | Criar PDF de arquivos do Office via LibreOffice headless; exportar para Excel, PowerPoint, HTML, RTF, PS e TIFF; criar de página web, da área de transferência e em branco; inserir da área de transferência e da web | X01–X05, O03 |
| 8 | **v6h — Impressão e otimização** | Livreto, pôster, marcadores, camadas e papel pela página; Otimizador de PDF e auditoria de espaço; Salvar como otimizado; linearizar; Salvar como outro | I01–I03, X09, X10, M05 |
| 9 | **v6i — Ortografia, digitalização e OCR** | Verificação ortográfica (Hunspell) na edição e nos comentários; digitalizar via SANE; corrigir o texto reconhecido; melhorar fotos de câmera | E03, C10, X06–X08 |
| 10 | **v6j — Medir e auxílios de visualização** | Réguas, grade, guias e coordenadas; ferramenta Medir; Lupa, Zoom dinâmico e Panorâmica; dividir janela e Nova janela; rolagem automática; preferências de tela cheia; mostrar/ocultar barras; barra de ações rápidas personalizável; seleção de texto pelo teclado; artigos; diálogo Girar e Excluir páginas | J01, V06, V11–V13, V17, V23, M07, M08, M10, O04 |
| 11 | **v6k — Certificados avançados** | Certificar (DocMDP), carimbo de tempo RFC 3161, LTV, criptografia por certificado, PKCS#11, códigos de redação | S01–S06 |
| 12 | **v6l — Acessibilidade** | Verificação completa, autotag, ordem de leitura, painéis Tags, Conteúdo e Ordem; texto alternativo; refluir; ler em voz alta (speech-dispatcher); opções de leitura e alto contraste; foco pelo teclado (`F6` etc.) | A01–A06, P06, V15, V16 |
| 13 | **v6m — Padrões e Preflight** | PDF/A, PDF/X e PDF/UA (converter e validar; o veraPDF, sob MPL/GPL, serve de referência para os testes); Preflight com perfis básicos; caixas de página | R01–R03 |
| 14 | **v6n — Automação e busca em vários arquivos** | Pesquisa avançada em pasta; Ações guiadas; scripts do documento e console JavaScript (reaproveitando o motor da v6e) | V20, J02, J03 |

As etapas v6a a v6g concentram todas as lacunas P1 e são a barra mínima para dizer "igual ao Acrobat" na
1.0.0. As etapas v6h a v6n fecham as P2.

---

## Fontes

Prefixo das páginas novas da Adobe: `https://helpx.adobe.com/acrobat/desktop/`.

[ks]: https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/keyboard-shortcuts.html
[ws]: https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/workspace.html
[nav]: https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/navigation.html
[side]: https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/customize-side-panels.html
[qa]: https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/customize-toolbar.html
[view]: https://helpx.adobe.com/acrobat/using/viewing-pdfs-viewing-preferences.html
[adj]: https://helpx.adobe.com/acrobat/using/adjusting-pdf-views.html
[grid]: https://helpx.adobe.com/acrobat/using/grids-guides-measurements-pdfs.html
[thumbs]: https://helpx.adobe.com/acrobat/using/page-thumbnails-bookmarks-pdfs.html
[search]: https://helpx.adobe.com/acrobat/using/searching-pdfs.html
[index]: https://helpx.adobe.com/acrobat/using/creating-pdf-indexes.html
[ai]: https://helpx.adobe.com/acrobat/desktop/use-acrobat-ai/get-started-with-generative-ai/acrobat-ai-overview.html
[links]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/link-web-pages.html
[linkfmt]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/format-links.html
[linkmove]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/move-resize-links.html
[attach]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/add-attachment.html
[openattach]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/open-attachment.html
[dest]: https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/manage-destinations.html
[props]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/pdf-properties.html
[createprops]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/create-properties.html
[meta]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/edit-metadata.html
[renum]: https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/renumber-pages.html
[movebetween]: https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/move-between-pdfs.html
[rotate]: https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/rotate-pages.html
[insclip]: https://helpx.adobe.com/acrobat/desktop/edit-documents/combine-files/insert-clipboard.html
[insweb]: https://helpx.adobe.com/acrobat/desktop/edit-documents/combine-files/insert-web-pages.html
[bg]: https://helpx.adobe.com/acrobat/desktop/edit-documents/add-backgrounds-and-watermarks/add-background.html
[format]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/format-text.html
[deffont]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/set-default-font.html
[lists]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/edit-lists.html
[spell]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/spell-check.html
[objs]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/add-objects.html
[modimg]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/modify-image.html
[extedit]: https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/edit-outside-acrobat.html
[insert]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/insert-text.html
[replace]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/replace-text.html
[markups]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-markups.html
[shapes]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-shapes-drawings.html
[callout]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-callouts.html
[attcom]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-attachment-comments.html
[group]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/group-markups.html
[stamps]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/use-stamps/create-custom-stamps.html
[spellc]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/spellcheck-comments.html
[reactions]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/add-reactions.html
[trackrev]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/track-reviews.html
[comment]: https://helpx.adobe.com/acrobat/using/commenting-pdfs.html
[toforms]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/convert-to-forms.html
[formsov]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/explore-basics/forms-overview.html
[fieldprops]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/field-properties.html
[fieldfmt]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/field-formatting.html
[calc]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/set-calculation-fields.html
[navfield]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/set-navigation.html
[align]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/align-fields.html
[copyf]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/copy-fields.html
[barcode]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/insert-barcodes/barcodes.html
[autofill]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/fill-sign-forms/auto-fill-forms.html
[xfa]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/explore-basics/forms-overview.html
[shareforms]: https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/share-forms/share-forms.html
[convert]: https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/convert-to-pdf.html
[web]: https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/web-pages-to-pdfs.html
[clip]: https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/clipboard-to-pdf.html
[scratch]: https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/create-pdf-scratch.html
[distiller]: https://helpx.adobe.com/acrobat/desktop/create-documents/explore-advanced-conversion-settings/acrobat-distiller-overview.html
[scan]: https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/scan.html
[fixocr]: https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/fix-scanned-text.html
[enhance]: https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/enhance-images.html
[optim]: https://helpx.adobe.com/acrobat/desktop/create-documents/optimize-pdfs/pdf-optimizer-settings.html
[audit]: https://helpx.adobe.com/acrobat/desktop/create-documents/optimize-pdfs/audit-space.html
[toxls]: https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-excel.html
[toppt]: https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-ppt.html
[tohtml]: https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-html.html
[tortf]: https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-rtf.html
[tops]: https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-ps.html
[certify]: https://helpx.adobe.com/acrobat/desktop/e-sign-documents/manage-digital-signatures/certify-pdfs.html
[tsa]: https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/add-time-stamps.html
[ltv]: https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/add-verification.html
[roaming]: https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/set-roaming-id.html
[esign]: https://helpx.adobe.com/acrobat/desktop/e-sign-documents/request-e-signatures/send-for-signing.html
[certenc]: https://helpx.adobe.com/acrobat/desktop/protect-documents/encrypt-with-certificates/certificate-encrypt.html
[ids]: https://helpx.adobe.com/acrobat/desktop/protect-documents/manage-digital-ids/register-ids.html
[codes]: https://helpx.adobe.com/acrobat/desktop/protect-documents/redact-pdfs/create-codes.html
[pview]: https://helpx.adobe.com/acrobat/desktop/protect-documents/use-protected-view/protect-view-mode.html
[secwarn]: https://helpx.adobe.com/acrobat/desktop/protect-documents/mitigate-security-risks/allow-block-links.html
[jsapi]: https://helpx.adobe.com/acrobat/desktop/protect-documents/mitigate-security-risks/restrict-javascript-api.html
[booklet]: https://helpx.adobe.com/acrobat/desktop/print-documents/booklets-posters-banners/print-booklets.html
[poster]: https://helpx.adobe.com/acrobat/desktop/print-documents/booklets-posters-banners/posters-banners.html
[printbm]: https://helpx.adobe.com/acrobat/desktop/print-documents/set-up-and-print-pdfs/bookmarked.html
[printlayers]: https://helpx.adobe.com/acrobat/desktop/print-documents/use-secure-and-special-print-mode/print-layers.html
[mixed]: https://helpx.adobe.com/acrobat/desktop/print-documents/set-up-and-print-pdfs/mixed-sizes.html
[presets]: https://helpx.adobe.com/acrobat/desktop/print-documents/print-duplex-and-multi-page-document/create-presets.html
[a11y]: https://helpx.adobe.com/acrobat/using/create-verify-pdf-accessibility.html
[a11yfeat]: https://helpx.adobe.com/acrobat/using/accessibility-features-pdfs.html
[tags]: https://helpx.adobe.com/acrobat/using/editing-document-structure-content-tags.html
[std]: https://helpx.adobe.com/acrobat/using/pdf-x-pdf-a-pdf.html
[preflight]: https://helpx.adobe.com/acrobat/using/preflight-profiles-acrobat-pro.html
[printprod]: https://helpx.adobe.com/sea/acrobat/using/print-production-tools-overview-acrobat.html
[actions]: https://helpx.adobe.com/acrobat/using/action-wizard-acrobat-pro.html
[media]: https://helpx.adobe.com/acrobat/using/adding-multimedia-pdfs.html
[portfolio]: https://helpx.adobe.com/acrobat/using/overview-pdf-portfolios.html
[share]: https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/share-documents/share-pdfs.html

Lista legível (as mesmas URLs das siglas):

- **ks**: <https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/keyboard-shortcuts.html>
- **ws**: <https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/workspace.html>
- **nav**: <https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/navigation.html>
- **side**: <https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/customize-side-panels.html>
- **qa**: <https://helpx.adobe.com/acrobat/desktop/get-started/preferences-and-settings/customize-toolbar.html>
- **view**: <https://helpx.adobe.com/acrobat/using/viewing-pdfs-viewing-preferences.html>
- **adj**: <https://helpx.adobe.com/acrobat/using/adjusting-pdf-views.html>
- **grid**: <https://helpx.adobe.com/acrobat/using/grids-guides-measurements-pdfs.html>
- **thumbs**: <https://helpx.adobe.com/acrobat/using/page-thumbnails-bookmarks-pdfs.html>
- **search**: <https://helpx.adobe.com/acrobat/using/searching-pdfs.html>
- **index**: <https://helpx.adobe.com/acrobat/using/creating-pdf-indexes.html>
- **ai**: <https://helpx.adobe.com/acrobat/desktop/use-acrobat-ai/get-started-with-generative-ai/acrobat-ai-overview.html>
- **links**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/link-web-pages.html>
- **linkfmt**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/format-links.html>
- **linkmove**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/move-resize-links.html>
- **attach**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/add-attachment.html>
- **openattach**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/open-attachment.html>
- **dest**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/use-links-and-attachments/manage-destinations.html>
- **props**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/pdf-properties.html>
- **createprops**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/create-properties.html>
- **meta**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-pdf-properties/edit-metadata.html>
- **renum**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/renumber-pages.html>
- **movebetween**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/move-between-pdfs.html>
- **rotate**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/organize-pages/rotate-pages.html>
- **insclip**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/combine-files/insert-clipboard.html>
- **insweb**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/combine-files/insert-web-pages.html>
- **bg**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/add-backgrounds-and-watermarks/add-background.html>
- **format**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/format-text.html>
- **deffont**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/set-default-font.html>
- **lists**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/edit-lists.html>
- **spell**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-text-in-pdfs/spell-check.html>
- **objs**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/add-objects.html>
- **modimg**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/modify-image.html>
- **extedit**: <https://helpx.adobe.com/acrobat/desktop/edit-documents/edit-images-or-objects/edit-outside-acrobat.html>
- **insert**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/insert-text.html>
- **replace**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/replace-text.html>
- **markups**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-markups.html>
- **shapes**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-shapes-drawings.html>
- **callout**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-callouts.html>
- **attcom**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/add-attachment-comments.html>
- **group**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/review-documents/group-markups.html>
- **stamps**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/use-stamps/create-custom-stamps.html>
- **spellc**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/spellcheck-comments.html>
- **reactions**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/add-reactions.html>
- **trackrev**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/manage-reviews/track-reviews.html>
- **comment**: <https://helpx.adobe.com/acrobat/using/commenting-pdfs.html>
- **toforms**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/convert-to-forms.html>
- **formsov**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/explore-basics/forms-overview.html>
- **fieldprops**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/field-properties.html>
- **fieldfmt**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/field-formatting.html>
- **calc**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/set-calculation-fields.html>
- **navfield**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/set-navigation.html>
- **align**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/align-fields.html>
- **copyf**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/copy-fields.html>
- **barcode**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/insert-barcodes/barcodes.html>
- **autofill**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/fill-sign-forms/auto-fill-forms.html>
- **xfa**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/explore-basics/forms-overview.html>
- **shareforms**: <https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/share-forms/share-forms.html>
- **convert**: <https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/convert-to-pdf.html>
- **web**: <https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/web-pages-to-pdfs.html>
- **clip**: <https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/clipboard-to-pdf.html>
- **scratch**: <https://helpx.adobe.com/acrobat/desktop/create-documents/create-pdfs/create-pdf-scratch.html>
- **distiller**: <https://helpx.adobe.com/acrobat/desktop/create-documents/explore-advanced-conversion-settings/acrobat-distiller-overview.html>
- **scan**: <https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/scan.html>
- **fixocr**: <https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/fix-scanned-text.html>
- **enhance**: <https://helpx.adobe.com/acrobat/desktop/create-documents/scan-documents-to-pdfs/enhance-images.html>
- **optim**: <https://helpx.adobe.com/acrobat/desktop/create-documents/optimize-pdfs/pdf-optimizer-settings.html>
- **audit**: <https://helpx.adobe.com/acrobat/desktop/create-documents/optimize-pdfs/audit-space.html>
- **toxls**: <https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-excel.html>
- **toppt**: <https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-ppt.html>
- **tohtml**: <https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-html.html>
- **tortf**: <https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-rtf.html>
- **tops**: <https://helpx.adobe.com/acrobat/desktop/save-export-documents/convert-to-other-formats/pdf-to-ps.html>
- **certify**: <https://helpx.adobe.com/acrobat/desktop/e-sign-documents/manage-digital-signatures/certify-pdfs.html>
- **tsa**: <https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/add-time-stamps.html>
- **ltv**: <https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/add-verification.html>
- **roaming**: <https://helpx.adobe.com/acrobat/desktop/e-sign-documents/fill-sign-documents/set-roaming-id.html>
- **esign**: <https://helpx.adobe.com/acrobat/desktop/e-sign-documents/request-e-signatures/send-for-signing.html>
- **certenc**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/encrypt-with-certificates/certificate-encrypt.html>
- **ids**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/manage-digital-ids/register-ids.html>
- **codes**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/redact-pdfs/create-codes.html>
- **pview**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/use-protected-view/protect-view-mode.html>
- **secwarn**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/mitigate-security-risks/allow-block-links.html>
- **jsapi**: <https://helpx.adobe.com/acrobat/desktop/protect-documents/mitigate-security-risks/restrict-javascript-api.html>
- **booklet**: <https://helpx.adobe.com/acrobat/desktop/print-documents/booklets-posters-banners/print-booklets.html>
- **poster**: <https://helpx.adobe.com/acrobat/desktop/print-documents/booklets-posters-banners/posters-banners.html>
- **printbm**: <https://helpx.adobe.com/acrobat/desktop/print-documents/set-up-and-print-pdfs/bookmarked.html>
- **printlayers**: <https://helpx.adobe.com/acrobat/desktop/print-documents/use-secure-and-special-print-mode/print-layers.html>
- **mixed**: <https://helpx.adobe.com/acrobat/desktop/print-documents/set-up-and-print-pdfs/mixed-sizes.html>
- **presets**: <https://helpx.adobe.com/acrobat/desktop/print-documents/print-duplex-and-multi-page-document/create-presets.html>
- **a11y**: <https://helpx.adobe.com/acrobat/using/create-verify-pdf-accessibility.html>
- **a11yfeat**: <https://helpx.adobe.com/acrobat/using/accessibility-features-pdfs.html>
- **tags**: <https://helpx.adobe.com/acrobat/using/editing-document-structure-content-tags.html>
- **std**: <https://helpx.adobe.com/acrobat/using/pdf-x-pdf-a-pdf.html>
- **preflight**: <https://helpx.adobe.com/acrobat/using/preflight-profiles-acrobat-pro.html>
- **printprod**: <https://helpx.adobe.com/sea/acrobat/using/print-production-tools-overview-acrobat.html>
- **actions**: <https://helpx.adobe.com/acrobat/using/action-wizard-acrobat-pro.html>
- **media**: <https://helpx.adobe.com/acrobat/using/adding-multimedia-pdfs.html>
- **portfolio**: <https://helpx.adobe.com/acrobat/using/overview-pdf-portfolios.html>
- **share**: <https://helpx.adobe.com/acrobat/desktop/share-and-review-documents/share-documents/share-pdfs.html>
