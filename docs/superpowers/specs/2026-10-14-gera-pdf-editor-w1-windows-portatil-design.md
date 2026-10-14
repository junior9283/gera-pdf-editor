# w1: versão Windows portátil

Data: 2026-10-14 · Pedido do dono: "a versão de Windows do app, portátil", testada no Windows 11
que roda em container neste PC (acesso por SSH). Versão alvo 0.23.0. Entra no roteiro antes da v6f.

## Objetivo

- Um **ZIP portátil** para Windows 10/11 x64. Basta extrair e abrir o `GeraPDFEditor.exe`. Não
  há instalador, não há DLL de sistema a instalar, e nada vai para o registro.
- **As mesmas funcionalidades da versão Linux**, com o comportamento esperado no Windows:
  - impressão pelas impressoras do Windows;
  - seletores de arquivo nativos;
  - área de transferência;
  - OCR em português e inglês;
  - ícone no executável;
  - sem janela de console.
- **Publicação**: o mesmo workflow Release que gera o AppImage gera o ZIP e anexa os dois à
  release do GitHub.

## Comportamento de referência

- **Portátil** segue a convenção dos apps portáteis (PortableApps): as configurações ficam numa
  pasta `dados\` ao lado do executável e acompanham o pendrive. O Acrobat não tem versão
  portátil; aqui vale a convenção.
- **Impressão no Windows** imita o Acrobat: o diálogo do próprio app (o mesmo da versão Linux)
  lista as impressoras do Windows, com a padrão já marcada. Cópias, frente e verso, orientação,
  páginas por folha, ajustar ao papel, ordem inversa e tons de cinza valem como no Linux.

## Decisões (Rulings)

1. **Compilação cruzada no Linux com MinGW-w64** (`x86_64-pc-windows-gnu`).
   - O `mupdf-sys` compila esse alvo pelo Make. A sondagem compilou MuPDF e Tesseract em 3,5 min.
   - A alternativa MSVC exige `msbuild` no Windows: a VM tem 4 GB e o runner Windows do GitHub
     não deixa testar em branch sem push.
   - libgcc, libstdc++ e winpthread são ligados estaticamente: o `.exe` não pede DLL.
2. **Interface no Windows com `wgpu`** (DirectX 12, com o WARP por software quando não há placa
   de vídeo). O `glow`, que é OpenGL, não abre sem driver de vídeo, como na VM ou num PC recém
   formatado. No Linux continua o `glow`.
   - Custo: binário maior no Windows. A meta é a mesma, < 50 MB.
3. **Pasta de dados**:
   - `dados\` ao lado do `.exe`, com `config\` e `cache\`;
   - se a pasta do `.exe` não for gravável (Arquivos de Programas, mídia somente leitura), vale
     `%APPDATA%\Gera PDF Editor`.
   - `XDG_CONFIG_HOME` e `XDG_CACHE_HOME`, quando definidos, continuam mandando (testes).
4. **Impressão no Windows por GDI**, sem programa externo:
   - lista e padrão vêm de `EnumPrintersW` e `GetDefaultPrinterW`;
   - cada folha é renderizada pelo MuPDF na resolução da impressora (no máximo 600 ppp) e
     desenhada com `StretchDIBits`;
   - cópias, frente e verso e orientação vão no `DEVMODE`;
   - páginas por folha, ajuste e ordem saem de uma montagem de folhas pura, em app-state, com
     testes, usada só no Windows (no Linux, o CUPS faz isso).
4a. **Impressão pela janela do sistema** (pedido do dono em 14/10, durante a etapa: "mude a
   impressão para que seja pela janela do sistema operacional, tanto para Windows quanto para
   Linux"). Arquivo ▸ Imprimir não abre mais o diálogo do app:
   - **Windows**: a janela padrão "Imprimir" (`PrintDlgEx`), já com a orientação da primeira
     página. A impressora, as páginas (todas, a atual ou intervalos), as cópias, o agrupamento e
     as Preferências do driver (frente e verso, cor, páginas por folha do driver) vêm dela. O
     app desenha cada página ajustada à área imprimível (decisão 4), com o DEVMODE escolhido.
   - **Linux**: a janela do portal de impressão (`org.freedesktop.portal.Print`, a mesma do KDE
     e do GNOME). Ela escolhe a impressora e as opções, e o app manda o PDF (com as marcações)
     para o sistema imprimir.
   - **Reserva**: sem o portal (ambiente mínimo, sem D-Bus), abre o diálogo do app com o CUPS,
     como antes.
   - As opções que só o diálogo do app tinha (páginas por folha do app, ordem inversa, só
     documento sem marcações) passam a ser as do sistema ou do driver. Custo: no Windows sem
     driver com "páginas por folha", essa opção não existe.
5. **Espaço livre em disco** por `GetDiskFreeSpaceExW`.
6. **Integração com a área de trabalho** do Linux (`.desktop`, ícones em `~/.local/share`) não
   existe no Windows. O app portátil não associa `.pdf` sozinho: o usuário abre por "Abrir com"
   ou arrastando o arquivo, e o caminho chega como argumento.
7. **Dados do OCR**: `tessdata\por.traineddata` e `eng.traineddata` ficam no ZIP, ao lado do
   `.exe`, e são procurados primeiro ali.
8. **Caminhos**: o MuPDF no Windows recebe o caminho em UTF-8, convertido para wide internamente.
   Caminhos com acentos funcionam. Um caminho que não é Unicode válido (raro) é recusado com
   mensagem.
9. **Nome do pacote**: `Gera_PDF_Editor-<versão>-windows-x64-portatil.zip`, com a pasta
   `Gera PDF Editor\` dentro.

## Arquitetura

- **`platform` (gera-ui e app-state)**: os pontos que mudam por sistema ficam atrás de `cfg`:
  - pastas de dados (`store.rs`);
  - espaço em disco (`create_export.rs`);
  - impressão (`print_job.rs`, CUPS no Linux; `print_win.rs`, GDI no Windows);
  - área de trabalho (`desktop.rs`, só no Linux);
  - permissões de arquivo (já em `cfg(unix)`).
- **`pdf-core`**: `mu_path(&Path) -> Result<&str>` para abrir e gravar pelo MuPDF nos dois
  sistemas; `tessdata_dir` procura ao lado do executável.
- **app-state `print_layout.rs`**: folhas a partir das opções (páginas, ordem, páginas por folha,
  ajuste, orientação), em coordenadas do papel. É puro e testado.
- **Empacotamento**: `packaging/build-windows.sh` compila, junta `.exe`, tessdata, licenças e
  avisos, e gera o ZIP. O ícone e as informações da versão vêm do `winresource` no `build.rs`
  (usa o `windres` do MinGW).
- **CI**: `release.yml` ganha o job `windows` (ubuntu-22.04 com `mingw-w64`, variante posix) que
  anexa o ZIP. `ci.yml` ganha um `cargo build --target x86_64-pc-windows-gnu` para que a versão
  Windows não quebre sem ninguém ver.

## Testes

- **Unitários (Linux)**:
  - montagem de folhas (1, 2, 4, 6, 9 por folha; retrato e paisagem; ajustar, reduzir e tamanho
    real; ordem inversa);
  - escolha da pasta de dados portátil.
- **Compilação**: `cargo build --target x86_64-pc-windows-gnu` e clippy nesse alvo.
- **E2E no Windows 11 do container**, por SSH, com o app rodando na sessão interativa do usuário
  (tarefa agendada) e capturas de tela por PowerShell:
  - abre sem console, com o `wgpu` funcionando sem placa de vídeo;
  - abrir PDF com acento no caminho;
  - renderização e texto;
  - salvar e reabrir;
  - preencher formulário;
  - OCR (português);
  - imprimir na impressora "Microsoft Print to PDF" para um arquivo e conferir o PDF gerado
    (número de páginas e conteúdo);
  - a pasta `dados\` criada ao lado do `.exe`, sem nada em `%APPDATA%`.
- **Medidas no Windows**: tamanho do ZIP e do `.exe`, tempo de abertura e memória.

## Decisões tomadas na execução

1. **libstdc++ estática.** O `mupdf-sys` pede a `stdc++` como biblioteca dinâmica, e o rustc mantém o
   modo dinâmico mesmo com `-static`. O `build.rs` do pdf-core põe uma pasta com só a
   `libstdc++.a` antes das do compilador. Vale para o app e para os testes: nenhum `.exe` depende de
   DLL do MinGW. O `.cargo/config.toml` liga o resto com `+crt-static`.
2. **Abrir pela memória no Windows.** O Windows não deixa substituir um arquivo aberto, e o MuPDF o
   mantém aberto. No Windows, o PDF é lido para a memória ao abrir (pico de 2× o tamanho do arquivo
   durante a abertura), e o Salvar grava por cima. O `sync` abre o temporário para escrita, porque
   o flush do Windows exige isso. No Linux o arquivo continua aberto pelo MuPDF, e nomes que não
   são UTF-8 abrem como bytes.
3. **Janela maximizada no Windows**, como o Acrobat, sem tamanho inicial (os dois juntos deixavam a
   janela "maximizada" com 1100 px).
4. **Caminho canônico** pelo `dunce`, que tira o `\\?\` só quando é seguro.
5. **Pasta portátil** resolvida uma vez por execução (o teste de gravação não se repete).
6. **Impressão pela janela do sistema** (decisão 4a), conforme a execução:
   - **Windows** (`PrintDlgEx`):
     - a janela do app é a dona;
     - DEVMODE inicial com a orientação da primeira página;
     - páginas das miniaturas pré-selecionadas;
     - cópias e agrupar feitos pelo app (com o DEVMODE em 1 cópia), porque a flag que deixa isso
       com o driver desabilita as cópias nos drivers que não as fazem.
   - **Desenho no Windows**:
     - cada página tem a sua lista de desenho e é rasterizada em faixas de 512 linhas, em BGRA
       direto num DIB de 32 bits, até 600 ppp;
     - a página deitada na folha em pé (ou o contrário) gira 90° no sentido anti-horário
       (retrato/paisagem automático do Acrobat).
   - **Linux** (portal):
     - `page-ranges` contadas a partir de 0, como o GtkPrintSettings;
     - espera de até 10 minutos pela resposta;
     - nos testes E2E, o D-Bus aponta para um endereço inexistente, para não abrir a janela do KDE
       na tela real.
7. **Sem o módulo `print_layout`**: com a janela do sistema, o app só ajusta e centraliza uma página
   por folha; páginas por folha e ordem são do driver.
8. **Desempenho no Windows fora das metas** (pedido do dono em 14/10: "não se preocupe com as
   métricas de desempenho no Windows"). Na VM sem placa de vídeo: abertura de 0,7 a 0,9 s e working
   set de 93 MB; o teste de tempo `hostile_pages_finish_quickly` leva 4,0 s, acima do limite de 3 s.
   As metas valem para o Linux.
9. **Ambiente de teste**:
   - a VM não tem "Microsoft Print to PDF" (a ativação pede reinicialização);
   - a "Adobe PDF" está com a licença expirada;
   - os testes de impressão usam a impressora "Gera Teste" (driver IPP de classe, porta `FILE:`),
     criada só para isso.
