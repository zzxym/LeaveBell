use crate::model::{now_str, Store};
use chrono::Timelike;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub static TRIAL_EXPIRED: AtomicBool = AtomicBool::new(false);

pub struct Broadcast {
    inner: RwLock<Vec<Sender<String>>>,
}

impl Broadcast {
    pub fn new() -> Self {
        Self { inner: RwLock::new(Vec::new()) }
    }
    pub fn subscribe(&self) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.inner.write().unwrap().push(tx);
        rx
    }
    pub fn send(&self, event: &str) {
        self.send_data(event, "{}");
    }

    pub fn send_data(&self, event: &str, data: &str) {
        let msg = format!("event: {}\ndata: {}\n\n", event, data);
        let mut dead = Vec::new();
        for (i, tx) in self.inner.read().unwrap().iter().enumerate() {
            if tx.send(msg.clone()).is_err() {
                dead.push(i);
            }
        }
        if !dead.is_empty() {
            let mut list = self.inner.write().unwrap();
            for i in dead.into_iter().rev() {
                list.remove(i);
            }
        }
    }
}

#[derive(Clone)]
pub struct WebDir(pub PathBuf);

type Shared = (Arc<Mutex<Store>>, Arc<Broadcast>, WebDir, Arc<Mutex<HashMap<String, String>>>);

pub fn start_server(
    bind_ip: &str,
    port: u16,
    store: Arc<Mutex<Store>>,
    broadcast: Arc<Broadcast>,
    web_dir: PathBuf,
) -> Result<(), String> {
    let server = Arc::new(
        Server::http(format!("{}:{}", bind_ip, port))
            .map_err(|e| format!("无法监听 {}:{}: {}", bind_ip, port, e))?
    );
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let shared: Shared = (store.clone(), broadcast.clone(), WebDir(web_dir), sessions);

    // 定时调度线程：每分钟检查一次到点的定时器
    {
        let store = store.clone();
        let broadcast = broadcast.clone();
        std::thread::spawn(move || {
            let mut last_minute = String::new();
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let now = chrono::Local::now();
                let minute_key = now.format("%Y-%m-%d %H:%M").to_string();
                if minute_key == last_minute { continue; }
                last_minute = minute_key;
                let h = now.hour() as u64;
                let m = now.minute() as u64;
                let today = now.format("%Y-%m-%d").to_string();
                let due: Vec<u64> = {
                    let db = store.lock().unwrap();
                    db.data["schedule"]["timers"].as_array()
                        .map(|a| a.iter()
                            .filter(|t| t["enabled"].as_bool().unwrap_or(false)
                                && t["hour"].as_u64() == Some(h)
                                && t["minute"].as_u64().unwrap_or(0) == m
                                && t["lastRunDate"].as_str() != Some(today.as_str()))
                            .filter_map(|t| t["id"].as_u64())
                            .collect())
                        .unwrap_or_default()
                };
                for tid in due {
                    let mut db = store.lock().unwrap();
                    run_timer_locked(&mut db, tid);
                    drop(db);
                    broadcast.send("update");
                }
            }
        });
    }

    println!("  后端已启动: http://{}:{}", if bind_ip == "0.0.0.0" { "127.0.0.1" } else { bind_ip }, port);
    for req in server.incoming_requests() {
        let s = shared.clone();
        std::thread::spawn(move || {
            let _ = handle(req, s);
        });
    }
    Ok(())
}

fn json_response(req: Request, obj: Value, code: u16) {
    let body = serde_json::to_vec(&obj).unwrap_or_default();
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap();
    let cl = Header::from_bytes(&b"Content-Length"[..], body.len().to_string().as_bytes()).unwrap();
    let _ = req.respond(
        Response::from_data(body)
            .with_status_code(StatusCode(code))
            .with_header(ct)
            .with_header(cl),
    );
}

fn not_found(req: Request) {
    json_response(req, json!({"ok": false, "msg": "Not Found"}), 404);
}

fn read_body(req: &mut Request) -> Option<Value> {
    let mut buf = Vec::new();
    if req.as_reader().read_to_end(&mut buf).is_err() {
        return None;
    }
    if buf.is_empty() {
        return Some(json!({}));
    }
    serde_json::from_slice(&buf).ok()
}

fn valid_session(sessions: &HashMap<String, String>, token: &str) -> bool {
    !token.is_empty() && sessions.get(token).map(|v| v == "admin").unwrap_or(false)
}

fn session_phone(sessions: &HashMap<String, String>, token: &str) -> Option<String> {
    sessions.get(token).cloned()
}

fn num(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if v.is_number() {
        v.as_f64()
    } else if let Some(s) = v.as_str() {
        s.parse().ok()
    } else {
        None
    }
}

