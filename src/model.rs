use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Linux deb/rpm 安装到 /opt，普通用户不可写；此时把状态文件放到用户配置目录。
pub fn state_dir(base: &Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if base.starts_with("/opt/") {
            let root = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .unwrap_or_else(|| PathBuf::from("."));
            let name = base.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "LeaveBell".into());
            let dir = root.join("LeaveBell").join(name);
            let _ = fs::create_dir_all(&dir);
            return dir;
        }
    }
    base.to_path_buf()
}

pub fn default_status_labels() -> Value {
    json!(["上课", "出教室", "抵达校门"])
}

/// 语音播报默认设置（sherpa-onnx 离线 TTS / EdgeTTS 在线）
pub fn default_voice() -> Value {
    json!({
        "enabled": true,
        "engine": "edge",
        "voice": "zh-CN-XiaoxiaoNeural",
        "speed": 1.0,
        "volume": 1.0
    })
}

/// 渐变效果默认配置。effect targets: bg(底色)/text(文字)/name(班级标签)/time(倒计时)
pub fn default_gradient() -> Value {
    default_display()["advanced"]["gradient"].clone()
}

/// 把用户提交的单个状态渐变配置归一化：补默认值、校验取值范围。
fn normalize_gradient_one(v: &Value, base: &Value) -> Value {
    let mut out = base.clone();
    if !v.is_object() { return out; }
    let allowed = ["bg", "text", "name", "time"];
    let targets: Vec<Value> = v.get("targets").and_then(|t| t.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str())
            .filter(|s| allowed.contains(s)).map(|s| json!(s)).collect())
        .unwrap_or_else(|| out["targets"].as_array().unwrap().clone());
    out["targets"] = json!(targets);
    let s = |key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("");
    { let x = s("mode"); if x == "static" || x == "flow" { out["mode"] = json!(x); } }
    { let x = s("kind"); if x == "linear" || x == "radial" { out["kind"] = json!(x); } }
    { let x = s("direction"); if ["ltr","rtl","ttb","btt"].contains(&x) { out["direction"] = json!(x); } }
    { let x = s("loop"); if x == "loop" || x == "pingpong" { out["loop"] = json!(x); } }
    for key in ["start", "end"] {
        let c = s(key);
        if c.starts_with('#') && c.len() == 7 { out[key] = json!(c); }
    }
    if let Some(b) = v.get("enabled").and_then(|x| x.as_bool()) { out["enabled"] = json!(b); }
    let num = |key: &str, lo: f64, hi: f64| -> Option<f64> {
        v.get(key).and_then(|x| if x.is_number() { x.as_f64() } else { x.as_str().and_then(|s| s.parse().ok()) })
            .map(|n| n.clamp(lo, hi))
    };
    for (key, lo, hi) in [("midpoint",0.0,100.0), ("opacity",0.0,100.0), ("period",0.1,60.0), ("spread",100.0,500.0)] {
        if let Some(n) = num(key, lo, hi) { out[key] = json!(n); }
    }
    out
}

/// 归一化整组渐变配置（按状态）。
/// 兼容旧版扁平结构：旧数据里 gradient 直接就是单个状态配置，此时平铺到三个状态。
pub fn normalize_gradient(v: &Value) -> Value {
    let mut out = default_gradient();
    if !v.is_object() { return out; }
    // 旧版扁平结构：包含 mode/targets 等字段，视为「所有状态共用」
    let is_flat = v.get("mode").is_some() || v.get("targets").is_some() || v.get("enabled").is_some();
    if is_flat {
        let one = normalize_gradient_one(v, &default_gradient_one());
        out["s0"] = one.clone();
        out["s1"] = one.clone();
        out["s2"] = one;
        return out;
    }
    for key in ["s0", "s1", "s2"] {
        if let Some(sub) = v.get(key) {
            let base = out[key].clone();
            out[key] = normalize_gradient_one(sub, &base);
        }
    }
    out
}

pub struct Store {
    pub data: Value,
    pub path: PathBuf,
}

