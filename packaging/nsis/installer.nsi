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

!define PRODUCTNAME "PingLatencyOverlay"
!define PUBLISHER "PingLatencyOverlay"
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
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"

!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_ABORTWARNING
!define MUI_COMPONENTS_PAGE
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApplication
!define MUI_FINISHPAGE_RUN_TEXT "Launch PingLatencyOverlay"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${APP_EXE}"

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

Section "Desktop shortcut" SEC_DESKTOP_SHORTCUT
  CreateShortCut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\ping-latency-overlay.exe"
SectionEnd

Section "Uninstall"
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