fn handle(req: Request, shared: Shared) -> Result<(), ()> {
    if TRIAL_EXPIRED.load(Ordering::Relaxed) {
        let _ = req.respond(
            Response::from_string("试用结束，软件已停止服务")
                .with_status_code(StatusCode(503)),
        );
        return Ok(());
    }
    let (store, broadcast, webdir, sessions) = shared;
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();

    if path == "/api/events" {
        return sse(req, broadcast);
    }

    if method == Method::Get {
        handle_get(req, &path, store, webdir, sessions)
    } else if method == Method::Post {
        handle_post(req, &path, store, broadcast, sessions)
    } else {
        not_found(req);
        Ok(())
    }
}

fn handle_get(
    req: Request,
    path: &str,
    store: Arc<Mutex<Store>>,
    webdir: WebDir,
    sessions: Arc<Mutex<HashMap<String, String>>>,
) -> Result<(), ()> {
    match path {
        "/api/ui-version" => {
            // 返回静态资源修改时间，供页面检测更新后自动刷新（绕过顽固缓存）
            let ver = |name: &str| -> u64 {
                fs::metadata(webdir.0.join(name))
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            };
            json_response(req, json!({
                "ok": true,
                "data": {
                    "index": ver("index.html"),
                    "style": ver("style.css"),
                    "css": ver("display.css")
                }
            }), 200);
        }
        "/api/classes" => {
            let db = store.lock().unwrap();
            json_response(req, json!({"ok": true, "data": db.classes()}), 200);
        }
        "/api/display" => {
            let db = store.lock().unwrap();
            let mut data = db.display().clone();
            data["statusLabels"] = db.data["statusLabels"].clone();
            json_response(req, json!({"ok": true, "data": data}), 200);
        }
        "/api/status" => {
            let db = store.lock().unwrap();
            let total = db.classes().len();
            let done = db.classes().iter().filter(|c| c["status"] == 2).count();
            json_response(req, json!({"ok": true, "data": {"total": total, "done": done, "remaining": total - done}}), 200);
        }
        "/api/admin/status" | "/api/admin/teachers" | "/api/admin/schedule" => {
            let token = extract_query(req.url(), "token");
            let ses = sessions.lock().unwrap();
            if !valid_session(&ses, &token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401);
                return Ok(());
            }
            drop(ses);
            let db = store.lock().unwrap();
            let data = match path {
                "/api/admin/status" => {
                    let total = db.classes().len();
                    let done = db.classes().iter().filter(|c| c["status"] == 2).count();
                    json!({"total": total, "done": done, "remaining": total - done})
                }
                "/api/admin/teachers" => {
                    let teachers: Vec<&Value> = db.data["teachers"].as_array().unwrap()
                        .iter().filter(|t| t["class_id"].as_str() != Some("admin")).collect();
                    json!(teachers)
                }
                _ => db.data["schedule"].clone(),
            };
            json_response(req, json!({"ok": true, "data": data}), 200);
        }
        "/api/login" => {
            let phone = extract_query(req.url(), "phone");
            let mut class_id = extract_query(req.url(), "class_id");
            let db = store.lock().unwrap();
            let teachers = db.data["teachers"].as_array().cloned().unwrap_or_default();
            // 管理员账号（手机号为 admin）仍返回 admin 角色
            if teachers.iter().any(|t| t["phone"].as_str() == Some(&phone)
                && t["class_id"].as_str() == Some("admin")) {
                let token = gen_token();
                sessions.lock().unwrap().insert(token.clone(), "guest-admin".to_string());
                json_response(req, json!({"ok": true, "role": "admin", "phone": phone, "token": token}), 200);
                return Ok(());
            }
            // 该手机号当前已绑定的班级（忽略已不存在的班级）
            let bound: Vec<String> = teachers.iter()
                .filter(|t| t["phone"].as_str() == Some(&phone))
                .filter_map(|t| t["class_id"].as_str().map(String::from))
                .filter(|c| db.find_class(c).is_some())
                .collect();

            if bound.is_empty() {
                json_response(req, json!({"ok": false, "msg": "账号未绑定班级，请联系管理员"}), 200);
                return Ok(());
            }
            if class_id.is_empty() {
                class_id = bound[0].clone();
            }
            // 只能进入管理员预设绑定的班级，不再自动注册
            if !bound.iter().any(|c| c == &class_id) {
                json_response(req, json!({"ok": false, "msg": "该班级未绑定到此账号，请联系管理员"}), 200);
                return Ok(());
            }
            let class = db.find_class(&class_id).cloned().unwrap_or(Value::Null);
            let classes: Vec<Value> = bound.iter()
                .filter_map(|c| db.find_class(c).cloned()).collect();
            let token = gen_token();
            sessions.lock().unwrap().insert(token.clone(), phone.clone());
            json_response(req, json!({
                "ok": true, "role": "teacher", "phone": phone, "token": token,
                "class": class, "classes": classes,
                "statusLabels": db.data["statusLabels"].clone()
            }), 200);
            }
            "/api/voice" => {
            let db = store.lock().unwrap();
            let query_eng = extract_query(req.url(), "engine");
            let eng = if query_eng == "sherpa" || query_eng == "edge" {
                query_eng
            } else {
                db.data["voice"]["engine"].as_str().unwrap_or("edge").to_string()
            };
            let voices = crate::voice::engine().map(|e| e.list_voices(&eng)).unwrap_or_default();
            json_response(req, json!({
                "ok": true,
                "data": {
                    "settings": db.data["voice"].clone(),
                    "voices": voices
                }
            }), 200);
        }
        "/api/scan/info" => {
                let db = store.lock().unwrap();
                json_response(req, json!({
                    "ok": true,
                    "data": {
                        "scanToken": db.data["scanToken"].clone(),
                        "scanServer": db.data["scanServer"].clone(),
                        "statusLabels": db.data["statusLabels"].clone(),
                        "nameFmt": db.display()["nameFmt"].clone()
                    }
                }), 200);
            }
            "/api/scan/qr" => {
                // 生成扫码二维码 SVG，内容为内网扫码地址
                let db = store.lock().unwrap();
                let server = db.data["scanServer"].as_str().unwrap_or("").trim().to_string();
                let url = if server.is_empty() {
                    // 未配置服务器地址时，优先用已绑定的 IP + 端口
                    let bind = db.data["bindIp"].as_str().unwrap_or("").trim();
                    let host = if !bind.is_empty() && bind != "0.0.0.0" {
                        bind.to_string()
                    } else {
                        req.headers().iter()
                            .find(|h| h.field.as_str() == "host")
                            .map(|h| h.value.as_str().split(':').next().unwrap_or("127.0.0.1").to_string())
                            .unwrap_or_else(|| "127.0.0.1".to_string())
                    };
                    let port = db.data["port"].as_u64().unwrap_or(80) as u16;
                    format!("http://{}:{}/scan", host, port)
                } else {
                    format!("{}/scan", server.trim_end_matches('/'))
                };
                let svg = gen_qr_svg(&url);
                let ct = Header::from_bytes(&b"Content-Type"[..], &b"image/svg+xml; charset=utf-8"[..]).unwrap();
                let url_header = Header::from_bytes(&b"X-Scan-Url"[..], url.as_bytes()).unwrap();
                let _ = req.respond(Response::from_data(svg.as_bytes().to_vec()).with_header(ct).with_header(url_header));
            }
            "/api/voice/current" => {
                json_response(req, json!({"ok": true, "data": crate::voice::current_audio()}), 200);
            }
            "/api/voice/audio" => {
                let id = extract_query(req.url(), "id");
                match crate::voice::audio_by_id(&id) {
                    Some((data, mime)) => {
                        let ct = Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap();
                        let cc = Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=600"[..]).unwrap();
                        let _ = req.respond(Response::from_data(data).with_header(ct).with_header(cc));
                    }
                    None => not_found(req),
                }
            }
            _ => serve_static(req, path, &webdir),
    }
    Ok(())
}

