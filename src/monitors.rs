use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenInfo {
    /// 稳定的设备标识（设备名），用于持久化记忆
    pub id: String,
    /// 显示名称（给用户看）
    pub name: String,
    /// 物理像素位置与尺寸
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Config {
    display_screen: Option<String>,
    controller_screen: Option<String>,
}

pub struct ScreenConfig {
    pub path: PathBuf,
    cfg: Config,
}

impl ScreenConfig {
    pub fn load(path: PathBuf) -> Self {
        let cfg = fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim_start_matches('\u{feff}').to_string())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, cfg }
    }

    pub fn display_screen(&self) -> Option<&str> {
        self.cfg.display_screen.as_deref()
    }

    pub fn controller_screen(&self) -> Option<&str> {
        self.cfg.controller_screen.as_deref()
    }

    pub fn set_display_screen(&mut self, id: String) {
        self.cfg.display_screen = Some(id);
        self.save();
    }

    pub fn set_controller_screen(&mut self, id: String) {
        self.cfg.controller_screen = Some(id);
        self.save();
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.cfg) {
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, s).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn enumerate() -> Vec<ScreenInfo> {
    use std::mem;
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, MONITORINFO};

    const MONITORINFOF_PRIMARY: u32 = 1;

    struct MonitorRaw { rect: RECT, primary: bool }

    let mut monitors: Vec<MonitorRaw> = Vec::new();

    unsafe extern "system" fn cb(
        hmon: windows::Win32::Graphics::Gdi::HMONITOR,
        _hdc: windows::Win32::Graphics::Gdi::HDC,
        _rc: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<MonitorRaw>);
        let mut mi = MONITORINFO {
            cbSize: mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
            list.push(MonitorRaw {
                rect: mi.rcMonitor,
                primary: (mi.dwFlags & MONITORINFOF_PRIMARY) != 0,
            });
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut monitors as *mut _ as isize));
    }

    monitors.into_iter().enumerate().map(|(i, m)| {
        let w = (m.rect.right - m.rect.left) as u32;
        let h = (m.rect.bottom - m.rect.top) as u32;
        // 用位置+尺寸作为稳定 ID（显示器插拔不变）
        let id = format!("{}x{}@{},{}", w, h, m.rect.left, m.rect.top);
        let name = format!("屏幕{}  {}×{}{}", i + 1, w, h,
                           if m.primary { " (主屏)" } else { "" });
        ScreenInfo {
            id, name,
            x: m.rect.left, y: m.rect.top,
            width: w, height: h,
            is_primary: m.primary,
        }
    }).collect()
}

#[cfg(not(target_os = "windows"))]
pub fn enumerate() -> Vec<ScreenInfo> {
    Vec::new()
}
