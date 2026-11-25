#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod http_server;
mod license;
mod license_ui;
mod model;
mod monitors;
mod sherpa_onnx;
mod voice;

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() -> eframe::Result {
    let base = exe_dir();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--license-keygen") {
        let msg = license::keygen(&base).unwrap_or_else(|e| e);
        let _ = std::fs::write(base.join("license-tool-result.txt"), msg);
        return Ok(());
    }
    if let Some(pos) = args.iter().position(|a| a == "--license-sign") {
        let expires = args.get(pos + 1).cloned().unwrap_or_else(|| "2099-12-31".into());
        let features = args.get(pos + 2).cloned().unwrap_or_else(|| "full".into());
        let msg = license::sign(&base, &expires, &features).unwrap_or_else(|e| e);
        let _ = std::fs::write(base.join("license-tool-result.txt"), msg);
        return Ok(());
    }
    if args.iter().any(|a| a == "--license-check") {
        let r = license::verify(&base);
        let msg = format!("registered={}\nexpires={}\nmessage={}\n", r.ok, r.expires, r.message);
        let _ = std::fs::write(base.join("license-tool-result.txt"), msg);
        return Ok(());
    }
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    if args.iter().any(|a| a == "--license-ui") || exe_name.starts_with("LeaveBell-License-Tool") {
        let icon = Arc::new(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
                .expect("窗口图标加载失败"),
        );
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("LeaveBell 算号机")
                .with_inner_size([760.0, 560.0])
                .with_min_inner_size([600.0, 420.0])
                .with_icon(icon),
            ..Default::default()
        };
        return eframe::run_native(
            "LeaveBell 算号机",
            options,
            Box::new(move |_cc| Ok(Box::new(license_ui::LicenseToolApp::new(base)))),
        );
    }
    let license_state = Arc::new(Mutex::new(license::initial(&base)));
    // 固定使用 exe 所在目录下的 web/data，避免不同启动方式（cwd 不同）
    // 加载到不同的数据文件，导致保存的配置/排序在重启后"丢失"。
    let web_dir = base.join("web");
    let data_dir = model::state_dir(&base).join("data");
    let data_file = data_dir.join("data.json");
    let screen_cfg_path = data_dir.join("screen.json");

    let mut store = model::Store::load(data_file);
    // 优先级：命令行 > PORT 环境变量 > data.json.port > 默认端口
    let default_port = if cfg!(target_os = "linux") { 8080 } else { 80 };
    let port: u16 = std::env::args()
        .find_map(|a| a.parse::<u16>().ok())
        .or_else(|| std::env::var("PORT").ok().and_then(|v| v.parse().ok()))
        .or_else(|| store.data["port"].as_u64().and_then(|v| u16::try_from(v).ok()))
        .unwrap_or(default_port);
    let mut bind_ip = store.data["bindIp"].as_str().unwrap_or("").trim().to_string();
    if bind_ip.is_empty() {
        let ips = app::local_ipv4s();
        bind_ip = ips.iter().find(|s| app::is_private_ipv4(s)).cloned()
            .unwrap_or_else(|| "0.0.0.0".to_string());
        store.data["bindIp"] = serde_json::json!(bind_ip);
        store.persist();
    }

    let broadcast = Arc::new(http_server::Broadcast::new());
    {
        let b = broadcast.clone();
        voice::init_broadcast(move |event, data| b.send_data(event, data));
    }
    voice::init_settings(store.data["voice"].clone());
    voice::init_engine(voice::VoiceEngine::new(base.clone()));
    // 音色为空或与当前引擎不匹配时，自动选用该引擎第一个可用音色
    let eng = store.data["voice"]["engine"].as_str().unwrap_or("edge").to_string();
    let voices = voice::engine().map(|e| e.list_voices(&eng)).unwrap_or_default();
    let current = store.data["voice"]["voice"].as_str().unwrap_or("");
    if !voices.iter().any(|v| v.as_str() == current) {
        if let Some(first) = voices.first() {
            store.data["voice"]["voice"] = serde_json::json!(first);
            store.persist();
            voice::update_settings(store.data["voice"].clone());
        }
    }

    let screens = monitors::enumerate();
    let screen_cfg = monitors::ScreenConfig::load(screen_cfg_path);

    // 按选定大屏的实际物理分辨率注入画布尺寸，管理端预览据此等比铺满
    if let Some(sid) = screen_cfg.display_screen() {
        if let Some(s) = screens.iter().find(|s| s.id == sid) {
            store.display_mut()["width"] = serde_json::json!(s.width as i64);
            store.display_mut()["height"] = serde_json::json!(s.height as i64);
            store.persist();
        }
    }

    // 首次启动需先完成密码+端口设置，之后才允许 HTTP 服务监听
    let first_launch = store.data["admin"]["password"].as_str().unwrap_or("").is_empty();
    let server_ready = Arc::new(Mutex::new(!first_launch));

    let store = Arc::new(Mutex::new(store));

    // 端口共享状态：(当前端口, 启动错误)。占用时由界面弹窗让用户更换。
    let port_state = Arc::new(Mutex::new((port, None::<String>)));

    // HTTP 服务器在独立线程运行；端口占用时等待界面修改端口后重试
    {
        let store = store.clone();
        let broadcast = broadcast.clone();
        let port_state = port_state.clone();
        let server_ready = server_ready.clone();
        let bind_ip = bind_ip.clone();
        std::thread::spawn(move || {
            while !*server_ready.lock().unwrap() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            loop {
                let p = {
                    let ps = port_state.lock().unwrap();
                    if ps.1.is_some() {
                        // 有未处理的错误，等待界面修改端口后清除错误
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        continue;
                    }
                    ps.0
                };
                match http_server::start_server(&bind_ip, p, store.clone(), broadcast.clone(), web_dir.clone()) {
                    Ok(()) => break,
                    Err(e) => {
                        eprintln!("{}", e);
                        port_state.lock().unwrap().1 = Some(e);
                    }
                }
            }
        });
    }

    let screen_cfg = Arc::new(Mutex::new(screen_cfg));

    let icon = Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
            .expect("窗口图标加载失败"),
    );
    let controller_on_top = {
        let cfg = screen_cfg.lock().unwrap();
        let display = cfg.display_screen().unwrap_or("");
        let controller = cfg.controller_screen().unwrap_or("");
        screens.len() < 2 || display.is_empty() || controller.is_empty() || display == controller
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("放学系统 · 本地控制器")
            .with_inner_size([1100.0, 820.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(icon)
            .with_window_level(if controller_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }),
        ..Default::default()
    };

    eframe::run_native(
        "放学系统",
        options,
        Box::new(move |_cc| Ok(Box::new(app::App::new(store, broadcast, screens, screen_cfg, port_state, server_ready, license_state)))),
    )
}