fn handle_post(
    mut req: Request,
    path: &str,
    store: Arc<Mutex<Store>>,
    broadcast: Arc<Broadcast>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
) -> Result<(), ()> {
    let body = match read_body(&mut req) {
        Some(v) => v,
        None => { json_response(req, json!({"ok": false, "msg": "请求体格式错误"}), 400); return Ok(()); }
    };

    match path {
        "/api/status" => {
            let token = body["token"].as_str().unwrap_or("");
            let phone = match session_phone(&sessions.lock().unwrap(), token) {
                Some(p) => p,
                None => { json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(()); }
            };
            let cid = body["class_id"].as_str().unwrap_or("").trim();
            let status = num(body.get("status")).unwrap_or(-1.0) as i32;
            if cid.is_empty() || !(0..=2).contains(&status) {
                json_response(req, json!({"ok": false, "msg": "参数缺失"}), 400); return Ok(());
            }
            let mut db = store.lock().unwrap();
            // 只能操作该账号已绑定的班级
            let bound = db.data["teachers"].as_array().map(|arr| arr.iter()
                .any(|t| t["phone"].as_str() == Some(&phone) && t["class_id"].as_str() == Some(cid)))
                .unwrap_or(false);
            if !bound {
                json_response(req, json!({"ok": false, "msg": "该班级未绑定到此账号"}), 403); return Ok(());
            }
            let cur = db.find_class(cid).and_then(|c| c["status"].as_i64()).unwrap_or(0) as i32;
            // 状态不可逆：只能顺序推进 0 -> 1 -> 2，不可跳级或回退
            if status < cur || status > cur + 1 {
                json_response(req, json!({"ok": false, "msg": "状态不可逆，只能顺序推进"}), 200); return Ok(());
            }
            let success = set_status_locked(&mut db, cid, status, &phone, "");
            drop(db);
            if success { broadcast.send("update"); }
            json_response(req, json!({"ok": success, "msg": if success {"ok"} else {"班级不存在"}}), 200);
        }
        "/api/scan/status" => {
            // 扫码操作：校验扫码凭证后执行状态推进（单次加锁，避免死锁）
            let token = body["token"].as_str().unwrap_or("").trim();
            let cid = body["class_id"].as_str().unwrap_or("").trim().to_string();
            let status = num(body.get("status")).unwrap_or(-1.0) as i32;
            if cid.is_empty() || !(0..=2).contains(&status) {
                json_response(req, json!({"ok": false, "msg": "参数缺失"}), 400); return Ok(());
            }
            let mut db = store.lock().unwrap();
            let expected = db.data["scanToken"].as_str().unwrap_or("");
            if token.is_empty() || token != expected {
                json_response(req, json!({"ok": false, "msg": "扫码凭证无效"}), 200); return Ok(());
            }
            let cur = db.find_class(&cid).and_then(|c| c["status"].as_i64()).unwrap_or(0) as i32;
            // 状态不可逆：只能顺序推进 0 -> 1 -> 2，不可跳级或回退
            if status < cur || status > cur + 1 {
                json_response(req, json!({"ok": false, "msg": "状态不可逆，只能顺序推进"}), 200); return Ok(());
            }
            let success = set_status_locked(&mut db, &cid, status, "scan", "");
            let labels = db.data["statusLabels"].clone();
            drop(db);
            if success { broadcast.send("update"); }
            json_response(req, json!({
                "ok": success,
                "msg": if success {"ok"} else {"班级不存在"},
                "statusLabels": labels
            }), 200);
        }
        "/api/admin/login" => {
            let pwd = body["password"].as_str().unwrap_or("");
            let db = store.lock().unwrap();
            let expected = db.data["admin"]["password"].as_str().unwrap_or("");
            // 首次启动尚未设置密码时禁止登录，需先在软件里设置管理密码
            if expected.is_empty() {
                json_response(req, json!({"ok": false, "msg": "请先在软件中设置管理密码"}), 401);
                return Ok(());
            }
            if pwd == expected {
                let token = gen_token();
                sessions.lock().unwrap().insert(token.clone(), "admin".to_string());
                json_response(req, json!({"ok": true, "token": token, "statusLabels": db.data["statusLabels"].clone()}), 200);
            } else {
                json_response(req, json!({"ok": false, "msg": "密码错误"}), 401);
            }
        }
        "/api/admin/status" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let status = num(body.get("status"));
            if status.is_none() || ![0.0, 1.0, 2.0].contains(&status.unwrap()) {
                json_response(req, json!({"ok": false, "msg": "状态缺失"}), 400); return Ok(());
            }
            let cid = body["class_id"].as_str().unwrap_or("").trim();
            let success = set_status(&store, cid, status.unwrap() as i32, "admin", body["reason"].as_str().unwrap_or(""));
            if success { broadcast.send("update"); }
            json_response(req, json!({"ok": success, "msg": if success {"ok"} else {"班级不存在"}}), 200);
        }
        "/api/admin/display" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            {
                let mut db = store.lock().unwrap();
                let cfg = if body["display"].is_object() { &body["display"] } else { &body };
                update_display(&mut db, cfg);
            }
            broadcast.send("display");
            json_response(req, json!({"ok": true, "msg": "已保存"}), 200);
        }
        "/api/admin/status-labels" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let arr = body["labels"].as_array()
                .or_else(|| body["statusLabels"].as_array());
            if let Some(arr) = arr {
                let mut labels = crate::model::default_status_labels();
                for i in 0..3 {
                    if let Some(s) = arr.get(i).and_then(|v| v.as_str()) {
                        let s = s.trim();
                        if !s.is_empty() && s.chars().count() <= 8 {
                            labels[i] = json!(s);
                        }
                    }
                }
                let mut db = store.lock().unwrap();
                db.data["statusLabels"] = labels.clone();
                db.persist();
                broadcast.send("display");
                json_response(req, json!({"ok": true, "msg": "已保存", "data": labels}), 200);
            } else {
                json_response(req, json!({"ok": false, "msg": "参数错误"}), 400);
            }
        }
        "/api/admin/scan-server" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let server = body["scanServer"].as_str().unwrap_or("").trim().to_string();
            if server.len() > 200 {
                json_response(req, json!({"ok": false, "msg": "地址过长"}), 400); return Ok(());
            }
            let mut db = store.lock().unwrap();
            db.data["scanServer"] = json!(server);
            db.persist();
            json_response(req, json!({"ok": true, "msg": "已保存", "data": server}), 200);
        }
        "/api/admin/password" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let old = body["old_password"].as_str().unwrap_or("");
            let new = body["new_password"].as_str().unwrap_or("");
            let mut db = store.lock().unwrap();
            if db.data["admin"]["password"].as_str() != Some(old) {
                json_response(req, json!({"ok": false, "msg": "原密码错误"}), 200);
            } else if new.is_empty() {
                json_response(req, json!({"ok": false, "msg": "新密码不能为空"}), 200);
            } else {
                db.data["admin"]["password"] = json!(new);
                db.persist();
                json_response(req, json!({"ok": true}), 200);
            }
        }
        "/api/admin/class/add" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let cid = body["class_id"].as_str().unwrap_or("").trim();
            let name = body["class_name"].as_str().unwrap_or("").trim();
            let mut db = store.lock().unwrap();
            if cid.is_empty() || name.is_empty() {
                json_response(req, json!({"ok": false, "msg": "编号和名称必填"}), 200);
            } else if db.find_class(cid).is_some() {
                json_response(req, json!({"ok": false, "msg": "班级编号已存在"}), 200);
            } else {
                let grade = num(body.get("grade")).unwrap_or(0.0) as i32;
                db.classes_mut().push(json!({
                    "class_id": cid, "class_name": name, "grade": grade,
                    "status": 0, "update_time": Value::Null
                }));
                db.persist();
                broadcast.send("update");
                json_response(req, json!({"ok": true, "msg": "已添加"}), 200);
            }
        }
        "/api/admin/class/del" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let cid = body["class_id"].as_str().unwrap_or("").trim();
            let mut db = store.lock().unwrap();
            let before = db.classes().len();
            db.classes_mut().retain(|c| c["class_id"].as_str() != Some(cid));
            db.data["teachers"].as_array_mut().unwrap().retain(|t| t["class_id"].as_str() != Some(cid));
            if db.classes().len() < before {
                db.persist();
                broadcast.send("update");
                json_response(req, json!({"ok": true}), 200);
            } else {
                json_response(req, json!({"ok": false, "msg": "班级不存在"}), 200);
            }
        }
        "/api/admin/schedule" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            // 立即执行指定定时器
            if body["run_now"].as_bool().unwrap_or(false) {
                let tid = num(body.get("timer_id")).unwrap_or(-1.0) as u64;
                let mut db = store.lock().unwrap();
                let (ok, msg) = run_timer_locked(&mut db, tid);
                let data = db.data["schedule"].clone();
                drop(db);
                broadcast.send("update");
                json_response(req, json!({"ok": ok, "msg": msg, "data": data}), 200);
                return Ok(());
            }
            // 保存定时器配置
            if let Some(timers) = body["schedule"]["timers"].as_array() {
                let mut db = store.lock().unwrap();
                if let Some(saved) = db.data["schedule"]["timers"].as_array_mut() {
                    for t in saved.iter_mut() {
                        let id = t["id"].as_u64().unwrap_or(u64::MAX);
                        if let Some(src) = timers.iter().find(|x| x["id"].as_u64() == Some(id)) {
                            t["enabled"] = json!(src["enabled"].as_bool().unwrap_or(false));
                            t["hour"] = json!(num(src.get("hour")).unwrap_or(0.0).clamp(0.0, 23.0) as u64);
                            t["minute"] = json!(num(src.get("minute")).unwrap_or(0.0).clamp(0.0, 59.0) as u64);
                        }
                    }
                }
                db.persist();
                let data = db.data["schedule"].clone();
                drop(db);
                json_response(req, json!({"ok": true, "msg": "已保存", "data": data}), 200);
                return Ok(());
            }
            json_response(req, json!({"ok": false, "msg": "参数缺失"}), 200);
        }
        "/api/admin/voice" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let mut db = store.lock().unwrap();
            let mut settings = crate::model::default_voice();
            if let Some(obj) = body.get("settings").and_then(|v| v.as_object()) {
                for (k, v) in obj {
                    settings[k] = v.clone();
                }
            }
            // 字段校验与钳制
            settings["enabled"] = json!(settings["enabled"].as_bool().unwrap_or(false));
            let eng = settings["engine"].as_str().unwrap_or("edge").trim().to_string();
            let eng = if eng == "sherpa" { "sherpa" } else { "edge" };
            settings["engine"] = json!(eng);
            let voices = crate::voice::engine().map(|e| e.list_voices(eng)).unwrap_or_default();
            let voice = settings["voice"].as_str().unwrap_or("").trim().to_string();
            let voice = if voices.iter().any(|v| v == &voice) { voice } else { voices.first().cloned().unwrap_or_default() };
            settings["voice"] = json!(voice);
            let speed = num(settings.get("speed")).unwrap_or(1.0).clamp(0.5, 2.0);
            settings["speed"] = json!(speed);
            let volume = num(settings.get("volume")).unwrap_or(1.0).clamp(0.0, 1.0);
            settings["volume"] = json!(volume);
            db.data["voice"] = settings.clone();
            db.persist();
            drop(db);
            crate::voice::update_settings(settings.clone());
            json_response(req, json!({"ok": true, "msg": "已保存", "data": settings}), 200);
        }
        "/api/admin/voice/test" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let text = body["text"].as_str().unwrap_or("").trim();
            let text = if text.is_empty() { "放学播报测试" } else { text };
            let settings = crate::voice::voice_settings_snapshot();
            if let Some(engine) = crate::voice::engine() {
                engine.test_from_settings(&settings, text);
            }
            json_response(req, json!({"ok": true, "msg": "已加入播报队列"}), 200);
        }
        "/api/admin/bind" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let phone = body["phone"].as_str().unwrap_or("").trim();
            let cid = body["class_id"].as_str().unwrap_or("").trim();
            let name = body["name"].as_str().unwrap_or("").trim();
            let mut db = store.lock().unwrap();
            if phone.is_empty() || cid.is_empty() {
                json_response(req, json!({"ok": false, "msg": "手机号与班级必填"}), 200);
            } else if db.find_class(cid).is_none() {
                json_response(req, json!({"ok": false, "msg": "班级不存在"}), 200);
            } else {
                // 同一手机号可绑定多个班级：仅当该「手机号+班级」组合已存在时更新，否则追加
                let teachers = db.data["teachers"].as_array_mut().unwrap();
                if let Some(t) = teachers.iter_mut().find(|t| {
                    t["phone"].as_str() == Some(phone) && t["class_id"].as_str() == Some(cid)
                }) {
                    t["name"] = json!(name);
                } else {
                    teachers.push(json!({"phone": phone, "class_id": cid, "name": name}));
                }
                db.persist();
                json_response(req, json!({"ok": true}), 200);
            }
        }
        "/api/admin/unbind" => {
            let token = body["token"].as_str().unwrap_or("");
            if !valid_session(&sessions.lock().unwrap(), token) {
                json_response(req, json!({"ok": false, "msg": "登录已失效"}), 401); return Ok(());
            }
            let phone = body["phone"].as_str().unwrap_or("").trim();
            let mut db = store.lock().unwrap();
            let before = db.data["teachers"].as_array().unwrap().len();
            db.data["teachers"].as_array_mut().unwrap().retain(|t| t["phone"].as_str() != Some(phone));
            if db.data["teachers"].as_array().unwrap().len() < before {
                db.persist();
                json_response(req, json!({"ok": true}), 200);
            } else {
                json_response(req, json!({"ok": false, "msg": "教师不存在"}), 200);
            }
        }
        _ => not_found(req),
    }
    Ok(())
}