/// 班级排序预设内容（按年级顺序排列的班级编号）
pub fn default_order() -> Value {
    json!([
        "101","102","103","104","105","106","107",
        "201","202","203","204","205","206","207",
        "301","302","303","304","305","306","307",
        "401","402","403","404","405","406","407",
        "501","502","503","504","505","506","507",
        "601","602","603","604","605","606","607"
    ])
}

fn default_display() -> Value {
    let mut display = json!({
        "width": 1920,
        "height": 1080,
        "cols": 6,
        "order": [],
        "nameFmt": "cn",
        "cardW": 0,
        "cardH": 0,
        "advanced": {
            "layout": 2, "radius": 10, "borderAlpha": 0.26, "padding": 0.9, "gap": 1,
            "nameScale": 60, "statusScale": 28, "timeScale": 28,
            "nameWeight": 800, "statusWeight": 700, "timeWeight": 600, "fontFamily": "",
            "s0Bg": "#000000", "s0Text": "#ffffff", "s1Bg": "#b4b400", "s1Text": "#ffefaa",
            "s1Marquee": "#fbbf24", "s2Bg": "#22c55e", "s2Text": "#0a3d1d",
            // 渐变效果按状态分别配置：s0=上课, s1=出教室, s2=抵达校门
            "gradient": {
                "s0": default_gradient_one(),
                "s1": default_gradient_one(),
                "s2": default_gradient_one(),
            }
        }
    });
    // 状态2渐变更亮；状态3默认关闭渐变（与预置数据一致）
    display["advanced"]["gradient"]["s1"]["end"] = json!("#ffefaa");
    display["advanced"]["gradient"]["s2"]["enabled"] = json!(false);
    display
}

/// 单个状态的渐变默认配置（三种状态可各自调整）
pub fn default_gradient_one() -> Value {
    json!({
        "enabled": true,
        "targets": ["text", "time"],
        "mode": "flow",
        "kind": "linear",
        "start": "#ffffff",
        "end": "#fbbf24",
        "midpoint": 50,
        "opacity": 100,
        "direction": "ltr",
        "loop": "pingpong",
        "period": 1.5,
        "spread": 200
    })
}

