# w1: versão Windows portátil. Plano

> **Para agentes:** sub-skill obrigatória: superpowers:executing-plans (execução nativa).

**Spec:** `docs/superpowers/specs/2026-10-14-gera-pdf-editor-w1-windows-portatil-design.md`

## Restrições globais

- O Linux não muda: o mesmo binário, as mesmas medidas, os testes passando.
- O código específico de cada sistema fica atrás de `cfg(windows)` ou `cfg(unix)`, em poucos
  arquivos.
- Comandos:
  - build Windows: `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --release --target x86_64-pc-windows-gnu`;
  - testes no Linux: `cargo test -q -p app-state -p gera-ui`, `cargo test -q -p pdf-core -- --test-threads=4` e clippy;
  - clippy também no alvo Windows.
- **Windows de teste**: `ssh -i ~/.ssh/assinador_windows -o UserKnownHostsFile=~/.ssh/known_hosts_assinador_windows geraldo@172.21.0.2`
  (PowerShell). O app roda na sessão interativa por tarefa agendada.

## Foco da revisão

1. Nada no Linux mudou de comportamento.
2. A impressão no Windows respeita páginas, cópias, frente e verso, orientação, páginas por folha
   e ajuste, e não vaza GDI nem memória com documentos grandes.
3. A pasta de dados portátil nunca grava fora de `dados\`, salvo quando não há permissão.
4. Caminhos com acentos e espaços funcionam nos dois sistemas.

---

### Tarefa 1: compilar no alvo Windows

- `mu_path` no pdf-core e espaço livre por sistema.
- `print_job` e `desktop` só em `cfg(unix)`, com uma fachada `printing` no Windows.
- `wgpu` no Windows e `glow` no Linux; `windows_subsystem`.
- [ ] Teste RED: `mu_path_accepts_unicode`.
- [ ] Build Windows sem erro, clippy nos dois alvos, commit.

### Tarefa 2: pasta de dados portátil

- `store::data_root()`: `dados\` ao lado do `.exe` se gravável; senão `%APPDATA%`. No Linux, o
  XDG de sempre.
- [ ] Testes RED: `portable_dir_next_to_exe`, `falls_back_when_read_only`.
- [ ] Implementar, GREEN, commit.

### Tarefa 3: montagem de folhas (app-state print_layout.rs)

- [ ] Testes RED:
  - `one_per_sheet_fit_shrink_actual`;
  - `n_up_grid_and_orientation`;
  - `reverse_and_ranges`.
- [ ] Implementar, GREEN, commit.

### Tarefa 4: impressão no Windows (GDI)

- Impressoras e padrão; um job numa thread que abre o PDF temporário, monta as folhas,
  renderiza e envia; `DEVMODE` com cópias, frente e verso e orientação.
- [ ] Build Windows; E2E com "Microsoft Print to PDF" para um arquivo.
- [ ] Commit.

### Tarefa 5: OCR, ícone e pacote

- `tessdata_dir` ao lado do `.exe`; `winresource` (ícone e versão);
  `packaging/build-windows.sh` gera o ZIP.
- [ ] Teste RED: `tessdata_next_to_exe_first`.
- [ ] Gerar o ZIP, commit.

### Tarefa 6: CI e release

- `release.yml` com o job windows; `ci.yml` com o build Windows.
- [ ] Commit.

### Fim da etapa

- [ ] Bateria E2E no Windows 11 (lista da spec), medidas no Windows e `measure.sh` no Linux.
- [ ] `simplify`, revisão Opus e uma rodada de correções.
- [ ] Merge; docs 0.23.0 (README com a seção Windows); tag e push; conferir os anexos do
      workflow; AppImage; página de andamento; decisões na spec.