fn set_status(store: &Arc<Mutex<Store>>, cid: &str, status: i32, phone: &str, reason: &str) -> bool {
    let mut db = store.lock().unwrap();
    set_status_locked(&mut db, cid, status, phone, reason)
}

/// 执行指定定时器：把所有班级重置为状态0，记录执行信息。
/// 返回执行结果消息；在持有 store 锁时调用。
fn run_timer_locked(db: &mut Store, timer_id: u64) -> (bool, String) {
    let label = db.data["schedule"]["timers"].as_array()
        .and_then(|a| a.iter().find(|t| t["id"].as_u64() == Some(timer_id)))
        .and_then(|t| t["label"].as_str())
        .unwrap_or("定时器").to_string();
    let ids: Vec<String> = db.classes().iter()
        .filter_map(|c| c["class_id"].as_str().map(|s| s.to_string())).collect();
    for cid in &ids {
        set_status_locked(db, cid, 0, "schedule", "");
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let now = now_str();
    if let Some(timers) = db.data["schedule"]["timers"].as_array_mut() {
        if let Some(t) = timers.iter_mut().find(|t| t["id"].as_u64() == Some(timer_id)) {
            t["lastRunDate"] = json!(today);
            t["lastRunTime"] = json!(now);
            t["lastStatus"] = json!("success");
            t["lastMsg"] = json!("重置成功");
            t["count"] = json!(t["count"].as_i64().unwrap_or(0) + 1);
        }
    }
    db.persist();
    (true, format!("{} 已执行：所有班级重置为初始状态", label))
}

/// 在已持有 Store 锁的情况下执行状态写入（避免重复加锁）
pub(crate) fn set_status_locked(db: &mut Store, cid: &str, status: i32, phone: &str, reason: &str) -> bool {
    let old;
    {
        let c = match db.find_class_mut(cid) { Some(c) => c, None => return false };
        old = c["status"].as_i64().unwrap_or(0);
        c["status"] = json!(status);
        c["update_time"] = json!(now_str());
    }
    let id = db.data["nextLogId"].as_i64().unwrap_or(1);
    db.data["logs"].as_array_mut().unwrap().push(json!({
        "id": id, "class_id": cid, "phone": phone,
        "action": if reason.is_empty() {"status"} else {"correct"},
        "old_status": old, "new_status": status, "reason": reason, "time": now_str()
    }));
    db.data["nextLogId"] = json!(id + 1);
    db.persist();
    // 语音播报：状态切换成功后入队（speak 非阻塞，不持锁等音频）
    let class_name = db.find_class(cid)
        .and_then(|c| c["class_name"].as_str())
        .unwrap_or(cid).to_string();
    let label = db.data["statusLabels"].as_array()
        .and_then(|a| a.get(status.max(0).min(2) as usize))
        .and_then(|v| v.as_str()).unwrap_or("").to_string();
    // 重置为状态0（初始状态）时不播报，避免定时任务批量重置时嘈杂
    if status != 0 {
        crate::voice::speak_status(&class_name, &label, cid);
    }
    true
}

fn update_display(db: &mut Store, cfg: &Value) {
    let cur = db.display().clone();
    let width = num(cfg.get("width")).or_else(|| num(cur.get("width"))).unwrap_or(1920.0)
        .max(64.0).min(7680.0) as i64;
    let height = num(cfg.get("height")).or_else(|| num(cur.get("height"))).unwrap_or(1080.0)
        .max(32.0).min(4320.0) as i64;
    let cols = num(cfg.get("cols")).or_else(|| num(cur.get("cols"))).unwrap_or(4.0)
        .max(1.0).min(20.0) as i64;
    let name_fmt = cfg["nameFmt"].as_str().unwrap_or_else(|| cur["nameFmt"].as_str().unwrap_or("cn"));
    let name_fmt = if name_fmt == "full" { "full" } else { "cn" };
    let card_w = num(cfg.get("cardW")).unwrap_or(0.0).max(0.0).min(width as f64) as i64;
    let card_h = num(cfg.get("cardH")).unwrap_or(0.0).max(0.0).min(height as f64) as i64;

    let mut order: Vec<Value> = Vec::new();
    if let Some(arr) = cfg.get("order").and_then(|v| v.as_array()) {
        for v in arr {
            let s = v.as_str().unwrap_or("");
            if s.is_empty() || db.find_class(s).is_some() { order.push(json!(s)); }
        }
    }
    for c in db.classes() {
        let cid = c["class_id"].as_str().unwrap_or("");
        if !order.iter().any(|o| o.as_str() == Some(cid)) { order.push(json!(cid)); }
    }

    let mut advanced = cfg.get("advanced").cloned().unwrap_or_else(|| cur["advanced"].clone());
    // 渐变配置在 old 数据或局部提交时可能缺失，归一化补齐并校验，保证各渲染端字段齐全
    if advanced.is_object() {
        let g = advanced.get("gradient").cloned().unwrap_or_else(|| cur["advanced"]["gradient"].clone());
        advanced["gradient"] = crate::model::normalize_gradient(&g);
    }
    db.display_mut()["width"] = json!(width);
    db.display_mut()["height"] = json!(height);
    db.display_mut()["cols"] = json!(cols);
    db.display_mut()["order"] = json!(order);
    db.display_mut()["nameFmt"] = json!(name_fmt);
    db.display_mut()["cardW"] = json!(card_w);
    db.display_mut()["cardH"] = json!(card_h);
    db.display_mut()["advanced"] = advanced;
    db.persist();
}

fn sse(req: Request, broadcast: Arc<Broadcast>) -> Result<(), ()> {
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"text/event-stream; charset=utf-8"[..]).unwrap();
    let cc = Header::from_bytes(&b"Cache-Control"[..], &b"no-cache, no-transform"[..]).unwrap();
    let conn = Header::from_bytes(&b"Connection"[..], &b"keep-alive"[..]).unwrap();

    // SSE 需要保持连接：用自定义 Read 流持续输出
    let rx = broadcast.subscribe();
    let stream = SseStream { rx, first: true, last_ping: std::time::Instant::now() };
    let response = Response::empty(StatusCode(200))
        .with_header(ct).with_header(cc).with_header(conn)
        .with_data(stream, None);
    let _ = req.respond(response);
    Ok(())
}

