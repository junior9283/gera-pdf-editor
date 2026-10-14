; Instalador do Gera PDF Editor para Windows 10/11 (64 bits), gerado pelo makensis no Linux:
;   makensis -DVERSION=0.24.0 -DSOURCE=<pasta do app> -DOUTFILE=<instalador.exe> instalador.nsi
; Instala em Arquivos de Programas (com administrador), cria atalhos, registra o app para PDF
; ("Abrir com" e Aplicativos padrão) e entra em "Aplicativos instalados". Desinstalar mantém as
; configurações do usuário (%APPDATA%\Gera PDF Editor).
Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 64

!ifndef VERSION | SOURCE | OUTFILE
  !error "use -DVERSION=... -DSOURCE=<pasta do app> -DOUTFILE=<instalador.exe>"
!endif
; o makensis compila de dentro da pasta deste script: a raiz do repositório fica duas acima
!define ROOT "../.."

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define APP "Gera PDF Editor"
!define EXE "GeraPDFEditor.exe"
!define PROGID "GeraPDFEditor.PDF"
!define APPKEY "Software\${APP}"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GeraPDFEditor"
!define URL "https://github.com/junior9283/gera-pdf-editor"
!define EXEPATH "$INSTDIR\${EXE}"
!define OPENCMD '"$INSTDIR\${EXE}" "%1"'
!define UNINST "$INSTDIR\Desinstalar.exe"
!define APPPATHS "Software\Microsoft\Windows\CurrentVersion\App Paths\${EXE}"
; o mesmo nome de app-state/src/store.rs (INSTALLED_MARKER): com ele, as configurações vão para %APPDATA%
!define MARKER "instalado"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APP}"
RequestExecutionLevel admin
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1046 "ProductName" "${APP}"
VIAddVersionKey /LANG=1046 "FileDescription" "Instalador do ${APP}"
VIAddVersionKey /LANG=1046 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1046 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1046 "LegalCopyright" "AGPL-3.0"

!define MUI_ICON "${ROOT}/packaging/icons/gera-pdf-editor.ico"
!define MUI_UNICON "${ROOT}/packaging/icons/gera-pdf-editor.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEFINISHPAGE_BITMAP "${ROOT}/packaging/windows/assistente.bmp"
!define MUI_COMPONENTSPAGE_SMALLDESC
; o app abre sem a elevação do instalador (pelo Explorer): arrastar arquivos para a janela
; continua funcionando
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Abrir o ${APP}"
!define MUI_FINISHPAGE_RUN_FUNCTION OpenApp
; o Windows não deixa um programa virar o padrão sozinho: o usuário escolhe em Aplicativos padrão
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Usar para abrir PDF (Aplicativos padrão)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION DefaultApps

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${ROOT}/LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "PortugueseBR"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "O ${APP} precisa do Windows de 64 bits."
    Abort
  ${EndIf}
  ; instalação por máquina: só com administrador (com o UAC desligado, uma conta comum chegaria
  ; aqui sem poder gravar em Arquivos de Programas)
  UserInfo::GetAccountType
  Pop $0
  ${If} $0 != "Admin"
    MessageBox MB_ICONSTOP "Para instalar o ${APP} é preciso entrar como administrador." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  ; atualização: a pasta da instalação anterior (lida na visão de 64 bits do registro)
  ReadRegStr $0 HKLM "${APPKEY}" "InstallDir"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
  ; instalação por máquina: atalhos no menu Iniciar e na área de trabalho de todos os usuários
  SetShellVarContext all
FunctionEnd

; o programa aberto não pode ser trocado nem apagado: pede para fechar (Repetir) ou cancela
!macro WaitClosed UN
Function ${UN}WaitClosed
  retry:
  IfFileExists "${EXEPATH}" 0 done
  ClearErrors
  FileOpen $0 "${EXEPATH}" a
  IfErrors busy
  FileClose $0
  Goto done
  busy:
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Feche o ${APP} e clique em Repetir." /SD IDCANCEL IDRETRY retry
  Abort
  done:
FunctionEnd
!macroend
!insertmacro WaitClosed ""
!insertmacro WaitClosed "un."

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd

Function OpenApp
  Exec '"$WINDIR\explorer.exe" "${EXEPATH}"'
