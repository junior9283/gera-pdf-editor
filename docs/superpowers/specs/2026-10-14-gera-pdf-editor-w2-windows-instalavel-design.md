# w2: versão Windows instalável

Data: 2026-10-14 · Pedido do dono: "após terminar a versão portátil, faça a versão instalável". Versão
alvo 0.24.0. Vem depois da w1 (0.23.0) e antes da v6f.

## Objetivo

Um instalador para Windows 10/11 x64, `Gera_PDF_Editor-<versão>-windows-x64-instalador.exe`, ao
lado do ZIP portátil:

- instala em `C:\Program Files\Gera PDF Editor` (com a confirmação de administrador do Windows,
  como o Acrobat);
- cria o atalho no menu Iniciar e, se marcado, na área de trabalho;
- registra o app para PDF:
  - aparece em "Abrir com" e em Configurações ▸ Aplicativos padrão;
  - o Explorer passa a usar o ícone do app;
- aparece em "Aplicativos instalados", com ícone, versão, editor e tamanho, e desinstala por ali;
- uma versão nova instalada por cima atualiza a anterior;
- o desinstalador remove programa, atalhos e registro, e mantém as configurações do usuário (como
  o Acrobat).

## Decisões (Rulings)

1. **NSIS** (zlib/libpng; o instalador gerado não leva restrição de licença), gerado no Linux pelo
   `makensis`, no mesmo CI. Usa a interface Modern UI 2 em português do Brasil, com a página da
   licença AGPL-3.0. O instalador é de 32 bits com Unicode (o padrão do NSIS) e instala o app de
   64 bits em `$PROGRAMFILES64`.
2. **Por máquina, com administrador**, como o Acrobat. O instalador se recusa a rodar num Windows
   de 32 bits.
3. **Configurações do usuário em `%APPDATA%\Gera PDF Editor`**. A pasta do programa tem o marcador
   `instalado`, e com ele o app nunca grava ao lado do `.exe` (nem quando roda como administrador).
   O ZIP portátil não tem o marcador.
4. **Associação com PDF** pelo registro recomendado pela Microsoft:
   - ProgID `GeraPDFEditor.PDF` com ícone e `open`;
   - `.pdf\OpenWithProgids`;
   - `Applications\GeraPDFEditor.exe` com `SupportedTypes`;
   - Capabilities em `RegisteredApplications`.

   O Windows 10/11 não deixa um programa se tornar o padrão sozinho: o usuário escolhe em "Abrir
   com" ▸ "Sempre" ou em Aplicativos padrão. Ao final, o instalador oferece abrir Aplicativos
   padrão.
5. **Sem assinatura de código.** Não há certificado; o SmartScreen avisa "editor desconhecido" na
   primeira vez. Isso fica registrado no README.
6. **Uma janela por arquivo aberto pelo Explorer**, como hoje no Linux. Abrir na janela que já está
   aberta (em abas, como o Acrobat) fica para uma etapa futura.
7. **Desinstalar mantém `%APPDATA%\Gera PDF Editor`**, com as configurações, IDs digitais e
   carimbos do usuário.

## Arquitetura

- `packaging/windows/instalador.nsi`: o script do NSIS (páginas, seções, registro, desinstalador).
- `packaging/build-windows.sh`: monta a pasta do app uma vez e gera o ZIP portátil e o instalador
  (`makensis -DVERSION=… -DSOURCE=…`).
- `app-state/store.rs`: o marcador `instalado` ao lado do `.exe` manda direto para `%APPDATA%`.
- CI: o job `windows` do `release.yml` instala o `nsis` e anexa os dois arquivos.

## Testes

- **Unitário**: `installed_marker_goes_to_appdata` (com o marcador, a pasta base é a reserva, sem
  tentar gravar ao lado do programa).
- **No Windows 11 do container**:
  - instalação silenciosa (`/S`) e pela interface (capturas);
  - arquivos em Program Files;
  - atalho no menu Iniciar;
  - chaves de Desinstalar, ProgID, OpenWithProgids e RegisteredApplications;
  - abrir pelo atalho;
  - abrir um PDF pelo comando de "Abrir com" do registro;
  - configurações em `%APPDATA%`, nada na pasta do programa;
  - atualizar por cima;
  - desinstalar e conferir que não sobra nada além das configurações.

## Decisões tomadas na execução

1. **Atalhos para todos os usuários** (`SetShellVarContext all`), porque a instalação é por
   máquina. Sem isso, o atalho do menu Iniciar ia só para quem instalou.
2. **Abrir o app ao concluir sem elevação**, pelo Explorer. Aberto direto pelo instalador, ele
   herdaria o administrador, e arrastar arquivos do Explorer para a janela pararia de funcionar.
3. **Atualização na mesma pasta.** A pasta da instalação anterior é lida no `.onInit`, depois do
   `SetRegView 64`. O `InstallDirRegKey` lia na visão de 32 bits e nunca achava.
4. **Desinstalar remove só os arquivos que instalou**, nunca pastas inteiras: a pasta escolhida
   pode ter outros arquivos (por exemplo, a do Tesseract).
5. **App aberto.** Instalar e desinstalar pedem para fechar (Repetir/Cancelar). No modo silencioso,
   cancelam com o código 2.
6. **Conta sem administrador é recusada** com aviso. Com o UAC desligado, ela chegaria a instalar
   pela metade.
7. **Imagem própria no assistente** (o ícone e o nome), no lugar da genérica do NSIS. A descrição
   dos componentes fica embaixo, para os nomes não serem cortados.
8. **Dicionário LZMA de 64 MB.** O instalador tem cerca de 14,9 MB e o ZIP portátil 18,8 MB.
9. **Testes no Windows sem trocar o padrão de PDF da VM.** O Gera aparece em "Selecione um aplicativo
   padrão para .pdf", e a abertura foi testada pelo comando registrado. A entrada de teclado e mouse
   no instalador exige um script elevado (UIPI).