struct SseStream {
    rx: Receiver<String>,
    first: bool,
    last_ping: std::time::Instant,
}

impl std::io::Read for SseStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.first {
                self.first = false;
                let msg = b": connected\n\n";
                buf[..msg.len()].copy_from_slice(msg);
                return Ok(msg.len());
            }
            if let Ok(msg) = self.rx.recv_timeout(Duration::from_millis(1000)) {
                let bytes = msg.as_bytes();
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                return Ok(n);
            }
            if self.last_ping.elapsed() > Duration::from_secs(25) {
                self.last_ping = std::time::Instant::now();
                let msg = b": ping\n\n";
                buf[..msg.len()].copy_from_slice(msg);
                return Ok(msg.len());
            }
        }
    }
}

fn serve_static(req: Request, path: &str, webdir: &WebDir) {
    let fname = match path {
        "/" | "/index" | "/index.html" | "/display" => "index.html",
        "/con" | "/control" | "/teacher" => "control.html",
        "/set" | "/admin" => "set.html",
        "/scan" => "scan.html",
        "/style.css" => "style.css",
        "/display.css" => "display.css",
        _ => { not_found(req); return; }
    };
    let full = webdir.0.join(fname);
    let body = match fs::read(full) { Ok(b) => b, Err(_) => { not_found(req); return; } };
    let ctype = if fname.ends_with(".css") { "text/css; charset=utf-8" } else { "text/html; charset=utf-8" };
    let ct = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap();
    let cl = Header::from_bytes(&b"Content-Length"[..], body.len().to_string().as_bytes()).unwrap();
    let cc = Header::from_bytes(&b"Cache-Control"[..], &b"no-store, no-cache, must-revalidate, max-age=0"[..]).unwrap();
    let pragma = Header::from_bytes(&b"Pragma"[..], &b"no-cache"[..]).unwrap();
    let _ = req.respond(Response::from_data(body).with_header(ct).with_header(cl).with_header(cc).with_header(pragma));
}

