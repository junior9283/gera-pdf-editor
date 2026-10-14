; Instalador do Gera PDF Editor para Windows 10/11 (64 bits), gerado pelo makensis no Linux:
;   makensis -DVERSION=0.24.0 -DSOURCE=<pasta do app> -DOUTFILE=<instalador.exe> -DROOT=<repositório> instalador.nsi
; Instala em Arquivos de Programas (com administrador), cria atalhos, registra o app para PDF
; ("Abrir com" e Aplicativos padrão) e entra em "Aplicativos instalados". Desinstalar mantém as
; configurações do usuário (%APPDATA%\Gera PDF Editor).
Unicode true
SetCompressor /SOLID lzma

!ifndef VERSION
  !error "use -DVERSION=... -DSOURCE=... -DOUTFILE=... -DROOT=..."
!endif
!ifndef SOURCE
  !error "falta -DSOURCE (a pasta do app montada pelo build-windows.sh)"
!endif
!ifndef OUTFILE
  !error "falta -DOUTFILE"
!endif
!ifndef ROOT
  !error "falta -DROOT (a raiz do repositório)"
!endif

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

!define APP "Gera PDF Editor"
!define EXE "GeraPDFEditor.exe"
!define PROGID "GeraPDFEditor.PDF"
!define APPKEY "Software\${APP}"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GeraPDFEditor"
!define URL "https://github.com/junior9283/gera-pdf-editor"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APP}"
InstallDirRegKey HKLM "${APPKEY}" "InstallDir"
RequestExecutionLevel admin
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1046 "ProductName" "${APP}"
VIAddVersionKey /LANG=1046 "FileDescription" "Instalador do ${APP}"
VIAddVersionKey /LANG=1046 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1046 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1046 "LegalCopyright" "AGPL-3.0"

!define MUI_ICON "${ROOT}\packaging\icons\gera-pdf-editor.ico"
!define MUI_UNICON "${ROOT}\packaging\icons\gera-pdf-editor.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEFINISHPAGE_BITMAP "${ROOT}\packaging\windows\assistente.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "${ROOT}\packaging\windows\assistente.bmp"
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
!insertmacro MUI_PAGE_LICENSE "${ROOT}\LICENSE"
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
  SetRegView 64
  ; instalação por máquina: atalhos no menu Iniciar e na área de trabalho de todos os usuários
  SetShellVarContext all
FunctionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd

Function OpenApp
  Exec '"$WINDIR\explorer.exe" "$INSTDIR\${EXE}"'
FunctionEnd

Function DefaultApps
  ExecShell "open" "ms-settings:defaultapps?registeredAppMachine=${APP}"
FunctionEnd

Section "${APP} (obrigatório)" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  ; o LEIA-ME é o da versão portátil (pasta "dados"): não serve para a instalada
  File /r /x "LEIA-ME.txt" "${SOURCE}\*.*"
  ; marcador: a cópia instalada guarda as configurações em %APPDATA%
  FileOpen $0 "$INSTDIR\instalado" w
  FileClose $0
  WriteUninstaller "$INSTDIR\Desinstalar.exe"
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKLM "${APPKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${EXE}" "" "$INSTDIR\${EXE}"

  ; PDF: ProgID, "Abrir com" e Aplicativos padrão (o modelo recomendado pela Microsoft)
  WriteRegStr HKLM "Software\Classes\${PROGID}" "" "Documento PDF"
  WriteRegStr HKLM "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKLM "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
  WriteRegStr HKLM "Software\Classes\.pdf\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}" "FriendlyAppName" "${APP}"
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}\SupportedTypes" ".pdf" ""
  WriteRegStr HKLM "Software\Classes\Applications\${EXE}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
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
  WriteRegStr HKLM "${UNKEY}" "UninstallString" '"$INSTDIR\Desinstalar.exe"'
  WriteRegStr HKLM "${UNKEY}" "QuietUninstallString" '"$INSTDIR\Desinstalar.exe" /S'
  WriteRegDWORD HKLM "${UNKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKLM "${UNKEY}" "EstimatedSize" $0

  ; o Explorer relê as associações (ícone dos PDF)
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section "Atalho na área de trabalho" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "O programa, os dados do OCR e o registro para abrir PDF."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Um atalho na área de trabalho."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  ; só o que o instalador pôs (nunca a pasta inteira, caso o usuário tenha escolhido uma pasta comum)
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\instalado"
  Delete "$INSTDIR\Desinstalar.exe"
  RMDir /r "$INSTDIR\tessdata"
  RMDir /r "$INSTDIR\licencas"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"

  DeleteRegKey HKLM "Software\Classes\${PROGID}"
  DeleteRegValue HKLM "Software\Classes\.pdf\OpenWithProgids" "${PROGID}"
  DeleteRegKey HKLM "Software\Classes\Applications\${EXE}"
  DeleteRegValue HKLM "Software\RegisteredApplications" "${APP}"
  DeleteRegKey HKLM "${APPKEY}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${EXE}"
  DeleteRegKey HKLM "${UNKEY}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  ; as configurações do usuário (%APPDATA%\Gera PDF Editor) ficam, como no Acrobat
SectionEnd
