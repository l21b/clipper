use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 弹窗类型 (前端需匹配 "info", "error", "success")
#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DialogType {
    Info,
    Error,
    #[allow(dead_code)]
    Success,
}

/// 序列化传输载体 (使用生命周期实现零拷贝)
#[derive(Clone, Serialize)]
struct DialogMessage<'a> {
    msg_type: DialogType,
    title: &'a str,
    content: &'a str,
}

/// 内部辅助：向前端发送标准的弹窗事件
fn emit_dialog(app: &AppHandle, event: &str, msg_type: DialogType, title: &str, content: &str) {
    let dialog = DialogMessage {
        msg_type,
        title,
        content,
    };
    if let Err(e) = app.emit(event, dialog) {
        eprintln!("Failed to emit dialog event '{}': {}", event, e);
    }
}

/// 触发常规内部弹窗 (主要用于在主界面内显示提示)
#[allow(dead_code)]
pub fn show_dialog(app: &AppHandle, msg_type: DialogType, title: &str, content: &str) {
    emit_dialog(app, "show-dialog", msg_type, title, content);
}

/// 触发全局独立 Toast/Popup 提示 (不依赖主界面是否获取焦点)
pub fn show_popup(app: &AppHandle, msg_type: DialogType, title: &str, content: &str) {
    emit_dialog(app, "popup-content", msg_type, title, content);
}
