//! sherpa-onnx C API 的 FFI 绑定与安全封装。
//!
//! 通过 `sherpa-onnx-c-api.dll` 直接在进程内加载 VITS 中文（MeloTTS）模型，
//! 避免子进程冷启动与 Python 依赖。模型加载一次后常驻，多次合成复用。

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

// ---- C 结构体映射（字段顺序与 c-api.h 严格一致）----

#[repr(C)]
pub struct SherpaOnnxOfflineTtsVitsModelConfig {
    pub model: *const c_char,
    pub lexicon: *const c_char,
    pub tokens: *const c_char,
    pub data_dir: *const c_char,
    pub noise_scale: f32,
    pub noise_scale_w: f32,
    pub length_scale: f32,
    pub dict_dir: *const c_char,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsMatchaModelConfig {
    pub acoustic_model: *const c_char,
    pub vocoder: *const c_char,
    pub lexicon: *const c_char,
    pub tokens: *const c_char,
    pub data_dir: *const c_char,
    pub noise_scale: f32,
    pub length_scale: f32,
    pub dict_dir: *const c_char,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsKokoroModelConfig {
    pub model: *const c_char,
    pub voices: *const c_char,
    pub tokens: *const c_char,
    pub data_dir: *const c_char,
    pub length_scale: f32,
    pub dict_dir: *const c_char,
    pub lexicon: *const c_char,
    pub lang: *const c_char,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsKittenModelConfig {
    pub model: *const c_char,
    pub voices: *const c_char,
    pub tokens: *const c_char,
    pub data_dir: *const c_char,
    pub length_scale: f32,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsZipvoiceModelConfig {
    pub tokens: *const c_char,
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub vocoder: *const c_char,
    pub data_dir: *const c_char,
    pub lexicon: *const c_char,
    pub feat_scale: f32,
    pub t_shift: f32,
    pub target_rms: f32,
    pub guidance_scale: f32,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsPocketModelConfig {
    pub lm_flow: *const c_char,
    pub lm_main: *const c_char,
    pub encoder: *const c_char,
    pub decoder: *const c_char,
    pub text_conditioner: *const c_char,
    pub vocab_json: *const c_char,
    pub token_scores_json: *const c_char,
    pub voice_embedding_cache_capacity: c_int,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsSupertonicModelConfig {
    pub duration_predictor: *const c_char,
    pub text_encoder: *const c_char,
    pub vector_estimator: *const c_char,
    pub vocoder: *const c_char,
    pub tts_json: *const c_char,
    pub unicode_indexer: *const c_char,
    pub voice_style: *const c_char,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsModelConfig {
    pub vits: SherpaOnnxOfflineTtsVitsModelConfig,
    pub num_threads: c_int,
    pub debug: c_int,
    pub provider: *const c_char,
    pub matcha: SherpaOnnxOfflineTtsMatchaModelConfig,
    pub kokoro: SherpaOnnxOfflineTtsKokoroModelConfig,
    pub kitten: SherpaOnnxOfflineTtsKittenModelConfig,
    pub zipvoice: SherpaOnnxOfflineTtsZipvoiceModelConfig,
    pub pocket: SherpaOnnxOfflineTtsPocketModelConfig,
    pub supertonic: SherpaOnnxOfflineTtsSupertonicModelConfig,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTtsConfig {
    pub model: SherpaOnnxOfflineTtsModelConfig,
    pub rule_fsts: *const c_char,
    pub max_num_sentences: c_int,
    pub rule_fars: *const c_char,
    pub silence_scale: f32,
}

#[repr(C)]
pub struct SherpaOnnxGeneratedAudio {
    pub samples: *const f32,
    pub n: c_int,
    pub sample_rate: c_int,
}

#[repr(C)]
pub struct SherpaOnnxGenerationConfig {
    pub silence_scale: f32,
    pub speed: f32,
    pub sid: c_int,
    pub reference_audio: *const f32,
    pub reference_audio_len: c_int,
    pub reference_sample_rate: c_int,
    pub reference_text: *const c_char,
    pub num_steps: c_int,
    pub extra: *const c_char,
}

#[repr(C)]
pub struct SherpaOnnxOfflineTts {
    _private: [u8; 0],
}

// 进度回调类型（本集成不使用，传 NULL）
type SherpaOnnxGeneratedAudioProgressCallbackWithArg =
    Option<unsafe extern "C" fn(*const f32, c_int, f32, *mut std::ffi::c_void) -> c_int>;

// ---- 原始 FFI：运行时按需加载，缺 DLL 时在线版仍可启动 ----

type CreateTts = unsafe extern "C" fn(*const SherpaOnnxOfflineTtsConfig) -> *const SherpaOnnxOfflineTts;
type DestroyTts = unsafe extern "C" fn(*const SherpaOnnxOfflineTts);
type TtsInt = unsafe extern "C" fn(*const SherpaOnnxOfflineTts) -> c_int;
type Generate = unsafe extern "C" fn(
    *const SherpaOnnxOfflineTts,
    *const c_char,
    *const SherpaOnnxGenerationConfig,
    SherpaOnnxGeneratedAudioProgressCallbackWithArg,
    *mut std::ffi::c_void,
) -> *const SherpaOnnxGeneratedAudio;
type DestroyAudio = unsafe extern "C" fn(*const SherpaOnnxGeneratedAudio);
type WriteWave = unsafe extern "C" fn(*const f32, c_int, c_int, *const c_char) -> c_int;

struct SherpaApi {
    create: CreateTts,
    destroy: DestroyTts,
    sample_rate: TtsInt,
    num_speakers: TtsInt,
    generate: Generate,
    destroy_audio: DestroyAudio,
    write_wave: WriteWave,
}

static API: std::sync::OnceLock<Result<(libloading::Library, SherpaApi), String>> = std::sync::OnceLock::new();

fn api() -> Result<&'static SherpaApi, String> {
    API.get_or_init(|| unsafe {
        let file = if cfg!(target_os = "windows") {
            "sherpa-onnx-c-api.dll"
        } else if cfg!(target_os = "macos") {
            "libsherpa-onnx-c-api.dylib"
        } else {
            "libsherpa-onnx-c-api.so"
        };
        let lib = libloading::Library::new(file)
            .map_err(|e| format!("加载 {} 失败: {}", file, e))?;
        macro_rules! sym {
            ($name:literal, $t:ty) => {{
                *lib.get::<$t>(concat!($name, "\0").as_bytes())
                    .map_err(|e| format!("缺少符号 {}: {}", $name, e))?
            }};
        }
        let api = SherpaApi {
            create: sym!("SherpaOnnxCreateOfflineTts", CreateTts),
            destroy: sym!("SherpaOnnxDestroyOfflineTts", DestroyTts),
            sample_rate: sym!("SherpaOnnxOfflineTtsSampleRate", TtsInt),
            num_speakers: sym!("SherpaOnnxOfflineTtsNumSpeakers", TtsInt),
            generate: sym!("SherpaOnnxOfflineTtsGenerateWithConfig", Generate),
            destroy_audio: sym!("SherpaOnnxDestroyOfflineTtsGeneratedAudio", DestroyAudio),
            write_wave: sym!("SherpaOnnxWriteWave", WriteWave),
        };
        Ok((lib, api))
    })
    .as_ref()
    .map(|(_, api)| api)
    .map_err(Clone::clone)
}

// ---- 安全封装 ----

/// 已加载的 sherpa-onnx 离线 TTS 引擎。
pub struct SherpaTts {
    tts: *const SherpaOnnxOfflineTts,
    #[allow(dead_code)]
    sample_rate: i32,
    #[allow(dead_code)]
    num_speakers: i32,
}

/// 将 Rust 路径转为 C 字符串（失败时返回 None）。
fn cstr(p: &str) -> Option<CString> {
    CString::new(p).ok()
}

impl SherpaTts {
    /// 用 VITS 中文（MeloTTS）模型创建 TTS 引擎。
    ///
    /// `model_dir` 下需包含：model.onnx、tokens.txt、lexicon.txt，以及
    /// date.fst / number.fst / phone.fst / new_heteronym.fst（中文数字规范化）。
    pub fn load_vits(model_dir: &str) -> Result<Self, String> {
        let model = cstr(&format!("{}/model.onnx", model_dir))
            .ok_or("路径含空字节")?;
        let tokens = cstr(&format!("{}/tokens.txt", model_dir))
            .ok_or("路径含空字节")?;
        let lexicon = cstr(&format!("{}/lexicon.txt", model_dir))
            .ok_or("路径含空字节")?;
        let provider = cstr("cpu").ok_or("路径含空字节")?;
        // 中文数字/日期/电话规范化规则，逗号分隔（与 CLI --tts-rule-fsts 一致）
        let rule_fsts = cstr(&format!(
            "{}/date.fst,{}/number.fst,{}/phone.fst,{}/new_heteronym.fst",
            model_dir, model_dir, model_dir, model_dir
        )).ok_or("路径含空字节")?;

        // 整个配置结构体零初始化，再只填 vits 字段
        let mut cfg: SherpaOnnxOfflineTtsConfig = unsafe { std::mem::zeroed() };
        cfg.model.num_threads = 2;
        cfg.model.debug = 0;
        cfg.model.provider = provider.as_ptr();
        cfg.model.vits.model = model.as_ptr();
        cfg.model.vits.tokens = tokens.as_ptr();
        cfg.model.vits.lexicon = lexicon.as_ptr();
        cfg.model.vits.noise_scale = 0.667;
        cfg.model.vits.noise_scale_w = 0.8;
        cfg.model.vits.length_scale = 1.0;
        cfg.rule_fsts = rule_fsts.as_ptr();
        cfg.max_num_sentences = 1;
        cfg.silence_scale = 0.2;

        let api = api()?;
        let tts = unsafe { (api.create)(&cfg) };
        if tts.is_null() {
            return Err("sherpa-onnx 创建 TTS 引擎失败（模型路径或文件有误）".to_string());
        }
        let sample_rate = unsafe { (api.sample_rate)(tts) };
        let num_speakers = unsafe { (api.num_speakers)(tts) };
        Ok(Self {
            tts,
            sample_rate,
            num_speakers,
        })
    }

    #[allow(dead_code)]
    pub fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    #[allow(dead_code)]
    pub fn num_speakers(&self) -> i32 {
        self.num_speakers
    }

    /// 合成文本为 WAV 文件。
    ///
    /// `sid` 为说话人编号，`speed` 为语速（>1 更快）。
    /// 成功返回 Ok(())，失败返回 Err(描述)。
    pub fn synthesize_wav(&self, text: &str, sid: i32, speed: f32, out_wav: &str) -> Result<(), String> {
        let text_c = cstr(text).ok_or("文本含空字节")?;
        let out_c = cstr(out_wav).ok_or("输出路径含空字节")?;

        let mut gen: SherpaOnnxGenerationConfig = unsafe { std::mem::zeroed() };
        gen.speed = speed;
        gen.sid = sid;
        gen.silence_scale = 0.2;

        let api = api()?;
        let audio = unsafe {
            (api.generate)(
                self.tts,
                text_c.as_ptr(),
                &gen,
                None,
                ptr::null_mut(),
            )
        };
        if audio.is_null() {
            return Err("sherpa-onnx 合成失败".to_string());
        }

        let audio_ref: &SherpaOnnxGeneratedAudio = unsafe { &*audio };
        let ok = unsafe {
            (api.write_wave)(
                audio_ref.samples,
                audio_ref.n,
                audio_ref.sample_rate,
                out_c.as_ptr(),
            )
        };
        let written = ok == 1;
        unsafe { (api.destroy_audio)(audio) };

        if written {
            Ok(())
        } else {
            Err("sherpa-onnx 写入 WAV 失败".to_string())
        }
    }
}

impl Drop for SherpaTts {
    fn drop(&mut self) {
        if !self.tts.is_null() {
            if let Ok(api) = api() {
                unsafe { (api.destroy)(self.tts) };
            }
            self.tts = ptr::null();
        }
    }
}

// SherpaTts 仅在 Mutex 保护下由播报线程访问，内部生命周期安全，可安全跨线程传递
unsafe impl Send for SherpaTts {}
unsafe impl Sync for SherpaTts {}

// 保留 CStr 引用以避免未使用警告（调试时可能用到）
#[allow(dead_code)]
fn _unused(_: &CStr) {}
