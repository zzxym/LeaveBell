use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::sherpa_onnx::SherpaTts;

/// 播报任务：文本 + 班级编号（试听时班级编号为空）
#[derive(Clone)]
pub struct SpeakJob {
    pub text: String,
    pub class_id: String,
}

/// 语音引擎：通过 mpsc 串行队列保证播报不重叠。
/// sherpa-onnx 引擎在进程内常驻，模型加载一次后复用，无子进程冷启动。
#[derive(Clone)]
pub struct VoiceEngine {
    tx: Sender<SpeakJob>,
    /// 当前正在播报的班级编号，大屏渲染据此显示边框动画
    pub speaking: Arc<Mutex<Option<String>>>,
    /// 实际执行用的 ASCII 根路径（中文路径下 TTS 运行时会失败）
    base: PathBuf,
    /// 常驻 sherpa-onnx 引擎，仅播报线程访问
    sherpa: Arc<Mutex<Option<SherpaTts>>>,
}

/// 部分 TTS 运行时无法处理非 ASCII 路径。若安装目录含中文，把子目录复制到
/// %TEMP%\school-display-tts 作为执行副本；否则直接用原目录。
fn prepare_base(original: &Path, sub: &str, marker: &str) -> PathBuf {
    let has_non_ascii = original.to_string_lossy().chars().any(|c| !c.is_ascii());
    if !has_non_ascii {
        return original.to_path_buf();
    }
    let src = original.join(sub);
    if !src.is_dir() {
        return original.to_path_buf();
    }
    let staging = std::env::temp_dir().join("school-display-tts");
    let dst = staging.join(sub);
    let need_copy = !dst.join(marker).is_file()
        || fs_mtime(&src.join(marker)) > fs_mtime(&dst.join(marker));
    if need_copy {
        let _ = std::fs::remove_dir_all(&dst);
        copy_dir_recursive(&src, &dst);
    }
    staging
}

fn fs_mtime(p: &Path) -> std::time::SystemTime {
    std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH)
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    if let Err(_) = std::fs::create_dir_all(dst) { return; }
    if let Ok(entries) = std::fs::read_dir(src) {
        for e in entries.flatten() {
            let from = e.path();
            let to = dst.join(e.file_name());
            if from.is_dir() { copy_dir_recursive(&from, &to); }
            else { let _ = std::fs::copy(&from, &to); }
        }
    }
}

impl VoiceEngine {
    pub fn new(base: PathBuf) -> Self {
        let base = prepare_base(&base, "melo", "models/sherpa-onnx-vits-zh-ll/model.onnx");
        let (tx, rx) = mpsc::channel::<SpeakJob>();
        let speaking = Arc::new(Mutex::new(None::<String>));
        let speak_state = speaking.clone();
        let base_thread = base.clone();
        let sherpa = Arc::new(Mutex::new(None::<SherpaTts>));
        let sherpa_thread = sherpa.clone();
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                *speak_state.lock().unwrap() = if job.class_id.is_empty() { None } else { Some(job.class_id.clone()) };
                // 通知大屏开始播报（带班级编号）
                if !job.class_id.is_empty() {
                    let data = format!("{{\"class_id\":{}}}", serde_json::to_string(&job.class_id).unwrap());
                    if let Some(send) = broadcast() { send("voice_start", &data); }
                }
                let _ = speak_tts(&base_thread, &sherpa_thread, &job);
                *speak_state.lock().unwrap() = None;
                if !job.class_id.is_empty() {
                    if let Some(send) = broadcast() { send("voice_end", "{}"); }
                }
            }
        });
        let engine = Self { tx, speaking, base, sherpa };
        // 启动时若已选中 sherpa，预先加载模型
        if voice_settings_snapshot()["engine"].as_str() == Some("sherpa") {
            engine.ensure_sherpa();
        }
        engine
    }

    /// 确保 sherpa-onnx 引擎已加载（设置切到 sherpa 时调用，消除首次播报的冷启动）
    pub fn ensure_sherpa(&self) {
        let mut guard = self.sherpa.lock().unwrap();
        if guard.is_none() {
            let model_dir = self.base.join("melo").join("models").join("sherpa-onnx-vits-zh-ll");
            *guard = SherpaTts::load_vits(&model_dir.to_string_lossy()).ok();
        }
    }

    pub fn speak(&self, text: String, class_id: String) {
        let _ = self.tx.send(SpeakJob { text, class_id });
    }

    /// 按当前引擎枚举音色列表
    pub fn list_voices(&self, engine: &str) -> Vec<String> {
        if engine == "edge" {
            return edge_voices();
        }
        // sherpa-onnx VITS（MeloTTS）：仅保留两个说话人，显示为女声/男声（前端映射）
        vec![
            "sherpa-onnx-vits-zh-ll/2".into(),
            "sherpa-onnx-vits-zh-ll/3".into(),
        ]
    }

    /// 试听：读取当前设置生成一段固定文本播报（不绑定班级）
    pub fn test_from_settings(&self, settings: &Value, text: &str) {
        if settings["voice"].as_str().unwrap_or("").is_empty() {
            return;
        }
        self.speak(text.to_string(), String::new());
    }
}