FunctionEnd

Function DefaultApps
  ExecShell "open" "ms-settings:defaultapps?registeredAppMachine=${APP}"
FunctionEnd

Section "${APP} (obrigatório)" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  Call WaitClosed
  ; o que build-windows.sh monta (o desinstalador remove a mesma lista)
  File /r "${SOURCE}\*.*"
  FileOpen $0 "$INSTDIR\${MARKER}" w
  FileClose $0
  WriteUninstaller "${UNINST}"
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "${EXEPATH}"

  WriteRegStr HKLM "${APPKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${APPPATHS}" "" "${EXEPATH}"

  ; PDF: ProgID, "Abrir com" e Aplicativos padrão (o modelo recomendado pela Microsoft)
  WriteRegStr HKLM "Software\Classes\${PROGID}" "" "Documento PDF"
  WriteRegStr HKLM "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKLM "Software\Classes\${PROGID}\shell\open\command" "" '${OPENCMD}'
  WriteRegStr HKLM "Software\Classes\.pdf\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}" "FriendlyAppName" "${APP}"
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}\SupportedTypes" ".pdf" ""
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}\shell\open\command" "" '${OPENCMD}'
  WriteRegStr HKLM "${APPKEY}\Capabilities" "ApplicationName" "${APP}"
  WriteRegStr HKLM "${APPKEY}\Capabilities" "ApplicationDescription" "Editor de PDF livre e de código aberto"
  WriteRegStr HKLM "${APPKEY}\Capabilities\FileAssociations" ".pdf" "${PROGID}"
  WriteRegStr HKLM "Software\RegisteredApplications" "${APP}" "${APPKEY}\Capabilities"

  ; "Aplicativos instalados"
  WriteRegStr HKLM "${UNKEY}" "DisplayName" "${APP}"
  WriteRegStr HKLM "${UNKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNKEY}" "Publisher" "${APP}"
  WriteRegStr HKLM "${UNKEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKLM "${UNKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNKEY}" "URLInfoAbout" "${URL}"
  WriteRegStr HKLM "${UNKEY}" "UninstallString" '"${UNINST}"'
  WriteRegStr HKLM "${UNKEY}" "QuietUninstallString" '"${UNINST}" /S'
  WriteRegDWORD HKLM "${UNKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNKEY}" "NoRepair" 1
  ; o tamanho do que este instalador traz (sem varrer a pasta, que pode ter outros arquivos)
  SectionGetSize ${SecApp} $0
  WriteRegDWORD HKLM "${UNKEY}" "EstimatedSize" $0

  ; o Explorer relê as associações (ícone dos PDF)
  ${RefreshShellIcons}
SectionEnd

Section "Atalho na área de trabalho" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "${EXEPATH}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "O programa, os dados do OCR e o registro para abrir PDF."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Um atalho na área de trabalho."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Call un.WaitClosed
  ; só os arquivos que o instalador pôs (a pasta montada por build-windows.sh), nunca pastas
  ; inteiras: o usuário pode ter escolhido uma pasta com outros arquivos
  Delete "${EXEPATH}"
  Delete "$INSTDIR\${MARKER}"
  Delete "${UNINST}"
  Delete "$INSTDIR\tessdata\por.traineddata"
  Delete "$INSTDIR\tessdata\eng.traineddata"
  RMDir "$INSTDIR\tessdata"
  Delete "$INSTDIR\licencas\LICENSE.txt"
  Delete "$INSTDIR\licencas\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\licencas\OFL-GreatVibes.txt"
  RMDir "$INSTDIR\licencas"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"

  DeleteRegKey HKLM "Software\Classes\${PROGID}"
  DeleteRegValue HKLM "Software\Classes\.pdf\OpenWithProgids" "${PROGID}"
  DeleteRegKey HKLM "Software\Classes\Applications\${EXE}"
  DeleteRegValue HKLM "Software\RegisteredApplications" "${APP}"
  DeleteRegKey HKLM "${APPKEY}"
  DeleteRegKey HKLM "${APPPATHS}"
  DeleteRegKey HKLM "${UNKEY}"
  ${un.RefreshShellIcons}
  ; as configurações do usuário (%APPDATA%\Gera PDF Editor) ficam, como no Acrobat
SectionEnd
