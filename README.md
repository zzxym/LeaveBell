# LeaveBell

## 发布版本

`v2.0.0` 起发布三个独立版本，命名统一为 `online` / `offline` / `screen`：

| 版本 | Windows | Linux | 说明 |
| --- | --- | --- | --- |
| online | `LeaveBell-online-windows-amd64.exe` | `LeaveBell-online-linux-amd64.deb` / `.rpm` | 控制器 + 大屏端，EdgeTTS 在线语音 |
| offline | `LeaveBell-offline-windows-amd64.exe` | `LeaveBell-offline-linux-amd64.deb` / `.rpm` | 控制器 + 大屏端 + MeloTTS 离线模型，可独立部署 |
| screen | `LeaveBell-screen-windows-amd64.exe` | `LeaveBell-screen-linux-amd64.deb` / `.rpm` | 独立大屏端，仅播放控制器推送的数据和音频 |

Linux 只发布 `amd64` / `arm64` 的 deb 和 rpm，不再发布 tar.gz；Windows 只发布 `amd64`。

控制器合成音频，大屏端/网页大屏负责播放。控制器可同时服务多个大屏。本地大屏开关默认开启，可在控制器界面关闭。

## 目录结构

```
Cargo.toml / Cargo.lock   Rust 工程
src/                      Rust 源码
web/                      大屏 / 老师端 / 管理端前端资源
assets/ preset/           图标、字体许可证与预置数据
build-win.ps1             Windows 构建，生成 online / screen 载荷
build-offline.sh          生成 offline 完整载荷
build-linux.sh            Linux 构建，生成 online / screen 载荷
build-linux-packages.sh   生成 online / offline / screen 的 deb / rpm
installer.nsi             参数化 NSIS 脚本，生成三个 Windows 独立安装包
.github/workflows/        Windows/Linux 多 job 构建、产物检测与发布
```

## 构建与运行

需要 Rust stable 工具链。

```
cargo build --release
cargo run --release
```

`LeaveBell` 是控制器，`LeaveBellScreen` 是大屏端；online/offline 包内附带的也是同一个 `LeaveBellScreen`。大屏端按 `F2` 打开设置，填写控制器地址，并可开关语音播放（默认开启）。首次运行会自动弹出设置窗口。

## 持久化数据

控制器数据默认保存在二进制文件同层的 `data/data.json`，独立大屏端配置保存在同层的 `display-config.json`。Linux deb/rpm 安装到 `/opt/LeaveBell-*` 时该目录不可写，状态文件改存到用户配置目录 `$XDG_CONFIG_HOME/LeaveBell/<安装目录名>`（未设置 `XDG_CONFIG_HOME` 时为 `~/.config/LeaveBell/<安装目录名>`）。离线版首次启动默认使用 sherpa 离线语音。

## 状态流转

`(0) → (1) → (2)`，单向不可逆。回退需在管理端「修正」并填写原因，记录日志。