/// 状态切换播报：读取设置，若开启则入队。班级名/状态标签由调用方传入。
pub fn speak_status(class_name: &str, status_label: &str, class_id: &str) {
    let settings = voice_settings_snapshot();
    if !settings["enabled"].as_bool().unwrap_or(false) {
        return;
    }
    if settings["voice"].as_str().unwrap_or("").is_empty() {
        return;
    }
    if let Some(engine) = engine() {
        engine.speak(format!("{}，{}", class_name, status_label), class_id.to_string());
    }
}

/// 按当前设置选择 TTS 引擎生成 wav 并同步播放。
fn speak_tts(base: &Path, sherpa: &Mutex<Option<SherpaTts>>, job: &SpeakJob) -> Result<(), String> {
    let settings = voice_settings_snapshot();
    match settings["engine"].as_str() {
        Some("sherpa") => speak_sherpa(base, sherpa, job, &settings),
        _ => speak_edge(job, &settings),
    }
}

/// edge-tts 可用中文音色（微软神经网络音色，需联网）
fn edge_voices() -> Vec<String> {
    vec![
        "zh-CN-XiaoxiaoNeural".into(),
        "zh-CN-XiaoyiNeural".into(),
        "zh-CN-YunjianNeural".into(),
        "zh-CN-YunxiNeural".into(),
        "zh-CN-YunyangNeural".into(),
        "zh-CN-YunxiaNeural".into(),
        "zh-CN-liaoning-XiaobeiNeural".into(),
        "zh-CN-shaanxi-XiaoniNeural".into(),
        "zh-TW-HsiaoChenNeural".into(),
        "zh-HK-HiuGaaiNeural".into(),
    ]
}

/// 调 edge-tts 合成 MP3 并用 MCI 同步播放
fn speak_edge(job: &SpeakJob, settings: &Value) -> Result<(), String> {
    use msedge_tts::tts::client::connect;
    use msedge_tts::tts::SpeechConfig;

    let voice = settings["voice"].as_str().unwrap_or("");
    if voice.is_empty() {
        return Err("未选择音色".to_string());
    }
    let speed = settings["speed"].as_f64().unwrap_or(1.0).clamp(0.5, 2.0);
    let rate = ((speed - 1.0) * 100.0).round() as i32;
    let volume = settings["volume"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0);
    let vol_pct = ((volume - 1.0) * 100.0).round() as i32;

    let cfg = SpeechConfig {
        voice_name: voice.to_string(),
        audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
        pitch: 0,
        rate,
        volume: vol_pct,
    };
    let mut tts = connect().map_err(|e| format!("连接 edge-tts 失败: {}", e))?;
    let text = job.text.replacen('，', "。", 1);
    let audio = tts.synthesize(&text, &cfg).map_err(|e| format!("edge-tts 合成失败: {}", e))?;
    if audio.audio_bytes.is_empty() {
        return Err("edge-tts 返回空音频".to_string());
    }
    let out = std::env::temp_dir().join(format!("school-edge-{}-{}.mp3",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
    ));
    std::fs::write(&out, &audio.audio_bytes).map_err(|e| e.to_string())?;
    let result = play_mp3(&out);
    let _ = std::fs::remove_file(&out);
    result
}

/// 用 sherpa-onnx VITS（MeloTTS）引擎合成 wav 并同步播放。
/// 引擎未加载时自动加载；合成失败自动重试一次。
fn speak_sherpa(base: &Path, sherpa: &Mutex<Option<SherpaTts>>, job: &SpeakJob, settings: &Value) -> Result<(), String> {
    let voice = settings["voice"].as_str().unwrap_or("");
    if voice.is_empty() {
        return Err("未选择音色".to_string());
    }
    // 音色格式：sherpa-onnx-vits-zh-ll/2（女声）或 /3（男声），取斜杠后的 sid
    let sid: i32 = voice.rsplit('/').next()
        .ok_or_else(|| format!("无效的音色: {}", voice))?
        .parse()
        .map_err(|_| format!("无效的说话人编号: {}", voice))?;
    let speed = settings["speed"].as_f64().unwrap_or(1.0).clamp(0.5, 2.0) as f32;
    let out = std::env::temp_dir().join(format!("school-sherpa-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
    ));
    let text = job.text.replacen('，', "。", 1);
    let out_str = out.to_string_lossy().to_string();

    let mut result = Err("sherpa-onnx 未响应".to_string());
    for _ in 0..2 {
        result = {
            let mut guard = sherpa.lock().unwrap();
            if guard.is_none() {
                let model_dir = base.join("melo").join("models").join("sherpa-onnx-vits-zh-ll");
                *guard = SherpaTts::load_vits(&model_dir.to_string_lossy()).ok();
            }
            match guard.as_ref() {
                Some(engine) => engine.synthesize_wav(&text, sid, speed, &out_str),
                None => Err("sherpa-onnx 引擎加载失败".to_string()),
            }
        };
        if result.is_ok() { break; }
        // 请求失败：丢弃引擎，下一轮重建
        *sherpa.lock().unwrap() = None;
    }
    result?;
    let played = play_wav(&out);
    let _ = std::fs::remove_file(&out);
    played
}

