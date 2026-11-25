#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

#[allow(dead_code)] mod app;
#[allow(dead_code)] mod http_server;
#[allow(dead_code)] mod license;
#[allow(dead_code)] mod license_ui;
#[allow(dead_code)] mod model;
#[allow(dead_code)] mod monitors;
#[allow(dead_code)] mod sherpa_onnx;
#[allow(dead_code)] mod voice;

use crate::app::{draw_display_grid_for_client, setup_chinese_font_once};
use eframe::egui::{self, ViewportBuilder, ViewportCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
struct DisplayConfig {
    controller: String,
    voice: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        // 与控制器端口保持一致：Linux 系统包使用非特权端口 8080，Windows 仍为 80。
        let controller = if cfg!(target_os = "linux") {
            "http://127.0.0.1:8080"
        } else {
            "http://127.0.0.1:80"
        };
        Self { controller: controller.into(), voice: true }
    }
}

impl DisplayConfig {
    fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) {
        let _ = fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn normalize_base(s: &str) -> String {
    let s = s.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") { s.to_string() } else { format!("http://{}", s) }
}

fn fetch_json(url: &str) -> Option<Value> {
    ureq::get(url).timeout(Duration::from_secs(3)).call().ok()?.into_json().ok()
}

fn fetch_audio(url: &str) -> Option<(Vec<u8>, bool)> {
    let res = ureq::get(url).timeout(Duration::from_secs(5)).call().ok()?;
    let mime = res.header("Content-Type").unwrap_or("").to_string();
    let mut buf = Vec::new();
    res.into_reader().read_to_end(&mut buf).ok()?;
    Some((buf, mime.contains("mpeg") || url.contains("mp3")))
}

fn play_audio(base: &str, audio: &str) {
    let url = if audio.starts_with("http://") || audio.starts_with("https://") {
        audio.to_string()
    } else {
        format!("{}{}", base.trim_end_matches('/'), audio)
    };
    std::thread::spawn(move || {
        if let Some((bytes, mp3)) = fetch_audio(&url) {
            let ext = if mp3 { "mp3" } else { "wav" };
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
            let path = std::env::temp_dir().join(format!("lb-display-{}-{}.{}", std::process::id(), stamp, ext));
            if fs::write(&path, bytes).is_ok() {
                let _ = if mp3 { voice::play_mp3(&path) } else { voice::play_wav(&path) };
                let _ = fs::remove_file(&path);
            }
        }
    });
}

struct DisplayApp {
    cfg: DisplayConfig,
    cfg_path: PathBuf,
    base: String,
    screens: Vec<monitors::ScreenInfo>,
    classes: Vec<Value>,
    display: Value,
    labels: Vec<String>,
    last_fetch: Instant,
    last_audio: String,
    speaking: String,
    settings_open: bool,
    controller_input: String,
    voice_input: bool,
    msg: String,
    fullscreen_set: bool,
}

impl DisplayApp {
    fn new(cfg: DisplayConfig, cfg_path: PathBuf) -> Self {
        let base = normalize_base(&cfg.controller);
        Self {
            controller_input: cfg.controller.clone(),
            voice_input: cfg.voice,
            cfg,
            cfg_path,
            base,
            screens: monitors::enumerate(),
            classes: Vec::new(),
            display: Value::Null,
            labels: Vec::new(),
            last_fetch: Instant::now() - Duration::from_secs(10),
            last_audio: String::new(),
            speaking: String::new(),
            settings_open: false,
            msg: String::new(),
            fullscreen_set: false,
        }
    }

    fn refresh(&mut self) {
        if self.last_fetch.elapsed() < Duration::from_secs(1) { return; }
        self.last_fetch = Instant::now();

        let classes_url = format!("{}/api/classes", self.base);
        let display_url = format!("{}/api/display", self.base);
        let voice_url = format!("{}/api/voice/current", self.base);

        if let Some(v) = fetch_json(&classes_url) {
            if let Some(arr) = v["data"].as_array() { self.classes = arr.clone(); }
        }
        if let Some(v) = fetch_json(&display_url) {
            self.display = v["data"].clone();
            self.labels = self.display["statusLabels"].as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
        }
        if let Some(v) = fetch_json(&voice_url) {
            let d = &v["data"];
            let id = d["id"].as_str().unwrap_or("").to_string();
            if self.cfg.voice && !id.is_empty() && id != self.last_audio {
                self.last_audio = id;
                self.speaking = d["class_id"].as_str().unwrap_or("").to_string();
                if let Some(audio) = d["audio"].as_str() { play_audio(&self.base, audio); }
            }
        }
    }

    fn save_settings(&mut self) {
        self.cfg.controller = normalize_base(&self.controller_input);
        self.cfg.voice = self.voice_input;
        self.base = self.cfg.controller.clone();
        self.cfg.save(&self.cfg_path);
        self.last_audio.clear();
        self.last_fetch = Instant::now() - Duration::from_secs(10);
        self.msg = "已保存".into();
    }
}

impl eframe::App for DisplayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        setup_chinese_font_once(ctx);
        ctx.request_repaint_after(Duration::from_millis(500));
        if !self.fullscreen_set {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            self.fullscreen_set = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F2)) { self.settings_open = true; }
        self.refresh();

        egui::CentralPanel::default().show(ctx, |ui| {
            let speaking = if self.speaking.is_empty() { None } else { Some(self.speaking.as_str()) };
            draw_display_grid_for_client(ui, &self.classes, &self.display, &self.labels, &self.screens, speaking);
        });

        let mut open = self.settings_open;
        let mut save = false;
        let msg = self.msg.clone();
        egui::Window::new("大屏端设置 (F2)")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("控制器地址");
                ui.text_edit_singleline(&mut self.controller_input);
                ui.checkbox(&mut self.voice_input, "语音播放");
                if ui.button("保存").clicked() { save = true; }
                if !msg.is_empty() { ui.label(&msg); }
            });
        self.settings_open = open;
        if save { self.save_settings(); }
    }
}

fn main() -> eframe::Result {
    let base = exe_dir();
    let cfg_path = model::state_dir(&base).join("display-config.json");
    let cfg = DisplayConfig::load(&cfg_path);
    let icon = Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
            .expect("窗口图标加载失败"),
    );
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("LeaveBell 大屏端")
            .with_fullscreen(true)
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "LeaveBell 大屏端",
        options,
        Box::new(move |_| Ok(Box::new(DisplayApp::new(cfg, cfg_path)))),
    )
}

use std::sync::Arc;
