use crate::http_server::set_status_locked;
use crate::model::Store;
use crate::monitors::{ScreenConfig, ScreenInfo};
use eframe::egui;
use egui::{Color32, Context, Pos2, Vec2, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel};
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DISPLAY_VIEWPORT: &str = "school-display-screen";

/// 数字转中文（班级名格式化用，支持到 99）
fn cn_num(n: u32) -> String {
    let cn = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十"];
    if n <= 10 { return cn[n as usize].to_string(); }
    if n < 20 { return format!("十{}", cn[(n - 10) as usize]); }
    let tens = n / 10;
    let ones = n % 10;
    if ones == 0 { format!("{}十", cn[tens as usize]) }
    else { format!("{}十{}", cn[tens as usize], cn[ones as usize]) }
}

/// 从班级名解析 (年级, 班号)，兼容「1年1班」「1年级1班」
fn parse_class(name: &str) -> Option<(u32, u32)> {
    let year_pos = name.find('年')?;
    let grade = name[..year_pos].parse::<u32>().ok()?;
    let rest = &name[year_pos + '年'.len_utf8()..];
    let rest = rest.strip_prefix('级').unwrap_or(rest);
    let class_pos = rest.find('班')?;
    let num = rest[..class_pos].parse::<u32>().ok()?;
    Some((grade, num))
}

/// 按管理端 nameFmt 格式化班级名：cn=一（1），full=一年1班
fn format_class_name(name: &str, name_fmt: &str) -> String {
    match parse_class(name) {
        Some((grade, num)) => {
            let g = cn_num(grade);
            if name_fmt == "full" { format!("{}年{}班", g, num) }
            else { format!("{}（{}）", g, num) }
        }
        None => name.to_string(),
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab, _] = a.to_array();
    let [br, bg, bb, _] = b.to_array();
    Color32::from_rgb(
        (ar as f32 * (1.0 - t) + br as f32 * t) as u8,
        (ag as f32 * (1.0 - t) + bg as f32 * t) as u8,
        (ab as f32 * (1.0 - t) + bb as f32 * t) as u8,
    )
}
#[derive(Clone)]
struct GradientCfg {
    enabled: bool,
    targets_bg: bool,
    targets_text: bool,
    targets_name: bool,
    targets_time: bool,
    flow: bool,        // true=动画流动, false=静态
    radial: bool,      // true=径向, false=线性
    start: Color32,
    end: Color32,
    midpoint: f32,     // 0..1
    alpha: u8,         // 透明度 0..255
    reverse: bool,     // 方向反向（rtl / btt）
    vertical: bool,    // 上→纵向 / 下→横向
    pingpong: bool,    // 往返 vs 单向
    period: f32,       // 流动周期（秒）
    spread: f32,       // 渐变延展长度 1..5
}