#[cfg(target_os = "windows")]
fn play_wav(path: &Path) -> Result<(), String> {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_FILENAME, SND_SYNC};
    use windows::core::PCWSTR;
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let ok = PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_SYNC).as_bool();
        if ok { Ok(()) } else { Err("PlaySoundW 播放失败".to_string()) }
    }
}

#[cfg(not(target_os = "windows"))]
fn play_external(path: &Path, mp3: bool) -> Result<(), String> {
    use std::process::Command;
    let p = path.to_string_lossy();
    let candidates: Vec<(&str, Vec<&str>)> = if mp3 {
        vec![
            ("ffplay", vec!["-nodisp", "-autoexit", p.as_ref()]),
            ("mpg123", vec!["-q", p.as_ref()]),
        ]
    } else {
        vec![
            ("paplay", vec![p.as_ref()]),
            ("aplay", vec![p.as_ref()]),
            ("ffplay", vec!["-nodisp", "-autoexit", p.as_ref()]),
        ]
    };
    for (cmd, args) in candidates {
        if let Ok(status) = Command::new(cmd).args(args).status() {
            if status.success() { return Ok(()); }
        }
    }
    Err("未找到可用的 Linux 音频播放器（paplay/aplay/ffplay/mpg123）".to_string())
}

#[cfg(not(target_os = "windows"))]
fn play_wav(path: &Path) -> Result<(), String> {
    play_external(path, false)
}

/// 用 Windows MCI 同步播放 MP3（PlaySoundW 不支持 MP3）
#[cfg(target_os = "windows")]
fn play_mp3(path: &Path) -> Result<(), String> {
    use windows::Win32::Media::Multimedia::mciSendStringW;
    use windows::core::PCWSTR;
    fn mci(cmd: &str) -> Result<(), String> {
        let w: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
        let rc = unsafe { mciSendStringW(PCWSTR(w.as_ptr()), None, None) };
        if rc == 0 { Ok(()) } else { Err(format!("MCI 命令失败({}): {}", rc, cmd)) }
    }
    let p = path.to_string_lossy().replace('\\', "\\\\");
    let alias = format!("lb{}", std::process::id());
    let open = format!("open \"{}\" type mpegvideo alias {}", p, alias);
    mci(&open)?;
    let result = mci(&format!("play {} wait", alias));
    let _ = mci(&format!("close {}", alias));
    result
}

#[cfg(not(target_os = "windows"))]
fn play_mp3(path: &Path) -> Result<(), String> {
    play_external(path, true)
}

/// 全局广播回调：(event, data) -> ()，由 main 注入，避免 voice 依赖 http_server
static BROADCAST: std::sync::OnceLock<Box<dyn Fn(&str, &str) + Send + Sync>> = std::sync::OnceLock::new();

pub fn init_broadcast<F: Fn(&str, &str) + Send + Sync + 'static>(f: F) {
    let _ = BROADCAST.set(Box::new(f));
}

fn broadcast() -> Option<&'static (dyn Fn(&str, &str) + Send + Sync)> {
    BROADCAST.get().map(|b| b.as_ref())
}

/// 全局语音引擎，HTTP 层与 UI 层共用
static ENGINE: std::sync::OnceLock<VoiceEngine> = std::sync::OnceLock::new();

pub fn init_engine(engine: VoiceEngine) {
    let _ = ENGINE.set(engine);
}

pub fn engine() -> Option<&'static VoiceEngine> {
    ENGINE.get()
}

/// 语音设置在启动时注入，播报线程通过此快照读取
static VOICE_SETTINGS: std::sync::OnceLock<Mutex<Value>> = std::sync::OnceLock::new();

pub fn init_settings(settings: Value) {
    let _ = VOICE_SETTINGS.set(Mutex::new(settings));
}

pub fn update_settings(settings: Value) {
    let sherpa_selected = settings["engine"].as_str() == Some("sherpa");
    if let Some(s) = VOICE_SETTINGS.get() {
        *s.lock().unwrap() = settings;
    }
    // 管理端切换到 sherpa 时立即加载模型，消除首次播报冷启动
    if sherpa_selected {
        if let Some(e) = engine() { e.ensure_sherpa(); }
    }
}

pub fn voice_settings_snapshot() -> Value {
    VOICE_SETTINGS.get()
        .map(|s| s.lock().unwrap().clone())
        .unwrap_or_else(|| json!({"voice": "", "speed": 1.0, "volume": 1.0}))
}
