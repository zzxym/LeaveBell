; LeaveBell NSIS Installer
; 用法: makensis installer.nsi  (需安装 NSIS 3.x)

!define APP_NAME "LeaveBell"
!define APP_VERSION "2.0.0"
!define APP_EXE "LeaveBell.exe"
!define APP_PUBLISHER "School"
!define INSTALL_DIR "D:\LeaveBell"
!ifndef ARCH
!define ARCH "amd64"
!endif
!ifndef PKG_DIR
!define PKG_DIR "dist-native\school-display"
!endif

Name "LeaveBell 在线语音精简版"
OutFile "LeaveBell-Online-Setup-${ARCH}.exe"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUnInstDetails show

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

; ---- 安装 ----
Section "Install"
  SetOutPath "$INSTDIR"
  ; 主程序
  File "${PKG_DIR}\${APP_EXE}"
  File "${PKG_DIR}\LeaveBellDisplay.exe"
  File "${PKG_DIR}\OFL-Noto-CJK.txt"
  ; 基础版只含 web / data（语音使用在线 edge-tts，无需本地运行时）
  File /r "${PKG_DIR}\web"
  File /r "${PKG_DIR}\data"

  ; 写入注册表（当前用户）
  WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\${APP_NAME}" "Version" "${APP_VERSION}"

  ; 卸载程序
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher" "${APP_PUBLISHER}"

  ; 开始菜单 / 桌面快捷方式
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\大屏端.lnk" "$INSTDIR\LeaveBellDisplay.exe"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\卸载.lnk" "$INSTDIR\uninstall.exe"
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

; ---- 卸载 ----
Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LeaveBellDisplay.exe"
  Delete "$INSTDIR\OFL-Noto-CJK.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\web"
  RMDir /r "$INSTDIR\data"
  RMDir /r "$INSTDIR\sherpa"
  RMDir /r "$INSTDIR\kokoro"
  RMDir /r "$INSTDIR\melo"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\大屏端.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\卸载.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  DeleteRegKey HKCU "Software\${APP_NAME}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
SectionEnd
