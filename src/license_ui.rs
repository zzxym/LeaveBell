//! 授权端图形算号机：读取 machine.txt + 私钥，生成 license.json。

use eframe::egui::{self, Color32};
use std::fs;
use std::path::PathBuf;

pub struct LicenseToolApp {
    base: PathBuf,
    machine: String,
    private_key: String,
    expires: String,
    features: String,
    public_key: String,
    message: String,
}

impl LicenseToolApp {
    pub fn new(base: PathBuf) -> Self {
        Self {
            machine: fs::read_to_string(base.join("machine.txt")).unwrap_or_default(),
            private_key: fs::read_to_string(base.join("license_private.key")).unwrap_or_default(),
            public_key: fs::read_to_string(base.join("license_public.key")).unwrap_or_default(),
            base,
            expires: "2099-12-31".to_string(),
            features: "full".to_string(),
            message: String::new(),
        }
    }
}

impl eframe::App for LicenseToolApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::app::setup_chinese_font_once(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("LeaveBell 算号机");
            ui.label("machine.txt 内容（客户端机器码）：");
            ui.add(egui::TextEdit::multiline(&mut self.machine).desired_rows(3).desired_width(620.0));
            ui.label("license_private.key 内容（授权端私钥，不要发给客户）：");
            ui.add(egui::TextEdit::multiline(&mut self.private_key).password(true).desired_rows(3).desired_width(620.0));
            ui.horizontal(|ui| {
                ui.label("到期日 YYYY-MM-DD：");
                ui.add(egui::TextEdit::singleline(&mut self.expires).desired_width(140.0));
                ui.label("功能标志：");
                ui.add(egui::TextEdit::singleline(&mut self.features).desired_width(120.0));
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("生成密钥对").clicked() {
                    self.message = crate::license::keygen(&self.base).unwrap_or_else(|e| e);
                    self.public_key = fs::read_to_string(self.base.join("license_public.key")).unwrap_or_default();
                    self.private_key = fs::read_to_string(self.base.join("license_private.key")).unwrap_or_default();
                }
                if ui.button("生成 license.json").clicked() {
                    let _ = fs::write(self.base.join("machine.txt"), self.machine.trim());
                    let _ = fs::write(self.base.join("license_private.key"), self.private_key.trim());
                    self.message = crate::license::sign(&self.base, &self.expires, &self.features)
                        .unwrap_or_else(|e| e);
                }
            });
            if !self.public_key.trim().is_empty() {
                ui.separator();
                ui.label(format!("当前公钥：{}", self.public_key.trim()));
                ui.label("生成新密钥对后，必须把新公钥嵌进程序并重新发布，旧许可证会全部失效。");
            }
            if !self.message.is_empty() {
                ui.colored_label(Color32::from_rgb(220, 120, 40), &self.message);
            }
        });
    }
}