impl GradientCfg {
    fn from_value(v: &Value, status: usize) -> Self {
        let sub = match v.get(&format!("s{}", status)) {
            Some(s) => s.clone(),
            None => v.clone(),
        };
        let strv = |k: &str| sub.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let numv = |k: &str, def: f64| sub.get(k).and_then(|x| if x.is_number(){x.as_f64()}else{x.as_str().and_then(|s|s.parse().ok())}).unwrap_or(def);
        let targets: Vec<String> = sub.get("targets").and_then(|t| t.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let direction = strv("direction");
        let mode = strv("mode");
        let kind = strv("kind");
        let loop_mode = strv("loop");
        Self {
            enabled: sub.get("enabled").and_then(|x| x.as_bool()).unwrap_or(status != 2),
            targets_bg: targets.iter().any(|t| t == "bg"),
            targets_text: targets.iter().any(|t| t == "text"),
            targets_name: targets.iter().any(|t| t == "name"),
            targets_time: targets.iter().any(|t| t == "time"),
            flow: mode != "static",
            radial: kind == "radial",
            start: hex_color(&strv("start"), Color32::WHITE),
            end: hex_color(&strv("end"), if status == 1 { Color32::from_rgb(255, 239, 170) } else { Color32::from_rgb(251, 191, 36) }),
            midpoint: (numv("midpoint", 50.0) / 100.0).clamp(0.0, 1.0) as f32,
            alpha: (numv("opacity", 100.0).clamp(0.0, 100.0) / 100.0 * 255.0) as u8,
            reverse: direction == "rtl" || direction == "btt",
            vertical: direction == "ttb" || direction == "btt",
            pingpong: loop_mode == "pingpong",
            period: numv("period", 1.5).max(0.1) as f32,
            spread: (numv("spread", 200.0) / 100.0).clamp(1.0, 5.0) as f32,
        }
    }
    fn color_at(&self, u: f32, phase: f32) -> Color32 {
        let mut u = u.clamp(0.0, 1.0);
        if self.reverse { u = 1.0 - u; }
        let g = if self.radial {
            let d = (u - 0.5).abs() * 2.0;
            if self.flow { ((d / self.spread - phase).fract() + 1.0).fract() } else { d }
        } else {
            if self.flow { ((u / self.spread - phase).fract() + 1.0).fract() } else { u }
        };
        let t = if self.midpoint <= 0.001 { 1.0 }
                else if g <= self.midpoint { g / self.midpoint }
                else { (1.0 - g) / (1.0 - self.midpoint).max(0.001) };
        let rgb = lerp_color(self.start, self.end, t);
        let [r, gg, b, _] = rgb.to_array();
        Color32::from_rgba_unmultiplied(r, gg, b, self.alpha)
    }
}
fn gradient_phase(cfg: &GradientCfg, time: f32) -> f32 {
    if !cfg.flow { return 0.0; }
    let t = time / cfg.period;
    if cfg.pingpong {
        let f = t.fract();
        if f < 0.5 { f * 2.0 } else { (1.0 - f) * 2.0 }
    } else {
        t.fract()
    }
}
fn draw_gradient_text(
    painter: &egui::Painter,
    center: Pos2,
    text: &str,
    font_size: f32,
    cfg: &GradientCfg,
    phase: f32,
) {
    use egui::text::{LayoutJob, TextFormat};

    let font = egui::FontId::proportional(font_size);
    if text.is_empty() { return; }

    let full_w = painter.layout(text.to_string(), font.clone(), Color32::WHITE, f32::INFINITY).rect.width();
    let mut job = LayoutJob::default();
    let mut acc = 0.0_f32;
    for c in text.chars() {
        let s = c.to_string();
        let w = painter.layout(s.clone(), font.clone(), Color32::WHITE, f32::INFINITY).rect.width();
        let ratio = if full_w > 0.0 { (acc + w / 2.0) / full_w } else { 0.0 };
        let color = cfg.color_at(ratio, phase);
        job.append(&s, 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
        acc += w;
    }
    let galley = painter.layout_job(job);
    let pos = Pos2::new(center.x - galley.rect.width() / 2.0, center.y - galley.rect.height() / 2.0);
    painter.galley(pos, galley, Color32::WHITE);
}
fn draw_gradient_bg(painter: &egui::Painter, rect: egui::Rect, radius: f32, cfg: &GradientCfg, phase: f32) {
    use egui::epaint::{Color32 as C32, Mesh, Shape, Vertex};
    let n = 12_u32;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        for j in 0..=n {
            let fx = i as f32 / n as f32;
            let fy = j as f32 / n as f32;
            let u = if cfg.radial {
                let dx = fx - 0.5; let dy = fy - 0.5;
                (dx * dx + dy * dy).sqrt() * 2.0_f32.sqrt()
            } else if cfg.vertical { fy } else { fx };
            let c = cfg.color_at(u, phase);
            let [r,g,b,a] = c.to_array();
            mesh.vertices.push(Vertex {
                pos: Pos2::new(rect.left() + rect.width() * fx, rect.top() + rect.height() * fy),
                uv: egui::Pos2::ZERO,
                color: C32::from_rgba_premultiplied(
                    (r as f32 * a as f32 / 255.0) as u8,
                    (g as f32 * a as f32 / 255.0) as u8,
                    (b as f32 * a as f32 / 255.0) as u8,
                    a,
                ),
            });
        }
    }
    let row = n + 1;
    for i in 0..n {
        for j in 0..n {
            let a = i * row + j;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            mesh.add_triangle(a, b, c);
            mesh.add_triangle(b, d, c);
        }
    }
    painter.add(Shape::rect_filled(rect, radius, Color32::BLACK));
    painter.add(Shape::Mesh(Arc::new(mesh)));
}
fn elapsed_mmss(update: &str) -> Option<String> {
    use chrono::{Local, NaiveDateTime};
    let t0 = NaiveDateTime::parse_from_str(update, "%Y-%m-%d %H:%M:%S").ok()?;
    let secs = (Local::now().naive_local() - t0).num_seconds();
    let secs = secs.max(0);
    Some(format!("{}:{:02}", secs / 60, secs % 60))
}

fn hex_color(s: &str, default: Color32) -> Color32 {
    if s.len() == 7 && s.starts_with('#') {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[1..3], 16),
            u8::from_str_radix(&s[3..5], 16),
            u8::from_str_radix(&s[5..7], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    }
    default
}

fn setup_chinese_font(ctx: &Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    let path = candidates.iter().find(|p| std::path::Path::new(p).exists());
    if let Some(path) = path {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("chinese".to_owned(), egui::FontData::from_owned(bytes).into());
            fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "chinese".to_owned());
            fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().push("chinese".to_owned());
            ctx.set_fonts(fonts);
        }
    }
}
/// 画顺时针跑马灯虚线边框：沿矩形周长按时间偏移绘制亮段
fn draw_voice_border(painter: &egui::Painter, rect: egui::Rect, time: f32, radius: f32) {
    use egui::Shape;
    let inset = 3.0;
    let r = rect.shrink(inset);
    let perim = 2.0 * (r.width() + r.height());
    let dash = 26.0;
    let gap = 18.0;
    let cycle = dash + gap;
    let n = (perim / cycle).ceil() as i32 + 1;
    let offset = (time * 120.0) % cycle; // 顺时针流动
    let bright = Color32::from_rgb(255, 240, 90);
    let dim = Color32::from_rgba_unmultiplied(255, 220, 80, 60);

    // 底框（暗虚线）
    for k in 0..n {
        let a = (k as f32 * cycle) % perim;
        let b = (a + dash).min(perim);
        let seg = rect_perim_segment(&r, a, b);
        painter.add(Shape::line(seg, egui::Stroke::new(3.0, dim)));
    }
    // 亮段随 offset 顺时针移动
    for k in 0..n {
        let a = (k as f32 * cycle + offset) % perim;
        let b = (a + dash).min(perim);
        if b <= a { continue; }
        let seg = rect_perim_segment(&r, a, b);
        painter.add(Shape::line(seg, egui::Stroke::new(4.0, bright)));
    }
    let _ = radius;
}

