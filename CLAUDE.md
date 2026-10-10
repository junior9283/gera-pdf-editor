# Gera PDF Editor — orientações do dono do projeto

Editor de PDF desktop para Linux, em Rust. Este arquivo reúne as orientações dadas pelo dono do projeto;
elas valem para toda sessão que trabalhar aqui.

## Objetivo e barra de qualidade

- O app deve ter **todas as funcionalidades do Adobe Acrobat Pro**, incluindo as barras, abas e
  **ferramentas em tela**.
- A **qualidade** de cada funcionalidade e do resultado deve ser **no mínimo igual à do Acrobat Pro**
  (aparência, precisão e comportamento). Não aceitar "quase": quando a biblioteca limitar, contornar ou
  programar.
- Implementação **original**: sem código, ícones ou recursos da Adobe.
- **Atalhos e comportamentos iguais aos do Acrobat**: pesquisar o que o Acrobat faz e copiar, em vez de
  inventar.

## Autonomia

- O dono pode não estar na frente do PC. **Executar o roteiro inteiro sem perguntar**, seguindo assim
  até o fim do projeto: tomar as decisões de design e escopo sozinho, registrá-las (spec e ledger) e
  aprovar as próprias specs e planos.
- Em caso de dúvida sobre como fazer uma funcionalidade: verificar se existe **ferramenta/biblioteca open
  source** que a entregue com qualidade de Acrobat Pro (licença compatível com AGPL-3.0); se não houver,
  **programar**.
- **Os testes são feitos pelo Claude**, inclusive os de interface (E2E em Xvfb + openbox + xdotool, sem
  D-Bus e com `XDG_CONFIG_HOME` temporário), rodando quantas baterias forem necessárias. Não pedir teste
  manual.
- Execução **nativa** (o próprio Claude implementa cada tarefa na sessão; só a revisão final da etapa usa
  um revisor separado): mais rápido e mais barato.

## Roteiro

v1 (leitor) → v1.1-A (casca do Acrobat) → v1.1-B1 (texto, impressão, propriedades) → v1.1-B2a
(anotações editáveis) → **v1.1-B2b** (caixa de texto, formas, linhas, carimbos) → v1.1-B2c (painel
Comentários com respostas) → v2 (organizar páginas) → v3 (formulários e assinatura) → v5 (converter, OCR,
proteger, comprimir) → v4 (editar texto e imagens).

## Fluxo de cada etapa

1. Spec → plano → implementação com TDD (tarefa por tarefa, com commits).
2. **`simplify` só no fim da etapa**, uma vez, sobre `git diff main...HEAD -- crates scripts`
   (não a cada tarefa).
3. Baterias E2E, `scripts/measure.sh`, revisão independente da branch (modelo mais capaz) e uma rodada
   de correções.
4. Merge **local** na `main` (nunca `git push` sem pedido explícito), regerar o AppImage
   (`packaging/build-appimage.sh`) e atualizar a página de andamento:
   https://claude.ai/artifact/URM9Yaxf5KFppzWzXNpioc

## Regras gerais do dono (valem aqui também)

- Conversar sempre em português do Brasil.
- Navegador: usar o Google Chrome instalado; nunca mudar o navegador padrão do sistema.
- Instalar pacotes com `pkexec <comando>` direto (nunca `sudo`, nunca pedir senha no chat); conferir antes
  com `dnf list --available` e usar `-y` só nos pacotes pedidos.
- A suíte completa de testes só quando o risco justificar; nas mudanças pequenas, lint e testes
  relacionados.
- Nunca imprimir segredos nem valores de `.env`; nunca commitar segredos.
- Commits terminam com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Metas de desempenho

Abertura < 300 ms (binário), memória < 150 MB (também com 3 abas), binário < 25 MB.
