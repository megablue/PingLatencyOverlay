Unicode true
ManifestDPIAware true
ManifestDPIAwareness PerMonitorV2

!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef APP_EXE
  !error "APP_EXE must be supplied with /DAPP_EXE=<absolute path>"
!endif
!ifndef APP_ICON
  !error "APP_ICON must be supplied with /DAPP_ICON=<absolute path>"
!endif
!ifndef OUT_FILE
  !error "OUT_FILE must be supplied with /DOUT_FILE=<absolute path>"
!endif
!ifndef APP_VERSION
  !error "APP_VERSION must be supplied with /DAPP_VERSION=<version>"
!endif
!ifndef APP_VERSIONWITHBUILD
  !error "APP_VERSIONWITHBUILD must be supplied with /DAPP_VERSIONWITHBUILD=<version>"
!endif
!ifndef APP_LICENSE
  !error "APP_LICENSE must be supplied with /DAPP_LICENSE=<absolute path>"
!endif
!ifndef APP_RENDERER_EXE
  !error "APP_RENDERER_EXE must be supplied with /DAPP_RENDERER_EXE=<absolute path>"
!endif

!define PRODUCTNAME "PingLatencyOverlay"
!define PUBLISHER "megablue"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"

Name "${PRODUCTNAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${APP_VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME} — live network latency overlay"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 megablue"
VIAddVersionKey "Comments" "GPL-3.0-only; source: https://github.com/megablue/PingLatencyOverlay"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"

!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApplication
!define MUI_FINISHPAGE_RUN_TEXT "Launch PingLatencyOverlay"

; MUI2 has exactly two checkboxes on the finish page and no way to add a
; third, so the "show readme" slot is reused as the desktop shortcut option.
; MUI_FINISHPAGE_SHOWREADME_FUNCTION makes it call our function instead of
; opening a file, and MUI_FINISHPAGE_SHOWREADME_TEXT replaces the "Show README"
; caption. The name is misleading, hence this note. Do not try to reach the
; finish page with MUI_PAGE_CUSTOMFUNCTION_SHOW instead: that define is not
; page-scoped and Pages.nsh undefines it after the first page that uses it, so
; it would fire on the welcome page and never reach here.
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${APP_EXE}"
  ; The renderer is a second executable, not a plugin: the shell starts it and
  ; talks to it over a named pipe. It MUST be installed into the same directory
  ; as the shell, because that is how the shell finds it - it looks for a
  ; sibling of its own executable. Installing it anywhere else produces an app
  ; that starts cleanly and shows no overlays, with nothing at runtime to say
  ; why. Only the shell gets a Start Menu shortcut, because only the shell is
  ; what a user launches.
  File "${APP_RENDERER_EXE}"
  File /oname=LICENSE "${APP_LICENSE}"

  CreateDirectory "$SMPROGRAMS"
  CreateShortCut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\ping-latency-overlay.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\ping-latency-overlay-renderer.exe"
  Delete "$INSTDIR\ping-latency-overlay.exe"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd

Function LaunchApplication
  Exec '"$INSTDIR\ping-latency-overlay.exe"'
FunctionEnd

; Called by MUI from the finish page when the shortcut checkbox is ticked.
Function CreateDesktopShortcut
  CreateShortCut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\ping-latency-overlay.exe"
FunctionEnd

; A silent install never shows the finish page, so the checkbox is never read
; and the function above never runs. Create the shortcut anyway, matching the
; box being ticked by default. .onInstSuccess fires once the sections are done,
; which is before the finish page, so the ${Silent} guard keeps this out of the
; way of a GUI install where the user still has the checkbox to answer.
Function .onInstSuccess
  ${If} ${Silent}
    Call CreateDesktopShortcut
  ${EndIf}
FunctionEnd