/// 把矩形周长上的 [start,end] 距离转换为折线点（顺时针：上→右→下→左）
fn rect_perim_segment(r: &egui::Rect, start: f32, end: f32) -> Vec<Pos2> {
    let w = r.width();
    let h = r.height();
    let top = r.top();
    let left = r.left();
    let point = |d: f32| -> Pos2 {
        let d = d.max(0.0);
        if d <= w { Pos2::new(left + d, top) }
        else if d <= w + h { Pos2::new(left + w, top + (d - w)) }
        else if d <= w * 2.0 + h { Pos2::new(left + w - (d - w - h), top + h) }
        else { Pos2::new(left, top + h - (d - w * 2.0 - h).min(h)) }
    };
    let mut pts = vec![point(start)];
    // 跨角时补上角点
    for corner in [w, w + h, w * 2.0 + h] {
        if start < corner && corner < end { pts.push(point(corner)); }
    }
    pts.push(point(end));
    pts
}

fn draw_display_grid(
    ui: &mut egui::Ui,
    classes: &[Value],
    display: &Value,
    status_labels: &[String],
    screens: &[ScreenInfo],
    screen_cfg: &Arc<Mutex<ScreenConfig>>,
    menu_state: &Arc<Mutex<bool>>,
    settings: &Arc<Mutex<SoftwareSettings>>,
    speaking: Option<&str>,
) {
    let cols = display["cols"].as_i64().unwrap_or(4).max(1) as usize;
    let adv = &display["advanced"];
    let name_fmt = display["nameFmt"].as_str().unwrap_or("cn");
    let order: Vec<String> = display["order"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut sorted: Vec<Option<&Value>> = Vec::new();
    if order.is_empty() {
        let mut cs: Vec<&Value> = classes.iter().collect();
        cs.sort_by_key(|c| c["class_id"].as_str().unwrap_or("").to_string());
        sorted.extend(cs.into_iter().map(Some));
    } else {
        for id in &order {
            if id.is_empty() {
                sorted.push(None);
            } else if let Some(c) = classes.iter().find(|c| c["class_id"].as_str() == Some(id)) {
                sorted.push(Some(c));
            }
        }
    }
    while sorted.len() % cols != 0 {
        sorted.push(None);
    }

    let radius = adv["radius"].as_f64().unwrap_or(10.0) as f32;
    let layout = adv["layout"].as_i64().unwrap_or(2).clamp(1, 3);
    let name_scale = adv["nameScale"].as_f64().unwrap_or(60.0) as f32;
    let status_scale = adv["statusScale"].as_f64().unwrap_or(28.0) as f32;
    let time_scale = adv["timeScale"].as_f64().unwrap_or(28.0) as f32;
    let s0_bg = hex_color(adv["s0Bg"].as_str().unwrap_or("#000000"), Color32::BLACK);
    let s0_text = hex_color(adv["s0Text"].as_str().unwrap_or("#ffffff"), Color32::WHITE);
    let s1_bg = hex_color(adv["s1Bg"].as_str().unwrap_or("#b4b400"), Color32::from_rgb(180, 180, 0));
    let s1_text = hex_color(adv["s1Text"].as_str().unwrap_or("#ffefaa"), Color32::from_rgb(255, 239, 170));
    let marquee = hex_color(adv["s1Marquee"].as_str().unwrap_or("#fbbf24"), Color32::YELLOW);
    let s2_bg = hex_color(adv["s2Bg"].as_str().unwrap_or("#22c55e"), Color32::from_rgb(34, 197, 94));
    let s2_text = hex_color(adv["s2Text"].as_str().unwrap_or("#0a3d1d"), Color32::from_rgb(10, 61, 29));
    let grads = [
        GradientCfg::from_value(&adv["gradient"], 0),
        GradientCfg::from_value(&adv["gradient"], 1),
        GradientCfg::from_value(&adv["gradient"], 2),
    ];
    if ui.input(|i| i.pointer.secondary_clicked()) {
        *menu_state.lock().unwrap() = true;
    }
    let canvas = ui.max_rect();
    let gap = 8.0_f32;
    let card_w = (canvas.width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let rows = (sorted.len() + cols - 1) / cols;
    let card_h = if rows > 0 { (canvas.height() - gap * (rows as f32 - 1.0)) / rows as f32 } else { 80.0 };
    let painter = ui.painter_at(canvas);
    let t = ui.input(|i| i.time) as f32;
    let phases = [gradient_phase(&grads[0], t), gradient_phase(&grads[1], t), gradient_phase(&grads[2], t)];

    for (idx, item) in sorted.iter().enumerate() {
        let r = idx / cols;
        let c = idx % cols;
        let x0 = canvas.left() + c as f32 * (card_w + gap);
        let y0 = canvas.top() + r as f32 * (card_h + gap);
        let rect = egui::Rect::from_min_size(egui::Pos2::new(x0, y0), Vec2::new(card_w, card_h));

        let cls = match item {
            Some(c) => c,
            None => { painter.rect_filled(rect, radius, egui::Color32::from_gray(20)); continue; }
        };
        let status = cls["status"].as_i64().unwrap_or(0);
        let (bg, fg) = match status {
            1 => (s1_bg, s1_text),
            2 => (s2_bg, s2_text),
            _ => (s0_bg, s0_text),
        };
        let name = format_class_name(cls["class_name"].as_str().unwrap_or(""), name_fmt);
        let label = status_labels.get(status.max(0).min(2) as usize)
            .cloned().unwrap_or_else(|| format!("状态{}", status.max(0).min(2) as usize + 1));
        let update = cls["update_time"].as_str().unwrap_or("");
        let grad = &grads[status.max(0).min(2) as usize];
        let phase = phases[status.max(0).min(2) as usize];
        if grad.enabled && grad.targets_bg { draw_gradient_bg(&painter, rect, radius, grad, phase); }
        else { painter.rect_filled(rect, radius, bg); }

        let center = rect.center();
        let counting = status == 1 || status == 2;
        let countdown = if counting { elapsed_mmss(update).unwrap_or_default() } else { String::new() };
        let target = if status == 1 { marquee } else { fg };
        let paint = |p: &egui::Painter, text: &str, y: f32, size: f32, color: Color32, grad_on: bool| {
            let pos = Pos2::new(center.x, center.y + y);
            if grad_on {
                draw_gradient_text(p, pos, text, size, &grad, phase);
            } else {
                p.text(pos, egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(size), color);
            }
        };
        let name_grad = grad.enabled && grad.targets_name;
        let text_grad = grad.enabled && grad.targets_text;
        let time_grad = grad.enabled && grad.targets_time;

        match layout {
            1 => {
                let line = if counting { format!("{} {} {}", name, label, countdown) }
                           else { format!("{} {}", name, label) };
                paint(&painter, &line, 0.0, name_scale.min(status_scale * 1.6), fg,
                    name_grad && text_grad && (!counting || time_grad));
            }
            2 => {
                paint(&painter, &name, -card_h * 0.22, name_scale, fg, name_grad);
                let line = if counting { format!("{} {}", label, countdown) } else { label.clone() };
                paint(&painter, &line, card_h * 0.20, status_scale, target,
                    text_grad && (!counting || time_grad));
            }
            _ => {
                paint(&painter, &name, -card_h * 0.28, name_scale, fg, name_grad);
                paint(&painter, &label, 0.0, status_scale, target, text_grad);
                if counting {
                    paint(&painter, &countdown, card_h * 0.30, time_scale, target, time_grad);
                }
            }
        }
        // 正在播报的班级：虚线跑马灯边框
        let cid = cls["class_id"].as_str().unwrap_or("");
        if speaking == Some(cid) {
            draw_voice_border(&painter, rect, t, radius);
        }
    }
    let menu_open = *menu_state.lock().unwrap();
    if menu_open {
        let mut close = false;
        egui::Window::new("切换大屏显示位置")
            .collapsible(false).resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label("选择大屏要显示在哪块屏幕：");
                for screen in screens {
                    if ui.button(&screen.name).clicked() {
                        screen_cfg.lock().unwrap().set_display_screen(screen.id.clone());
                        close = true;
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() { close = true; }
                    if ui.button("软件设置").clicked() {
                        let mut s = settings.lock().unwrap();
                        s.open = true;
                        s.on_main = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("退出软件").clicked() {
                            std::process::exit(0);
                        }
                    });
                });
            });
        if close { *menu_state.lock().unwrap() = false; }
    }
}