fn default_schedule() -> Value {
    let mut timers = Vec::new();
    let labels = ["定时器1", "定时器2", "定时器3"];
    let hours = [8, 14, 21];
    for i in 0..3 {
        timers.push(json!({
            "id": i, "label": labels[i], "enabled": i < 2, "hour": hours[i], "minute": 0,
            "lastRunDate": "", "lastRunTime": "", "lastStatus": "idle", "lastMsg": "", "count": 0
        }));
    }
    json!({ "timers": timers })
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let data: Value = match fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => {
                    let _ = fs::copy(&path, path.with_extension("json.bak"));
                    eprintln!("data.json 解析失败，已备份为 .json.bak，将以默认数据启动");
                    Value::Null
                }
            },
            Err(_) => Value::Null,
        };

        let mut data = if data.is_object() { data } else { json!({}) };

        // 补齐缺失字段（兼容现有 data.json）
        if !data.get("classes").is_some_and(|v| v.is_array()) {
            data["classes"] = json!([]);
        }
        if data["classes"].as_array().unwrap().is_empty() {
            let mut classes = Vec::new();
            for grade in 1..=6 {
                for i in 1..=7 {
                    let cid = format!("{}{:02}", grade, i);
                    classes.push(json!({
                        "class_id": cid,
                        "class_name": format!("{}年{}班", grade, i),
                        "grade": grade,
                        "status": 0,
                        "update_time": Value::Null
                    }));
                }
            }
            data["classes"] = json!(classes);
        }
        if !data.get("teachers").is_some_and(|v| v.is_array()) {
            data["teachers"] = json!([{"phone": "admin", "class_id": "admin", "name": "管理员"}]);
        }
        if !data.get("logs").is_some_and(|v| v.is_array()) {
            data["logs"] = json!([]);
        }
        if data.get("nextLogId").is_none() {
            data["nextLogId"] = json!(1);
        }
        if data.get("admin").is_none() {
            // 首次启动不设默认密码，留空交由原生窗口引导用户设置
            data["admin"] = json!({ "password": "" });
        }
        if data.get("port").is_none() {
            let port = if cfg!(target_os = "linux") { 8080 } else { 80 };
            data["port"] = json!(port);
        }
        if data.get("bindIp").is_none() {
            data["bindIp"] = json!("");
        }
        // 状态名称：默认 上课/出教室/抵达校门
        {
            let mut labels = default_status_labels();
            if let Some(saved) = data.get("statusLabels").and_then(|v| v.as_array()) {
                for i in 0..3 {
                    if let Some(s) = saved.get(i).and_then(|v| v.as_str()) {
                        if !s.is_empty() { labels[i] = json!(s); }
                    }
                }
            }
            data["statusLabels"] = labels;
        }

        // 显示设置：默认值 + 已保存值合并
        let saved_display = data.get("display").cloned().unwrap_or(Value::Null);
        let mut display = default_display();
        merge_objects(&mut display, &saved_display);
        // 班级排序为空时套用预设顺序，避免大屏出现空白占位
        let order_empty = display.get("order").and_then(|o| o.as_array()).map(|a| a.is_empty()).unwrap_or(true);
        if order_empty { display["order"] = default_order(); }
        data["display"] = display;

        // 定时任务
        let saved_schedule = data.get("schedule").cloned().unwrap_or(Value::Null);
        let mut schedule = if saved_schedule.is_object() { saved_schedule } else { default_schedule() };
        // 兼容旧数据：给缺少 minute 的定时器补 0
        if let Some(timers) = schedule["timers"].as_array_mut() {
            for t in timers.iter_mut() {
                if t.get("minute").is_none() { t["minute"] = json!(0); }
            }
        }
        data["schedule"] = schedule;

        // 扫码凭证：扫码操作的身份令牌，固定不变，持久化保存
        if data.get("scanToken").and_then(|v| v.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
            data["scanToken"] = json!(gen_scan_token());
        }
        if data.get("scanServer").and_then(|v| v.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
            data["scanServer"] = json!("");
        }
        // 语音播报设置：默认值 + 已保存值合并
        {
            let mut voice = default_voice();
            if let Some(saved) = data.get("voice") {
                merge_objects(&mut voice, saved);
            }
            data["voice"] = voice;
        }

        let store = Self { data, path };
        store.persist();
        store
    }

    pub fn persist(&self) {
        if let Err(e) = self.persist_checked() {
            eprintln!("保存 data.json 失败: {}", e);
        }
    }

    pub fn persist_checked(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        // 先写临时文件再原子替换，避免写入中断损坏 data.json
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, s).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn classes(&self) -> &Vec<Value> {
        self.data["classes"].as_array().unwrap()
    }

    pub fn classes_mut(&mut self) -> &mut Vec<Value> {
        self.data["classes"].as_array_mut().unwrap()
    }

    pub fn find_class(&self, cid: &str) -> Option<&Value> {
        self.classes().iter().find(|c| c["class_id"].as_str() == Some(cid))
    }

    pub fn find_class_mut(&mut self, cid: &str) -> Option<&mut Value> {
        self.classes_mut().iter_mut().find(|c| c["class_id"].as_str() == Some(cid))
    }

    pub fn display(&self) -> &Value {
        &self.data["display"]
    }

    pub fn display_mut(&mut self) -> &mut Value {
        &mut self.data["display"]
    }

    pub fn status_labels(&self) -> &Vec<Value> {
        self.data["statusLabels"].as_array().unwrap()
    }
}

fn merge_objects(base: &mut Value, patch: &Value) {
    if let (Some(b), Some(p)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            if let Some(bv) = b.get_mut(k) {
                if bv.is_object() && v.is_object() {
                    merge_objects(bv, v);
                } else {
                    *bv = v.clone();
                }
            } else {
                b.insert(k.clone(), v.clone());
            }
        }
    }
}

pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 生成扫码操作凭证（64 位随机十六进制字符串，内网离线可用）
pub fn gen_scan_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