fn extract_query(url: &str, key: &str) -> String {
    let q = url.split('?').nth(1).unwrap_or("");
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return it.next().unwrap_or("").to_string();
        }
    }
    String::new()
}

fn gen_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..24).map(|_| rng.gen::<u8>()).collect();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 用 qrcodegen 生成二维码，输出为 SVG 字符串
fn gen_qr_svg(content: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = match QrCode::encode_text(content, QrCodeEcc::Medium) {
        Ok(q) => q,
        Err(_) => return String::new(),
    };
    let size = qr.size() as usize;
    // 每模块 8px，加 4 模块留白
    let cell = 8;
    let margin = 4;
    let dim = (size + margin * 2) * cell;
    let mut rects = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x as i32, y as i32) {
                let px = (x + margin) * cell;
                let py = (y + margin) * cell;
                rects.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>", px, py, cell, cell
                ));
            }
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" shape-rendering=\"crispEdges\"><rect width=\"{}\" height=\"{}\" fill=\"#ffffff\"/><g fill=\"#000000\">{}</g></svg>",
        dim, dim, dim, dim, dim, dim, rects
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::default_status_labels;

    fn test_store() -> Store {
        let data = json!({
            "classes": [
                {"class_id": "101", "class_name": "1年1班", "status": 0, "update_time": null}
            ],
            "logs": [],
            "nextLogId": 1,
            "statusLabels": default_status_labels(),
            "schedule": {"timers": [{"id": 0, "label": "定时器1", "enabled": true, "hour": 8, "minute": 30, "lastRunDate": "", "lastRunTime": "", "lastStatus": "idle", "lastMsg": "", "count": 0}]}
        });
        Store { data, path: std::env::temp_dir().join("school-display-test.json") }
    }

    #[test]
    fn admin_session_only_accepts_admin() {
        let mut sessions = HashMap::new();
        sessions.insert("admin-token".to_string(), "admin".to_string());
        sessions.insert("teacher-token".to_string(), "13800000000".to_string());
        sessions.insert("guest-token".to_string(), "guest-admin".to_string());
        assert!(valid_session(&sessions, "admin-token"));
        assert!(!valid_session(&sessions, "teacher-token"));
        assert!(!valid_session(&sessions, "guest-token"));
        assert!(!valid_session(&sessions, ""));
    }

    #[test]
    fn status_write_writes_log_and_advances_id() {
        let mut db = test_store();
        assert!(set_status_locked(&mut db, "101", 1, "admin", ""));
        assert_eq!(db.data["classes"][0]["status"], 1);
        assert_eq!(db.data["logs"][0]["old_status"], 0);
        assert_eq!(db.data["logs"][0]["new_status"], 1);
        assert_eq!(db.data["logs"][0]["phone"], "admin");
        assert_eq!(db.data["nextLogId"], 2);
    }

    #[test]
    fn status_write_rejects_unknown_class() {
        let mut db = test_store();
        assert!(!set_status_locked(&mut db, "999", 1, "admin", ""));
        assert_eq!(db.data["logs"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn run_timer_resets_all_classes_and_records_status() {
        let mut db = test_store();
        set_status_locked(&mut db, "101", 2, "admin", "");
        assert_eq!(db.data["classes"][0]["status"], 2);
        let (ok, msg) = run_timer_locked(&mut db, 0);
        assert!(ok);
        assert!(msg.contains("已执行"));
        assert_eq!(db.data["classes"][0]["status"], 0);
        let t = &db.data["schedule"]["timers"][0];
        assert_eq!(t["lastStatus"], "success");
        assert_eq!(t["count"], 1);
    }
}
