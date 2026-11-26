//! 机器码生成与离线许可证验签。
//! 许可证为 Ed25519 签名的 JSON，放 exe 同目录 license.json。

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ring::digest::{digest, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// 发布时替换为 --license-keygen 生成的 license_public.key 内容（base64，32 字节）
const PUBLIC_KEY_B64: &str = "pwYRagUoUgf5nukzNJ1NG8A38GkfqJQbMfZUBdiCIcE=";

#[derive(Clone)]
pub struct MachineInfo {
    pub machine: String,
}

pub struct VerifyResult {
    pub ok: bool,
    pub expires: String,
    pub message: String,
    pub machine: MachineInfo,
}

pub struct LicenseState {
    pub registered: bool,
    pub expired: bool,
    pub expires: String,
    pub warn: String,
    pub machine: MachineInfo,
    pub started: Instant,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(target_arch = "x86_64")]
fn cpu_signature() -> String {
    use std::arch::x86_64::__cpuid;
    let vendor = __cpuid(0);
    let bytes = [vendor.ebx.to_le_bytes(), vendor.edx.to_le_bytes(), vendor.ecx.to_le_bytes()].concat();
    let vendor = String::from_utf8_lossy(&bytes).trim().to_string();
    let version = __cpuid(1);
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    format!("{}|{:08x}|{}", vendor, version.eax, cores)
}

#[cfg(not(target_arch = "x86_64"))]
fn cpu_signature() -> String {
    format!("unknown|{}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
}

#[cfg(target_os = "windows")]
fn physical_macs() -> Vec<String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_MULTICAST, GAA_FLAG_SKIP_DNS_SERVER,
    };
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut out = Vec::new();
    unsafe {
        let mut size = 0u32;
        let _ = GetAdaptersAddresses(0, flags, None, None, &mut size);
        if size == 0 { return out; }
        let mut buf = vec![0u8; size as usize];
        let ptr = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        if GetAdaptersAddresses(0, flags, None, Some(ptr), &mut size) != 0 { return out; }
        let mut cur = ptr;
        while !cur.is_null() {
            let adapter = &*cur;
            let name = adapter.FriendlyName.to_string().unwrap_or_default().to_lowercase();
            let excluded = ["virtual", "vmware", "hyper-v", "vethernet", "tap",
                "virtualbox", "bluetooth", "蓝牙", "loopback"]
                .iter().any(|k| name.contains(k));
            // 6=以太网，71=无线；其余（环回/隧道/蓝牙等）全部排除
            if !excluded && (adapter.IfType == 6 || adapter.IfType == 71)
                && adapter.PhysicalAddressLength == 6 {
                let raw = &adapter.PhysicalAddress[..6];
                if raw.iter().any(|b| *b != 0) {
                    out.push(raw.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(":"));
                }
            }
            cur = adapter.Next;
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(not(target_os = "windows"))]
fn physical_macs() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned().to_lowercase();
            let excluded = name == "lo"
                || ["veth", "docker", "virbr", "tun", "tap", "wg", "br", "bond", "vm", "bluetooth"]
                    .iter().any(|k| name.starts_with(k));
            if excluded { continue; }
            if let Ok(s) = fs::read_to_string(entry.path().join("address")) {
                let mac = s.trim().to_uppercase();
                if mac.len() == 17 && mac.matches(':').count() == 5 && mac != "00:00:00:00:00:00" {
                    out.push(mac);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

impl MachineInfo {
    fn payload(&self, expires: &str, features: &str) -> String {
        format!("{}|{}|{}", self.machine, expires, features)
    }

    pub fn text(&self) -> String {
        format!("{}\n", self.machine)
    }
}

fn valid_expiry(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

fn machine_info(base: &Path) -> MachineInfo {
    let cpu_raw = cpu_signature();
    let macs = physical_macs().join(",");
    // 首次运行生成随机盐并持久化：同型号同网卡的机器也能区分
    let salt_path = base.join("machine.id");
    let salt = match fs::read(&salt_path) {
        Ok(v) if v.len() >= 16 => v,
        _ => {
            let v: [u8; 16] = rand::random();
            let _ = fs::write(&salt_path, v);
            v.to_vec()
        }
    };
    let machine = hex(digest(&SHA256, format!("{}|{}|{}", cpu_raw, macs, hex(&salt)).as_bytes()).as_ref());
    let cpu = hex(digest(&SHA256, cpu_raw.as_bytes()).as_ref());
    let mac = hex(digest(&SHA256, macs.as_bytes()).as_ref());
    MachineInfo { machine: format!("{}{}{}", machine, cpu, mac) }
}

pub fn machine_file_path(base: &Path) -> PathBuf { base.join("machine.txt") }
pub fn license_file_path(base: &Path) -> PathBuf { base.join("license.json") }

pub fn write_machine_file(base: &Path, info: &MachineInfo) {
    let _ = fs::write(machine_file_path(base), info.text());
}

fn public_key() -> Result<Vec<u8>, String> {
    STANDARD.decode(PUBLIC_KEY_B64).map_err(|e| format!("内置公钥错误: {}", e))
}

pub fn verify(base: &Path) -> VerifyResult {
    let machine = machine_info(base);
    let data = match fs::read_to_string(license_file_path(base)) {
        Ok(v) => v,
        Err(_) => return VerifyResult {
            ok: false, expires: String::new(), message: "未注册".into(), machine,
        },
    };
    let value: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return VerifyResult {
            ok: false, expires: String::new(), message: "许可证格式错误".into(), machine,
        },
    };
    let expires = value["expires"].as_str().unwrap_or("").to_string();
    let features = value["features"].as_str().unwrap_or("").to_string();
    let sig = match STANDARD.decode(value["sig"].as_str().unwrap_or("")) {
        Ok(v) => v,
        Err(_) => return VerifyResult {
            ok: false, expires, message: "许可证签名格式错误".into(), machine,
        },
    };
    let lic_code = value["machine"].as_str().unwrap_or("").to_string();
    let payload = format!("{}|{}|{}", lic_code, expires, features);
    let public = match public_key() {
        Ok(v) => v,
        Err(e) => return VerifyResult { ok: false, expires, message: e, machine },
    };
    if UnparsedPublicKey::new(&ED25519, &public).verify(payload.as_bytes(), &sig).is_err() {
        return VerifyResult { ok: false, expires, message: "许可证验签失败".into(), machine };
    }
    if !valid_expiry(&expires) {
        return VerifyResult { ok: false, expires, message: "许可证到期日期无效".into(), machine };
    }
    if lic_code == machine.machine {
        return VerifyResult { ok: true, expires, message: String::new(), machine };
    }
    VerifyResult { ok: false, expires, message: "机器码不匹配，请重新授权".into(), machine }
}

pub fn initial(base: &Path) -> LicenseState {
    let result = verify(base);
    let machine = result.machine.clone();
    let state = LicenseState {
        registered: result.ok,
        expired: false,
        expires: result.expires,
        warn: result.message,
        machine,
        started: Instant::now(),
    };
    write_machine_file(base, &state.machine);
    state
}

pub fn reload(base: &Path, state: &mut LicenseState) {
    let result = verify(base);
    state.registered = result.ok;
    state.expires = result.expires;
    state.warn = result.message;
    if result.ok {
        state.expired = false;
    }
    write_machine_file(base, &result.machine);
}

/// 开发/授权端：生成 Ed25519 密钥对，写到 exe 目录。
pub fn keygen(base: &Path) -> Result<String, String> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|e| format!("生成密钥失败: {:?}", e))?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|e| format!("读取密钥失败: {}", e))?;
    let private = STANDARD.encode(pkcs8.as_ref());
    let public = STANDARD.encode(pair.public_key().as_ref());
    fs::write(base.join("license_private.key"), &private).map_err(|e| e.to_string())?;
    fs::write(base.join("license_public.key"), &public).map_err(|e| e.to_string())?;
    Ok(format!("已生成 license_private.key / license_public.key\n公钥: {}\n", public))
}

/// 开发/授权端：读取 machine.txt + 私钥，签发 license.json。
pub fn sign(base: &Path, expires: &str, features: &str) -> Result<String, String> {
    let text = fs::read_to_string(machine_file_path(base))
        .map_err(|_| "缺少 machine.txt，请先正常启动一次软件".to_string())?;
    let machine = text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.strip_prefix("machine=").unwrap_or(line).trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "machine.txt 内容不完整".to_string())?;
    if machine.len() != 64 && machine.len() != 192 {
        return Err("机器码长度应为 64 位十六进制".into());
    }
    if !valid_expiry(expires) {
        return Err("到期日期必须为 YYYY-MM-DD".into());
    }
    let private = fs::read_to_string(base.join("license_private.key"))
        .map_err(|_| "缺少 license_private.key".to_string())?;
    let pkcs8 = STANDARD.decode(private.trim())
        .map_err(|e| format!("私钥格式错误: {}", e))?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8)
        .map_err(|e| format!("私钥无效: {}", e))?;
    let info = MachineInfo { machine };
    let sig = STANDARD.encode(pair.sign(info.payload(expires, features).as_bytes()).as_ref());
    let license = json!({
        "machine": info.machine,
        "expires": expires,
        "features": features,
        "sig": sig,
    });
    fs::write(license_file_path(base), serde_json::to_string_pretty(&license).unwrap())
        .map_err(|e| e.to_string())?;
    Ok("已生成 license.json".into())
}