#[allow(dead_code)]
pub(crate) fn draw_display_grid_for_client(
    ui: &mut egui::Ui,
    classes: &[Value],
    display: &Value,
    status_labels: &[String],
    screens: &[ScreenInfo],
    speaking: Option<&str>,
) {
    let cfg = Arc::new(Mutex::new(ScreenConfig::load(std::path::PathBuf::new())));
    let menu = Arc::new(Mutex::new(false));
    let settings = Arc::new(Mutex::new(SoftwareSettings::default()));
    draw_display_grid(ui, classes, display, status_labels, screens, &cfg, &menu, &settings, speaking);
}

#[derive(Default)]
struct SoftwareSettings {
    open: bool,
    on_main: bool,
    ip: String,
    port: String,
    ips: Vec<String>,
    pwd: String,
    pwd_confirm: String,
    msg: String,
}

#[cfg(target_os = "windows")]
// shortcut: 用 ipconfig 解析 IPv4，Windows 专有且依赖系统语言含 "IPv4" 字样
pub(crate) fn local_ipv4s() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(out) = std::process::Command::new("ipconfig").output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if !line.contains("IPv4") { continue; }
            if let Some(token) = line.split_whitespace().last() {
                if let Ok(addr) = token.parse::<std::net::Ipv4Addr>() {
                    let s = addr.to_string();
                    if !addr.is_loopback() && !ips.contains(&s) { ips.push(s); }
                }
            }
        }
    }
    ips
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn local_ipv4s() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(out) = std::process::Command::new("ip")
        .args(["-4", "addr", "show", "scope", "global"]).output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let line = line.trim_start();
            if !line.starts_with("inet ") { continue; }
            if let Some(token) = line.split_whitespace().nth(1) {
                let addr = token.split('/').next().unwrap_or("");
                if let Ok(v) = addr.parse::<std::net::Ipv4Addr>() {
                    let s = v.to_string();
                    if !ips.contains(&s) { ips.push(s); }
                }
            }
        }
    }
    if ips.is_empty() {
        if let Ok(out) = std::process::Command::new("hostname").arg("-I").output() {
            for token in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                if let Ok(v) = token.parse::<std::net::Ipv4Addr>() { ips.push(v.to_string()); }
            }
        }
    }
    ips
}

pub(crate) fn is_private_ipv4(s: &str) -> bool {
    let b = match s.parse::<std::net::Ipv4Addr>() { Ok(v) => v.octets(), Err(_) => return false };
    b[0] == 10 || (b[0] == 172 && (16..=31).contains(&b[1])) || (b[0] == 192 && b[1] == 168)
}

