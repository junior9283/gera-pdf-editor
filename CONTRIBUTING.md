# Como contribuir

Obrigado pelo interesse! Toda ajuda conta: relatar bugs, testar com PDFs reais, sugerir melhorias,
traduzir e escrever código.

## Relatar um bug

Abra uma [issue](https://github.com/junior9283/gera-pdf-editor/issues/new/choose) com:
- versão (Ajuda → Sobre) e distribuição Linux;
- passos para reproduzir, o que esperava e o que aconteceu;
- se possível, um PDF de exemplo **sem dados pessoais**.

Falhas de segurança não vão para issues públicas: veja [SECURITY.md](SECURITY.md).

## Preparar o ambiente

Veja **Compilar a partir do código** no [README](README.md#compilar-a-partir-do-código). Depois:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## Como o código é organizado

- `crates/pdf-core`: tudo o que mexe no PDF (MuPDF). Tem testes de integração em `tests/`.
- `crates/app-state`: estado e regras da interface, sem UI.
- `crates/gera-ui`: interface egui/eframe.
- `docs/superpowers/specs` e `docs/superpowers/plans`: especificação e plano de cada etapa.

## Regras do projeto

- **Comportamento igual ao do Acrobat Pro.** Antes de inventar um atalho ou fluxo, confira como o
  Acrobat faz. A implementação deve ser **original**: não copie código, ícones nem textos da Adobe.
- **Testes primeiro.** Toda correção de bug vem com um teste que falha sem a correção; toda
  funcionalidade nova vem com testes em `pdf-core` (e na UI, quando houver lógica).
- **Desempenho.** Abertura < 300 ms, memória < 150 MB (também com 3 abas) e binário < 50 MB:
  rode `scripts/measure.sh` se a mudança puder afetar isso.
- **Interface e comentários em português do Brasil.**
- Dependências novas precisam ter licença compatível com a **AGPL-3.0** e ser listadas em
  `THIRD_PARTY_NOTICES.md` quando entrarem no binário.

## Pull requests

1. Crie um branch a partir da `main` (`feat/…`, `fix/…`).
2. Commits pequenos e descritivos (ex.: `fix: …`, `feat: …`, `docs: …`).
3. Garanta que `cargo test`, `cargo clippy` e `cargo fmt --check` passam.
   Os testes de interface (Xvfb) usam `cargo build --release --features gera-ui/e2e`: só nesse
   build a variável `GERA_E2E_PICK` substitui os seletores de arquivo.
4. Descreva o que mudou e como testou; inclua capturas de tela para mudanças visuais.

Ao contribuir, você concorda que sua contribuição seja licenciada sob a [AGPL-3.0](LICENSE).
Siga o [código de conduta](CODE_OF_CONDUCT.md).
