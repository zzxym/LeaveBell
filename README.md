# 放学系统（LeaveBell）

小学放学状态上大屏：班级实时显示，纯内网运行。核心为 Rust 单二进制（eframe/egui GUI + tiny_http 服务端），零外部运行时依赖。

## 语音版本

项目提供两个发行包：

| 版本 | 安装包 | 语音引擎 |
| --- | --- | --- |
| 在线语音精简版 | `LeaveBell-Online-Setup-1.0.0.exe` | EdgeTTS（在线，Rust 原生 `msedge-tts`） |
| 本地语音包补丁 | `LeaveBell-LocalVoice-Patch-1.0.0.exe` | EdgeTTS（在线）+ MeloTTS（离线，sherpa-onnx VITS，需先装精简版） |

本地语音包补丁在精简版基础上叠加，不单独运行；安装后可在管理端切换语音引擎。离线音色使用模型说话人 2 / 3，界面显示为「女声」「男声」。

## 目录结构

```
Cargo.toml / Cargo.lock   Rust 工程
src/                      Rust 源码（main/app/http_server/model/monitors/voice.rs）
web/                      精简版前端资源（大屏 / 老师端 / 管理端）
web-melo/                 补丁版前端资源源（含语音引擎选择 UI）
patch-melo/payload/       本地语音补丁内容
  web/                    补丁安装的前端资源
  melo/                   MeloTTS（sherpa-onnx VITS 中文）模型与运行时 DLL
assets/ preset/           图标与预置数据
build.rs                  构建脚本
build-win.ps1             Windows 发行目录构建
installer.nsi             精简版 NSIS 脚本
installer-melo-patch.nsi  补丁版 NSIS 脚本
dist/                     构建输出
```

## 构建与运行

需要 Rust 工具链（stable）。

```
cargo build --release
cargo run --release
```

运行后访问：

- 大屏端：`http://本机IP:port/`
- 老师端：`http://本机IP:port/con`
- 管理端：`http://本机IP:port/set`

端口优先级：**命令行参数 > 环境变量 `PORT` > data.json 持久化值 > 默认 80**。

首次运行自动生成 42 个班级（6 年级 × 7 班）和一个管理员账号（手机号 `admin`，初始密码 `Aa123456`）。

## 持久化数据

所有数据（班级设置、教师绑定、日志、大屏与定时配置、语音设置、端口）保存在**二进制文件同层目录的 `data/data.json`**，每次运行从该文件读取，无需额外配置文件。

## 老师绑定

在管理端「账号管理」里输入老师手机号、选择班级、点「绑定」。老师再到老师端用该手机号登录即可。

## 状态流转

`未出教室(0) → 已出教室前往校门(1) → 已到门口可接走(2)`，单向不可逆。回退需在管理端「修正」并填写原因，记录日志。

## 部署说明

- 单台机器同时跑服务端和大屏端，HDMI 输出到 LED 或电视
- 内网部署，无需外网；语音缓存与模型均随补丁包离线安装