fn render_software_settings(
    ctx: &Context,
    state: &Arc<Mutex<SoftwareSettings>>,
    store: &Arc<Mutex<Store>>,
) {
    let mut s = state.lock().unwrap();
    if !s.open { return; }
    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    let ips = s.ips.clone();
    let selected_ip = s.ip.clone();
    egui::Window::new("软件设置")
        .collapsible(false).resizable(false)
        .min_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("绑定 IP：");
            egui::ComboBox::from_id_salt("bind_ip")
                .selected_text(&selected_ip)
                .show_ui(ui, |ui| {
                    for ip in &ips {
                        ui.selectable_value(&mut s.ip, ip.clone(), ip);
                    }
                });
            ui.horizontal(|ui| {
                ui.label("监听端口：");
                ui.add(egui::TextEdit::singleline(&mut s.port).desired_width(120.0));
            });
            ui.separator();
            ui.label("重设管理密码（留空表示不修改）：");
            ui.horizontal(|ui| {
                ui.label("新密码：");
                ui.add(egui::TextEdit::singleline(&mut s.pwd).password(true).desired_width(220.0));
            });
            ui.horizontal(|ui| {
                ui.label("确认密码：");
                ui.add(egui::TextEdit::singleline(&mut s.pwd_confirm).password(true).desired_width(220.0));
            });
            if !s.msg.is_empty() { ui.colored_label(Color32::from_rgb(220, 120, 40), &s.msg); }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("保存").clicked() { save = true; }
                if ui.button("取消").clicked() { cancel = true; }
            });
        });
    if cancel { open = false; }
    s.open = open;
    if !save { return; }
    s.msg.clear();
    let port = match s.port.trim().parse::<u16>().ok().filter(|p| *p > 0) {
        Some(p) => p,
        None => { s.msg = "端口必须是 1-65535 的数字".into(); return; }
    };
    if !s.pwd.is_empty() || !s.pwd_confirm.is_empty() {
        if s.pwd.trim().is_empty() { s.msg = "密码不能为空".into(); return; }
        if s.pwd != s.pwd_confirm { s.msg = "两次输入的密码不一致".into(); return; }
    }
    {
        let mut db = store.lock().unwrap();
        db.data["bindIp"] = serde_json::json!(s.ip.clone());
        db.data["port"] = serde_json::json!(port);
        if !s.pwd.is_empty() {
            db.data["admin"]["password"] = serde_json::json!(s.pwd.clone());
        }
        db.persist();
    }
    s.pwd.clear();
    s.pwd_confirm.clear();
    s.msg = "已保存，重启软件后生效".into();
}

pub struct App {
    pub store: Arc<Mutex<Store>>,
    broadcast: Arc<crate::http_server::Broadcast>,
    pub screens: Vec<ScreenInfo>,
    pub     screen_cfg: Arc<Mutex<ScreenConfig>>,
    display_menu: Arc<Mutex<bool>>,
    display_applied: Arc<Mutex<String>>,
    window_icon: Arc<egui::IconData>,
    picker_open: bool,
    picked_display: String,
    picked_controller: String,
    pwd_setup_open: bool,
    pwd_input: String,
    pwd_confirm: String,
    pwd_err: String,
    port_err: String,
    port_state: Arc<Mutex<(u16, Option<String>)>>,
    port_input: String,
    server_ready: Arc<Mutex<bool>>,
    last_controller_id: String,
    status_message: String,
    software_settings: Arc<Mutex<SoftwareSettings>>,
    license_state: Arc<Mutex<crate::license::LicenseState>>,
    reg_open: bool,
    reg_msg: String,
    last_ontop: Option<bool>,
}

