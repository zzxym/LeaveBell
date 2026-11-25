; LeaveBell Local Voice Patch Installer
; 用法: makensis installer-melo-patch.nsi

!define APP_NAME "LeaveBell"
!define APP_VERSION "2.0.0"
!define PATCH_NAME "LeaveBell 本地语音包补丁"
!define REG_KEY "Software\${APP_NAME}"
!ifndef ARCH
!define ARCH "amd64"
!endif
!ifndef PKG_DIR
!define PKG_DIR "patch-melo\payload"
!endif

Name "${PATCH_NAME}"
OutFile "LeaveBell-LocalVoice-Patch-${ARCH}.exe"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show

; 安装目录由主程序注册表决定，不显示目录选择页
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install Local Voice"
  ; 读取主程序安装路径
  ReadRegStr $INSTDIR HKCU "${REG_KEY}" "InstallDir"
  StrCmp $INSTDIR "" notfound found
  notfound:
    MessageBox MB_ICONSTOP|MB_OK "未检测到已安装的 LeaveBell，请先安装主程序后再运行本补丁。"
    Quit
  found:
    ; 校验目录下确实存在主程序
    IfFileExists "$INSTDIR\LeaveBell.exe" 0 notfound
    IfFileExists "$INSTDIR\web\set.html" 0 notfound

    ; 提示关闭正在运行的主程序，避免 exe 被占用导致替换失败
    MessageBox MB_ICONINFORMATION|MB_OK|MB_TOPMOST "安装将更新主程序与语音组件。$\r$\n请确保 LeaveBell 已完全退出（含任务栏托盘图标），再点击确定继续。"

    ; 覆盖主程序 + web + 运行时 DLL + 模型
    SetOutPath "$INSTDIR"
    File /r "${PKG_DIR}\*"

    ; 标记补丁已安装
    WriteRegStr HKCU "${REG_KEY}" "LocalVoice" "1"

    ; 写入卸载程序（覆盖主程序的，确保能清理全部补丁文件）
    WriteUninstaller "$INSTDIR\uninstall.exe"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "${APP_VERSION}"

    DetailPrint "本地语音包补丁安装完成：$INSTDIR"
    MessageBox MB_ICONINFORMATION|MB_OK "本地语音包补丁安装完成。$\r$\n现已支持 EdgeTTS 在线与 MeloTTS 离线语音（女声/男声）。$\r$\n请重新启动 LeaveBell，然后在“语音设置”中切换语音引擎。"
SectionEnd

; ---- 卸载：清理主程序 + 补丁全部文件 ----
Section "Uninstall"
  ReadRegStr $INSTDIR HKCU "${REG_KEY}" "InstallDir"
  StrCmp $INSTDIR "" 0 hasdir
    ; 注册表缺失时从卸载程序所在目录推断
    GetFullPathName $INSTDIR "$EXEDIR"
  hasdir:

  ; 主程序与卸载程序
  Delete "$INSTDIR\LeaveBell.exe"
  Delete "$INSTDIR\uninstall.exe"
  ; 补丁安装的 sherpa-onnx DLL（与主程序同目录）
  Delete "$INSTDIR\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  ; 补丁安装的 Kokoro 模型与遗留目录
  RMDir /r "$INSTDIR\kokoro"
  RMDir /r "$INSTDIR\sherpa"
  RMDir /r "$INSTDIR\melo"
  ; 主程序资源
  RMDir /r "$INSTDIR\web"
  RMDir /r "$INSTDIR\data"
  RMDir "$INSTDIR"

  ; 快捷方式
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\卸载.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  ; 注册表
  DeleteRegValue HKCU "${REG_KEY}" "LocalVoice"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
SectionEnd
