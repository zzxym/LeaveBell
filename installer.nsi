; LeaveBell NSIS Installer
; Variants: online | offline | screen
; Call with /DVARIANT=... /DPKG_DIR=... and /DSCREEN for screen-only payload.

!define APP_NAME "LeaveBell"
!define APP_PUBLISHER "School"
!ifndef APP_VERSION
!define APP_VERSION "2.0.0"
!endif
!ifndef ARCH
!define ARCH "amd64"
!endif
!ifndef VARIANT
!define VARIANT "online"
!endif
!ifndef APP_TITLE
!if "${VARIANT}" == "offline"
!define APP_TITLE "LeaveBell Offline"
!elseif "${VARIANT}" == "screen"
!define APP_TITLE "LeaveBell Screen"
!else
!define APP_TITLE "LeaveBell Online"
!endif
!endif
!ifndef APP_DIR
!define APP_DIR "LeaveBell-${VARIANT}"
!endif
!ifndef REG_KEY
!define REG_KEY "Software\${APP_NAME}\${VARIANT}"
!endif
!ifndef PKG_DIR
!if "${VARIANT}" == "offline"
!define PKG_DIR "dist-offline\win-${ARCH}"
!else
!define PKG_DIR "dist-native\${VARIANT}-${ARCH}"
!endif
!endif
!if "${VARIANT}" == "screen"
!ifndef SCREEN
!define SCREEN
!endif
!endif

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_TITLE}"

Name "${APP_TITLE}"
OutFile "LeaveBell-${VARIANT}-windows-${ARCH}.exe"
InstallDir "D:\${APP_DIR}"
InstallDirRegKey HKCU "${REG_KEY}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUnInstDetails show

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File /r "${PKG_DIR}\*"

  WriteRegStr HKCU "${REG_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${REG_KEY}" "Version" "${APP_VERSION}"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_TITLE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"

  CreateDirectory "$SMPROGRAMS\${APP_TITLE}"
  !ifdef SCREEN
    CreateShortcut "$SMPROGRAMS\${APP_TITLE}\大屏端.lnk" "$INSTDIR\LeaveBellScreen.exe"
    CreateShortcut "$DESKTOP\${APP_TITLE}.lnk" "$INSTDIR\LeaveBellScreen.exe"
  !else
    CreateShortcut "$SMPROGRAMS\${APP_TITLE}\${APP_TITLE}.lnk" "$INSTDIR\LeaveBell.exe"
    CreateShortcut "$SMPROGRAMS\${APP_TITLE}\大屏端.lnk" "$INSTDIR\LeaveBellScreen.exe"
    CreateShortcut "$DESKTOP\${APP_TITLE}.lnk" "$INSTDIR\LeaveBell.exe"
  !endif
  CreateShortcut "$SMPROGRAMS\${APP_TITLE}\卸载.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\uninstall.exe"
  !ifdef SCREEN
    Delete "$INSTDIR\LeaveBellScreen.exe"
  !else
    Delete "$INSTDIR\LeaveBell.exe"
    Delete "$INSTDIR\LeaveBellScreen.exe"
    RMDir /r "$INSTDIR\web"
    RMDir /r "$INSTDIR\data"
    RMDir /r "$INSTDIR\melo"
    RMDir /r "$INSTDIR\sherpa"
    RMDir /r "$INSTDIR\kokoro"
    Delete "$INSTDIR\sherpa-onnx-c-api.dll"
    Delete "$INSTDIR\onnxruntime.dll"
    Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  !endif
  Delete "$INSTDIR\OFL-Noto-CJK.txt"
  Delete "$INSTDIR\display-config.json"
  RMDir "$INSTDIR"

  !ifdef SCREEN
    Delete "$SMPROGRAMS\${APP_TITLE}\大屏端.lnk"
  !else
    Delete "$SMPROGRAMS\${APP_TITLE}\${APP_TITLE}.lnk"
  !endif
  Delete "$SMPROGRAMS\${APP_TITLE}\卸载.lnk"
  RMDir "$SMPROGRAMS\${APP_TITLE}"
  Delete "$DESKTOP\${APP_TITLE}.lnk"

  DeleteRegKey HKCU "${REG_KEY}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