impl App {
    pub fn new(store: Arc<Mutex<Store>>, broadcast: Arc<crate::http_server::Broadcast>, screens: Vec<ScreenInfo>, screen_cfg: Arc<Mutex<ScreenConfig>>, port_state: Arc<Mutex<(u16, Option<String>)>>, server_ready: Arc<Mutex<bool>>, license_state: Arc<Mutex<crate::license::LicenseState>>) -> Self {
        let (d, c) = {
            let cfg = screen_cfg.lock().unwrap();
            (cfg.display_screen().unwrap_or("").to_string(), cfg.controller_screen().unwrap_or("").to_string())
        };
        // 首次启动（未设置管理密码）先弹密码设置，完成后再选屏幕
        let pwd_unset = {
            let db = store.lock().unwrap();
            db.data["admin"]["password"].as_str().unwrap_or("").is_empty()
        };
        let picker_open = !pwd_unset && (d.is_empty() || screens.len() < 2);
        let port_input = port_state.lock().unwrap().0.to_string();
        let mut ips = local_ipv4s();
        let saved_ip = {
            let db = store.lock().unwrap();
            db.data["bindIp"].as_str().unwrap_or("").to_string()
        };
        let ip = if saved_ip.is_empty() {
            ips.iter().find(|s| is_private_ipv4(s)).cloned().unwrap_or_else(|| "0.0.0.0".into())
        } else { saved_ip };
        if !ips.iter().any(|s| s == &ip) { ips.insert(0, ip.clone()); }
        if !ips.iter().any(|s| s == "0.0.0.0") { ips.push("0.0.0.0".into()); }
        let software_settings = Arc::new(Mutex::new(SoftwareSettings {
            open: false,
            on_main: false,
            ip,
            port: port_input.clone(),
            ips,
            pwd: String::new(),
            pwd_confirm: String::new(),
            msg: String::new(),
        }));
        Self {
            store, broadcast, screens, screen_cfg,
            display_menu: Arc::new(Mutex::new(false)),
            display_applied: Arc::new(Mutex::new(String::new())),
            window_icon: Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
                    .expect("窗口图标加载失败"),
            ),
            picker_open,
            picked_display: d.clone(),
            picked_controller: c,
            pwd_setup_open: pwd_unset,
            pwd_input: String::new(),
            pwd_confirm: String::new(),
            pwd_err: String::new(),
            port_err: String::new(),
            port_input,
            port_state,
            server_ready,
            last_controller_id: String::new(),
            status_message: String::new(),
            software_settings,
            license_state,
            reg_open: false,
            reg_msg: String::new(),
            last_ontop: None,
        }
    }

    fn set_status(&mut self, cid: &str, status: i32) {
        let mut db = self.store.lock().unwrap();
        if set_status_locked(&mut db, cid, status, "local", "") {
            self.status_message = format!("{} -> {}", cid, status);
        }
        drop(db);
        self.broadcast.send("update");
    }

    fn render_controller(&mut self, ctx: &Context) {
        let settings = self.software_settings.clone();
        let license = self.license_state.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("系统控制器");
                let (registered, expired, remaining) = {
                    let l = license.lock().unwrap();
                    (l.registered, l.expired, 300u64.saturating_sub(l.started.elapsed().as_secs()))
                };
                if registered {
                    ui.label("已注册");
                } else if expired {
                    ui.colored_label(Color32::from_rgb(220, 60, 60), "试用结束");
                } else {
                    ui.label(format!("试用剩余 {:02}:{:02}", remaining / 60, remaining % 60));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut local_display = {
                        let db = self.store.lock().unwrap();
                        db.data["localDisplayEnabled"].as_bool().unwrap_or(true)
                    };
                    if ui.checkbox(&mut local_display, "本地大屏").clicked() {
                        let mut db = self.store.lock().unwrap();
                        db.data["localDisplayEnabled"] = Value::Bool(local_display);
                        db.persist();
                    }
                    if ui.button("设置").clicked() {
                        let mut s = settings.lock().unwrap();
                        s.open = true;
                        s.on_main = true;
                    }
                    if !registered && !expired && ui.button("注册").clicked() {
                        self.reg_open = true;
                    }
                });
            });
            ui.separator();
            if !self.status_message.is_empty() { ui.label(&self.status_message); }
            ui.separator();

            let (classes, labels, name_fmt) = {
                let db = self.store.lock().unwrap();
                let labels: Vec<String> = db.status_labels().iter()
                    .map(|v| v.as_str().unwrap_or("").to_string()).collect();
                let name_fmt = db.display()["nameFmt"].as_str().unwrap_or("cn").to_string();
                let order: Vec<String> = db.display()["order"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let mut classes = db.classes().clone();
                // 按管理端排序排列；order 中未出现的班级按编号补到末尾
                if !order.is_empty() {
                    classes.sort_by_key(|c| {
                        let id = c["class_id"].as_str().unwrap_or("");
                        let pos = order.iter().position(|o| o == id);
                        (pos.unwrap_or(usize::MAX), id.to_string())
                    });
                }
                (classes, labels, name_fmt)
            };
            let names = |i: usize| {
                labels.get(i).map(|s| s.as_str()).filter(|s| !s.is_empty())
                    .unwrap_or(&format!("状态{}", i + 1)).to_string()
            };
            ui.label(format!("点击班级切换状态名-> {} -> {} -> {} -> {}", names(0), names(1), names(2), names(0)));
            let cols = 4;
            let gap = 10.0_f32;
            let avail_w = ui.available_width();
            let btn_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(120.0);
            let rows = (classes.len() + cols - 1) / cols;
            let avail_h = ui.available_height();
            let btn_h = if rows > 0 {
                ((avail_h - gap * (rows as f32 - 1.0)) / rows as f32).max(48.0)
            } else { 76.0 };
            let btn_size = Vec2::new(btn_w, btn_h);
            egui::Grid::new("controller_grid").num_columns(cols).spacing(Vec2::new(gap, gap)).show(ui, |ui| {
                for (i, c) in classes.iter().enumerate() {
                    let cid = c["class_id"].as_str().unwrap_or("");
                    let name = format_class_name(c["class_name"].as_str().unwrap_or(cid), &name_fmt);
                    let status = c["status"].as_i64().unwrap_or(0);
                    let idx = status.max(0).min(2) as usize;
                    let fallback = format!("状态{}", idx + 1);
                    let label = labels.get(idx).map(|s| s.as_str()).filter(|s| !s.is_empty()).unwrap_or(&fallback);
                    let color = match status {
                        1 => Color32::from_rgb(200, 138, 10),
                        2 => Color32::from_rgb(34, 160, 80),
                        _ => Color32::from_rgb(70, 78, 92),
                    };
                    let font_size = (btn_h * 0.28).clamp(15.0, 24.0);
                    let text = egui::RichText::new(format!("{}\n{}", name, label))
                        .color(Color32::WHITE)
                        .size(font_size)
                        .strong();
                    let btn = egui::Button::new(text).fill(color).min_size(btn_size);
                    if ui.add(btn).clicked() {
                        let next = if status == 2 { 0 } else { status + 1 };
                        self.set_status(cid, next as i32);
                    }
                    if (i + 1) % cols == 0 { ui.end_row(); }
                }
            });
        });
    }

    fn render_registration(&mut self, ctx: &Context) {
        if !self.reg_open { return; }
        let (machine_text, warn, expires, registered) = {
            let l = self.license_state.lock().unwrap();
            (l.machine.text(), l.warn.clone(), l.expires.clone(), l.registered)
        };
        let mut machine_text = machine_text;
        let message = self.reg_msg.clone();
        let mut recheck = false;
        let mut close = false;
        egui::Window::new("软件注册")
            .collapsible(false).resizable(false)
            .min_width(540.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("机器码（已写入 exe 目录 machine.txt，发给管理员生成 license.json）：");
                ui.add(egui::TextEdit::singleline(&mut machine_text).desired_width(500.0));
                if !expires.is_empty() { ui.label(format!("许可证到期：{}", expires)); }
                if registered { ui.label("状态：已注册"); }
                else { ui.label("状态：未注册，试用期 5 分钟"); }
                if !warn.is_empty() { ui.colored_label(Color32::from_rgb(220, 120, 40), &warn); }
                if !message.is_empty() { ui.colored_label(Color32::from_rgb(220, 60, 60), &message); }
                ui.separator();
                ui.label("把 license.json 放到 exe 目录后点“重新验证”。");
                ui.horizontal(|ui| {
                    if ui.button("重新验证").clicked() { recheck = true; }
                    if ui.button("关闭").clicked() { close = true; }
                });
            });
        if recheck {
            let base = std::env::current_exe()
                .ok().and_then(|p| p.parent().map(|x| x.to_path_buf()))
                .unwrap_or_default();
            let mut l = self.license_state.lock().unwrap();
            crate::license::reload(&base, &mut l);
            self.reg_msg = if l.registered { "注册成功".into() } else { l.warn.clone() };
        }
        if close { self.reg_open = false; }
    }

    fn render_trial_expired(&mut self, ctx: &Context) {
        egui::Window::new("试用结束")
            .collapsible(false).resizable(false)
            .min_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("试用结束");
                ui.label("5 分钟试用期已到。");
                ui.label("请完成注册，或关闭软件。");
                if ui.button("关闭软件").clicked() { std::process::exit(0); }
            });
    }

    fn render_port_error(&mut self, ctx: &Context) {
        let err = {
            let ps = self.port_state.lock().unwrap();
            ps.1.clone().unwrap_or_default()
        };
        if err.is_empty() { return; }
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("端口被占用")
            .collapsible(false).resizable(false)
            .min_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.colored_label(Color32::from_rgb(220, 120, 40), &err);
                ui.label("请更换一个未被占用的端口后重试。");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("新端口：");
                    ui.add(egui::TextEdit::singleline(&mut self.port_input).desired_width(120.0));
                });
                if !self.port_err.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 60, 60), &self.port_err);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("使用此端口").clicked() { apply = true; }
                    if ui.button("退出").clicked() { cancel = true; }
                });
            });
        if apply {
            self.port_err.clear();
            if let Ok(p) = self.port_input.trim().parse::<u16>() {
                if p > 0 {
                    {
                        let mut db = self.store.lock().unwrap();
                        db.data["port"] = serde_json::json!(p);
                        db.persist();
                    }
                    let mut ps = self.port_state.lock().unwrap();
                    ps.0 = p;
                    ps.1 = None; // 清除错误，触发重试
                    return;
                }
                self.port_err = "端口不能为 0".to_string();
            } else {
                self.port_err = "端口必须是 1-65535 的数字".to_string();
            }
        }
        if cancel {
            std::process::exit(0);
        }
    }

    fn render_pwd_setup(&mut self, ctx: &Context) {
        let mut done = false;
        egui::Window::new("首次启动设置")
            .collapsible(false).resizable(false)
            .min_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("请先设置管理密码和访问端口。");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("管理密码：");
                    ui.add(egui::TextEdit::singleline(&mut self.pwd_input).password(true).desired_width(220.0));
                });
                ui.horizontal(|ui| {
                    ui.label("确认密码：");
                    ui.add(egui::TextEdit::singleline(&mut self.pwd_confirm).password(true).desired_width(220.0));
                });
                ui.horizontal(|ui| {
                    ui.label("访问端口：");
                    ui.add(egui::TextEdit::singleline(&mut self.port_input).desired_width(120.0));
                });
                if !self.port_err.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 60, 60), &self.port_err);
                }
                if !self.pwd_err.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 60, 60), &self.pwd_err);
                }
                ui.separator();
                if ui.button("确定").clicked() {
                    self.port_err.clear();
                    self.pwd_err.clear();
                    if self.pwd_input.trim().is_empty() {
                        self.pwd_err = "密码不能为空".to_string();
                    } else if self.pwd_input != self.pwd_confirm {
                        self.pwd_err = "两次输入的密码不一致".to_string();
                    } else if let Ok(p) = self.port_input.trim().parse::<u16>() {
                        if p == 0 {
                            self.port_err = "端口不能为 0".to_string();
                        } else {
                            {
                                let mut db = self.store.lock().unwrap();
                                db.data["admin"]["password"] = serde_json::json!(self.pwd_input.clone());
                                db.data["port"] = serde_json::json!(p);
                                db.persist();
                            }
                            let mut ps = self.port_state.lock().unwrap();
                            ps.0 = p;
                            ps.1 = None;
                            *self.server_ready.lock().unwrap() = true;
                            self.pwd_err.clear();
                            done = true;
                        }
                    } else {
                        self.port_err = "端口必须是 1-65535 的数字".to_string();
                    }
                }
            });
        if done {
            self.pwd_setup_open = false;
            // 密码设置完成后弹出屏幕选择
            self.picker_open = true;
        }
    }

    fn render_picker(&mut self, ctx: &Context) {
        let mut open = self.picker_open;
        let mut confirm = false;
        egui::Window::new("选择显示屏幕")
            .collapsible(false).resizable(false)
            .min_width(360.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("大屏显示屏幕：");
                for s in &self.screens { ui.radio_value(&mut self.picked_display, s.id.clone(), &s.name); }
                ui.separator();
                ui.label("控制器显示屏幕：");
                for s in &self.screens { ui.radio_value(&mut self.picked_controller, s.id.clone(), &s.name); }
                ui.separator();
                if ui.button("确定").clicked() && !self.picked_display.is_empty() {
                    let mut cfg = self.screen_cfg.lock().unwrap();
                    cfg.set_display_screen(self.picked_display.clone());
                    if !self.picked_controller.is_empty() {
                        cfg.set_controller_screen(self.picked_controller.clone());
                    }
                    confirm = true;
                }
            });
        if confirm { open = false; }
        self.picker_open = open;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        setup_chinese_font_once(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let trial_expired = {
            let mut l = self.license_state.lock().unwrap();
            if !l.registered && !l.expired && l.started.elapsed() >= Duration::from_secs(300) {
                l.expired = true;
                crate::http_server::TRIAL_EXPIRED.store(true, Ordering::Relaxed);
            }
            l.expired
        };
        // 单屏或两个窗口在同一屏幕时，控制器置顶；用户可自行最小化到任务栏
        let same_screen = {
            let cfg = self.screen_cfg.lock().unwrap();
            let display = cfg.display_screen().unwrap_or("");
            let controller = cfg.controller_screen().unwrap_or("");
            self.screens.len() < 2 || display.is_empty() || controller.is_empty() || display == controller
        };
        if self.last_ontop != Some(same_screen) {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
                if same_screen { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal },
            ));
            self.last_ontop = Some(same_screen);
        }
        if trial_expired {
            self.render_trial_expired(ctx);
            return;
        }
        self.render_controller(ctx);
        self.render_registration(ctx);
        let show_settings_main = {
            let s = self.software_settings.lock().unwrap();
            s.open && s.on_main
        };
        if show_settings_main {
            render_software_settings(ctx, &self.software_settings, &self.store);
        }
        // 端口被占用最优先处理；否则首次启动先设密码，再选屏幕
        if self.port_state.lock().unwrap().1.is_some() {
            self.render_port_error(ctx);
        } else if self.pwd_setup_open { self.render_pwd_setup(ctx); }
        else if self.picker_open { self.render_picker(ctx); }
        let controller_id = self.screen_cfg.lock().unwrap().controller_screen().unwrap_or("").to_string();
        if controller_id != self.last_controller_id {
            if let Some(s) = self.screens.iter().find(|s| s.id == controller_id) {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(s.x as f32, s.y as f32)));
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            }
            self.last_controller_id = controller_id;
        }
        // 端口错误/设置密码/选屏阶段不创建大屏窗口，避免全屏大屏遮挡控制器弹窗
        let port_ok = self.port_state.lock().unwrap().1.is_none();
        let local_display = self.store.lock().unwrap().data["localDisplayEnabled"].as_bool().unwrap_or(true);
        if port_ok && !self.picker_open && !self.pwd_setup_open && local_display {
            let store = self.store.clone();
            let screens = self.screens.clone();
            let cfg = self.screen_cfg.clone();
            let menu = self.display_menu.clone();
            let applied = self.display_applied.clone();
            let icon = self.window_icon.clone();
            let settings = self.software_settings.clone();
            ctx.show_viewport_deferred(
                ViewportId::from_hash_of(DISPLAY_VIEWPORT),
                ViewportBuilder::default().with_title("放学系统 · 大屏")
                    .with_inner_size([800.0, 600.0]).with_icon(icon),
                move |ctx, _class| {
                    ctx.request_repaint_after(std::time::Duration::from_millis(33));
                    let cfg_guard = cfg.lock().unwrap();
                    if let Some(sid) = cfg_guard.display_screen() {
                        if let Some(s) = screens.iter().find(|s| &s.id == sid) {
                            let sig = format!("{}|{}x{}", sid, s.width, s.height);
                            let need = applied.lock().unwrap().as_str() != sig;
                            if need {
                                {
                                    let mut db = store.lock().unwrap();
                                    db.display_mut()["width"] = serde_json::json!(s.width as i64);
                                    db.display_mut()["height"] = serde_json::json!(s.height as i64);
                                    db.persist();
                                }
                                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(Pos2::new(s.x as f32, s.y as f32)));
                                ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(s.width as f32, s.height as f32)));
                                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
                                *applied.lock().unwrap() = sig;
                            }
                        }
                    }
                    drop(cfg_guard);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let db = store.lock().unwrap();
                        let labels: Vec<String> = db.status_labels().iter()
                            .map(|v| v.as_str().unwrap_or("").to_string()).collect();
                        let speaking = crate::voice::engine()
                            .and_then(|e| e.speaking.lock().ok())
                            .and_then(|g| g.clone());
                        draw_display_grid(ui, db.classes(), db.display(), &labels, &screens, &cfg, &menu, &settings, speaking.as_deref());
                    });
                    let show_settings_display = {
                        let s = settings.lock().unwrap();
                        s.open && !s.on_main
                    };
                    if show_settings_display {
                        render_software_settings(ctx, &settings, &store);
                    }
                },
            );
        } else if !local_display {
            ctx.send_viewport_cmd_to(
                ViewportId::from_hash_of(DISPLAY_VIEWPORT),
                ViewportCommand::Close,
            );
        }
    }
}

thread_local! {
    static FONT_SET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(crate) fn setup_chinese_font_once(ctx: &Context) {
    if !FONT_SET.get() { setup_chinese_font(ctx); FONT_SET.set(true); }
}
