Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "WinVer.nsh"

!define WEBVIEW2_CLIENT "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\NothingXDesktop"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

Name "WinX"
Caption "WinX ${APP_VERSION} Setup"
VIProductVersion "${APP_FILE_VERSION}"
VIAddVersionKey /LANG=1033 "ProductName" "WinX"
VIAddVersionKey /LANG=1033 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1033 "FileDescription" "WinX installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${APP_FILE_VERSION}"
VIAddVersionKey /LANG=1033 "CompanyName" "Threalcocoboy"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright (c) 2026 Nipun Yatawara and contributors"
OutFile "${OUTPUT_EXE}"
InstallDir "$LOCALAPPDATA\Programs\WinX"
InstallDirRegKey HKCU "Software\NothingXDesktop" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "..\assets\winx.ico"
!define MUI_UNICON "..\assets\winx.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "Install WinX for your Windows account.$\r$\n$\r$\nIf Microsoft Edge WebView2 is missing, setup installs it from Microsoft's bundled bootstrapper. This step requires internet access."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "WinX requires 64-bit Windows 10 or Windows 11." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext current
FunctionEnd

Function CheckWebView2
  SetRegView 64
  ReadRegStr $0 HKCU "${WEBVIEW2_CLIENT}" "pv"
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ; The machine-wide runtime is typically registered in the 32-bit registry view.
    SetRegView 32
    ReadRegStr $0 HKCU "${WEBVIEW2_CLIENT}" "pv"
    ${If} $0 == ""
    ${OrIf} $0 == "0.0.0.0"
      ReadRegStr $0 HKLM "${WEBVIEW2_CLIENT}" "pv"
    ${EndIf}
    SetRegView 64
    ${If} $0 == ""
    ${OrIf} $0 == "0.0.0.0"
      ReadRegStr $0 HKLM "${WEBVIEW2_CLIENT}" "pv"
    ${EndIf}
  ${EndIf}
  SetRegView 64
FunctionEnd

Function CheckAppClosed
  ${If} ${FileExists} "$INSTDIR\nothingx-desktop.exe"
    retry_close:
    ClearErrors
    FileOpen $0 "$INSTDIR\nothingx-desktop.exe" a
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "Quit WinX using its tray menu or power button, then choose Retry." /SD IDCANCEL IDRETRY retry_close
      SetErrorLevel 1
      Abort
    ${EndIf}
    FileClose $0
  ${EndIf}
FunctionEnd

Section "WinX (required)" Main
  SectionIn RO
  Call CheckAppClosed
  Call CheckWebView2
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File /oname=MicrosoftEdgeWebView2Setup.exe "${WEBVIEW2_BOOTSTRAPPER}"
    DetailPrint "Installing Microsoft Edge WebView2 Runtime..."
    ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebView2Setup.exe" /silent /install' $1
    Call CheckWebView2
    ${If} $0 == ""
    ${OrIf} $0 == "0.0.0.0"
      MessageBox MB_OK|MB_ICONSTOP "WebView2 Runtime could not be installed (exit code $1). Install Microsoft's Evergreen WebView2 Runtime, then run setup again." /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
  SetOutPath "$INSTDIR"
  File "${APP_DIR}\nothingx-desktop.exe"
  File "${APP_DIR}\WebView2Loader.dll"
  File "${APP_DIR}\README.md"
  File "${APP_DIR}\LICENSE"
  File "${APP_DIR}\THIRD_PARTY.md"
  File "${APP_DIR}\DEPENDENCY_LICENSES.txt"
  SetOutPath "$INSTDIR\docs"
  File "${APP_DIR}\docs\WINX-TALK-MODE.md"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\NothingXDesktop" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "WinX"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Threalcocoboy"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/Threalcocoboy/WinX"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\nothingx-desktop.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Start menu shortcut"
  CreateShortcut "$SMPROGRAMS\WinX.lnk" "$INSTDIR\nothingx-desktop.exe"
SectionEnd

Section /o "Desktop shortcut"
  CreateShortcut "$DESKTOP\WinX.lnk" "$INSTDIR\nothingx-desktop.exe"
SectionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext current
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  ; Remove only this install's files. Keep local preferences and shared WebView2.
  ${If} ${FileExists} "$INSTDIR\nothingx-desktop.exe"
    un_retry_close:
    ClearErrors
    FileOpen $0 "$INSTDIR\nothingx-desktop.exe" a
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONINFORMATION "Quit WinX using its tray menu or power button, then choose Retry." /SD IDCANCEL IDRETRY un_retry_close
      SetErrorLevel 1
      Abort
    ${EndIf}
    FileClose $0
  ${EndIf}
  RMDir /r "$INSTDIR\nothingx-desktop.exe.WebView2"
  Delete "$INSTDIR\nothingx-desktop.exe"
  Delete "$INSTDIR\WebView2Loader.dll"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY.md"
  Delete "$INSTDIR\DEPENDENCY_LICENSES.txt"
  Delete "$INSTDIR\docs\WINX-TALK-MODE.md"
  RMDir "$INSTDIR\docs"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$DESKTOP\WinX.lnk"
  Delete "$SMPROGRAMS\WinX.lnk"
  ReadRegStr $0 HKCU "${RUN_KEY}" "WinX"
  ${If} $0 == '$\"$INSTDIR\nothingx-desktop.exe$\" --startup'
    DeleteRegValue HKCU "${RUN_KEY}" "WinX"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\NothingXDesktop"
SectionEnd
