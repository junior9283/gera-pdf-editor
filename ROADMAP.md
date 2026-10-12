# Roteiro

A meta da versão **1.0.0**: todas as ferramentas do Adobe Acrobat Pro, com qualidade no mínimo igual
(aparência, precisão e comportamento) e os mesmos atalhos.

## Concluído

- [x] v1 — visualizador leve com anotações (0.1.0)
- [x] v1.1-A — interface no estilo do Acrobat (0.2.0)
- [x] v1.1-B1 — texto, impressão, propriedades (0.3.0)
- [x] v1.1-B2 — anotações editáveis, formas, carimbos, painel Comentários (0.4.0)
- [x] v2 — organizar, dividir e combinar (0.5.0)
- [x] v3 — formulários, preencher e assinar, certificados (0.6.0)
- [x] v5 — proteger, comprimir, criar/exportar, OCR (0.7.0)
- [x] v4a — editar imagens (0.8.0)
- [x] v4b — editar texto (0.9.0)
- [x] v2c — marca d'água, cabeçalho e rodapé, Bates (0.10.0)
- [x] v5e — redigir e limpar documento (0.11.0)
- [x] v5f — comparar arquivos (0.12.0)
- [x] v6a — navegar e exibir: links, histórico de vistas, tela cheia, modo leitura, zoom até 6400 % (0.13.0)
- [x] v6b — Mão, Zoom com marca, Preferências, Recortar/Colar, busca com opções (0.14.0)
- [x] v6c — Propriedades editáveis, painéis Marcadores e Anexos (0.15.1)
- [x] v6c2 — Miniaturas, Camadas, Destinos, rótulos de página, copiar páginas entre PDFs (0.16.0)
- [x] v6d — ferramentas de comentário: inserir e substituir texto, nuvem, polígono, texto explicativo,
  borracha, anexar arquivo (0.17.0)
- [x] v6d2 — carimbos personalizados, FDF/XFDF, Resumir comentários, agrupar (0.18.0)

## Em andamento

- Avaliação para a 1.0.0 concluída: 32 lacunas P1 e 71 P2 em relação ao Acrobat Pro
  ([análise](docs/superpowers/specs/2026-10-10-gera-pdf-editor-avaliacao-1.0.md)).
- [x] v6a — navegar e exibir (0.13.0)
- [x] v6b — ferramentas de tela, preferências, menus e busca (0.14.0)
- [x] v6c — Propriedades editáveis, painéis Marcadores e Anexos (0.15.0)
- [x] v6c2 — Miniaturas, Camadas, Destinos, rótulos de página (0.16.0)
- [x] v6d — ferramentas de comentário (0.17.0)
- [x] v6d2 — carimbos personalizados, FDF/XFDF, Resumir comentários, agrupar (0.18.0)
- [x] v6e — Formatar, Validar e Calcular; Botão e Data; ordem de tabulação (0.19.0)
- [x] v6e2 — organizar campos, ações de campo e de página, campo Imagem (0.20.0)
- [x] v6e3 — detecção automática de campos e campo Código de barras (0.21.1)
- [x] v6e4 — dados do formulário, Enviar, nivelar e preenchimento automático (0.22.0)

## Planejado

- [ ] v6c3 — pendências dos painéis: visibilidade de camadas pela API do MuPDF (olho em PDF
  assinado), /PageMode /UseOC abre Camadas, grupos exclusivos de camadas, Delete no painel Destinos
- [ ] v6f — próxima: editar PDF: painel Formatar, ferramenta Link, plano de fundo
- [ ] v6g — converter: criar de Office, exportar Excel/PowerPoint/HTML
- [ ] v6h a v6n — impressão de livreto/pôster e Otimizador, ortografia e digitalização, Medir e réguas,
  certificados avançados, acessibilidade, PDF/A-X-UA e Preflight, pesquisa em vários arquivos
- [ ] Comparar arquivos: visão "só antigo / só novo", filtros de cabeçalho/rodapé, fundo e
  anotações, bandas ligando os dois lados
- [ ] Melhorias pendentes:
  - progresso do Exportar;
  - object streams no Comprimir;
  - impressão em alta resolução sempre;
  - escolha de páginas no Proteger;
  - OCR sem prender as outras operações;
  - comprimir os fluxos regravados ao salvar;
  - Editar texto: alinhamento centralizado/à direita, painel Formatar (fonte, tamanho, cor),
    fonte do sistema parecida antes da padrão, subconjunto da fonte embutida ao salvar.
- [ ] Interface em inglês (internacionalização)
- [ ] Pacotes Flatpak e .deb/.rpm

Sugestões? Abra uma [issue](https://github.com/junior9283/gera-pdf-editor/issues/new/choose).
