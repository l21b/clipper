use arboard::{Clipboard, ImageData};
#[cfg(target_os = "windows")]
use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use enigo::{Enigo, Key, KeyboardControllable};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, ImageFormat};
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::CloseHandle;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

use crate::models::ClipboardRecord;

static MONITORING: AtomicBool = AtomicBool::new(false);
static MONITOR_SESSION_ID: AtomicU64 = AtomicU64::new(0);
/// 忽略下一次剪贴板变化（用于复制操作时避免重复记录）
static IGNORE_NEXT_CHANGE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static TARGET_HWND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(ptr::null_mut());
static LAST_IMAGE_RECORD_MS: AtomicU64 = AtomicU64::new(0);
static PASTE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
const ENABLE_IMAGE_RECORDING: bool = false;

const MAX_IMAGE_BYTES: usize = 48 * 1024 * 1024; // raw RGBA cap before processing
const MAX_IMAGE_PIXELS: usize = 2_600_000; // approx <= 1920x1350
const MAX_IMAGE_DIMENSION: usize = 2200;
const MAX_ENCODED_IMAGE_BYTES: usize = 6 * 1024 * 1024; // PNG blob cap
const MIN_IMAGE_RECORD_INTERVAL_MS: u64 = 1200;
const PASTE_SETTLE_MS_TEXT: u64 = 5;
const PASTE_SETTLE_MS_IMAGE: u64 = 20;
const PASTE_KEY_STEP_MS: u64 = 2;
#[cfg(target_os = "windows")]
const EVENT_MONITOR_RETRY_MIN_MS: u64 = 300;
#[cfg(target_os = "windows")]
const EVENT_MONITOR_RETRY_MAX_MS: u64 = 3000;

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn encode_rgba_to_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new(&mut out);
    encoder
        .write_image(rgba, width as u32, height as u32, ColorType::Rgba8.into())
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn downscale_rgba_nearest(
    width: usize,
    height: usize,
    rgba: &[u8],
    target_width: usize,
    target_height: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; target_width * target_height * 4];
    for ty in 0..target_height {
        let sy = ty * height / target_height;
        for tx in 0..target_width {
            let sx = tx * width / target_width;
            let src = (sy * width + sx) * 4;
            let dst = (ty * target_width + tx) * 4;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    out
}

fn normalize_image_for_storage<'a>(
    width: usize,
    height: usize,
    rgba: &'a [u8],
) -> (usize, usize, Cow<'a, [u8]>, bool) {
    let mut ratio: f64 = 1.0;
    if width > MAX_IMAGE_DIMENSION {
        ratio = ratio.max(width as f64 / MAX_IMAGE_DIMENSION as f64);
    }
    if height > MAX_IMAGE_DIMENSION {
        ratio = ratio.max(height as f64 / MAX_IMAGE_DIMENSION as f64);
    }
    let pixels = width.saturating_mul(height);
    if pixels > MAX_IMAGE_PIXELS {
        ratio = ratio.max((pixels as f64 / MAX_IMAGE_PIXELS as f64).sqrt());
    }

    if ratio <= 1.0 {
        return (width, height, Cow::Borrowed(rgba), false);
    }

    let target_width = ((width as f64 / ratio).round() as usize).max(1);
    let target_height = ((height as f64 / ratio).round() as usize).max(1);
    let resized = downscale_rgba_nearest(width, height, rgba, target_width, target_height);
    (target_width, target_height, Cow::Owned(resized), true)
}

fn build_image_record(width: usize, height: usize, rgba: &[u8]) -> Result<ClipboardRecord, String> {
    let (normalized_width, normalized_height, normalized_rgba, scaled) =
        normalize_image_for_storage(width, height, rgba);
    let png_bytes = encode_rgba_to_png(
        normalized_width,
        normalized_height,
        normalized_rgba.as_ref(),
    )?;
    if png_bytes.len() > MAX_ENCODED_IMAGE_BYTES {
        return Err(format!(
            "encoded image too large: {} bytes > {} bytes",
            png_bytes.len(),
            MAX_ENCODED_IMAGE_BYTES
        ));
    }

    Ok(ClipboardRecord {
        id: 0,
        content_type: "image".to_string(),
        content: if scaled {
            format!(
                "图片 {}x{} (缩放自 {}x{})",
                normalized_width, normalized_height, width, height
            )
        } else {
            format!("图片 {}x{}", width, height)
        },
        image_data: Some(png_bytes),
        is_favorite: false,
        is_pinned: false,
        source_app: get_source_app(),
        created_at: chrono::Local::now().to_rfc3339(),
    })
}

fn build_text_record(text: String) -> ClipboardRecord {
    let content_type = if is_url(&text) { "link" } else { "text" };
    ClipboardRecord {
        id: 0,
        content_type: content_type.to_string(),
        content: text,
        image_data: None,
        is_favorite: false,
        is_pinned: false,
        source_app: get_source_app(),
        created_at: chrono::Local::now().to_rfc3339(),
    }
}

fn image_signature(width: usize, height: usize, rgba: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    rgba.len().hash(&mut hasher);

    // 只采样部分字节，避免每次对超大图片做全量哈希
    for b in rgba.iter().take(4096) {
        b.hash(&mut hasher);
    }
    for b in rgba.iter().rev().take(4096) {
        b.hash(&mut hasher);
    }

    format!("image:{}:{}:{}", width, height, hasher.finish())
}


/// 设置剪贴板文本（使用 arboard）
pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;
    Ok(())
}

fn decode_png_rgba(png_bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let image = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((width as usize, height as usize, rgba.into_raw()))
}

fn set_clipboard_image_rgba(width: usize, height: usize, bytes: Vec<u8>) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(ImageData {
            width,
            height,
            bytes: Cow::Owned(bytes),
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn with_paste_in_progress<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    PASTE_IN_PROGRESS.store(true, Ordering::SeqCst);
    let result = f();
    PASTE_IN_PROGRESS.store(false, Ordering::SeqCst);
    result
}

/// 捕获呼出 UI 前的当前前台窗口，用于后续粘贴前恢复焦点。
#[cfg(target_os = "windows")]
pub fn capture_target_window() {
    unsafe {
        let hwnd = GetForegroundWindow();
        TARGET_HWND.store(hwnd as *mut _, Ordering::SeqCst);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_target_window() {}

#[cfg(target_os = "windows")]
fn force_restore_focus(hwnd: isize) -> bool {
    unsafe {
        if hwnd == 0 {
            return false;
        }

        if GetForegroundWindow() as isize == hwnd {
            return true;
        }

        let target_hwnd = hwnd as *mut std::ffi::c_void;
        let target_thread = GetWindowThreadProcessId(target_hwnd, ptr::null_mut());
        if target_thread == 0 {
            return false;
        }

        let current_thread = GetCurrentThreadId();
        if current_thread == target_thread {
            return SetForegroundWindow(target_hwnd) != 0;
        }

        let _ = AttachThreadInput(current_thread, target_thread, 1);
        let focused = SetForegroundWindow(target_hwnd) != 0;
        let _ = AttachThreadInput(current_thread, target_thread, 0);
        focused
    }
}

/// 检测是否为 URL
pub fn is_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.")
}

/// 获取来源应用
#[cfg(target_os = "windows")]
pub fn get_source_app() -> String {
    // 使用当前前台窗口的进程名作为来源应用（如 chrome / Code / explorer）。
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return "Unknown".to_string();
        }

        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return "Unknown".to_string();
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return "Unknown".to_string();
        }

        let mut buffer = vec![0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            0 as PROCESS_NAME_FORMAT,
            buffer.as_mut_ptr(),
            &mut size,
        );
        let _ = CloseHandle(process);

        if ok == 0 || size == 0 {
            return "Unknown".to_string();
        }

        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        let name = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .trim();

        if name.is_empty() {
            "Unknown".to_string()
        } else {
            name.to_string()
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_source_app() -> String {
    "Unknown".to_string()
}

fn next_monitor_session_id() -> u64 {
    MONITOR_SESSION_ID
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1)
}

fn is_monitor_session_active(session_id: u64) -> bool {
    MONITORING.load(Ordering::SeqCst) && MONITOR_SESSION_ID.load(Ordering::SeqCst) == session_id
}

fn build_startup_signature() -> Option<String> {
    Clipboard::new().ok().and_then(|mut clipboard| {
        if ENABLE_IMAGE_RECORDING {
            if let Ok(image) = clipboard.get_image() {
                return Some(image_signature(
                    image.width,
                    image.height,
                    image.bytes.as_ref(),
                ));
            }
        }
        if let Ok(text) = clipboard.get_text() {
            return Some(format!("text:{}", text));
        }
        None
    })
}

fn emit_history_changed(app: &AppHandle) {
    let _ = app.emit("history-changed", ());
}

fn process_clipboard_change(
    last_signature: &Mutex<Option<String>>,
    _source: &str,
    app: &AppHandle,
) {
    if PASTE_IN_PROGRESS.load(Ordering::SeqCst) {
        return;
    }

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(_) => {
            return;
        }
    };

    if ENABLE_IMAGE_RECORDING {
        if let Ok(image) = clipboard.get_image() {
            let raw = image.bytes.as_ref();
            let signature = image_signature(image.width, image.height, raw);
            let should_ignore = IGNORE_NEXT_CHANGE.swap(false, Ordering::SeqCst);
            let changed = if let Ok(mut last) = last_signature.lock() {
                if *last == Some(signature.clone()) {
                    false
                } else {
                    *last = Some(signature);
                    true
                }
            } else {
                false
            };

            if changed && !should_ignore {
                if raw.len() > MAX_IMAGE_BYTES {
                    return;
                }

                let now = now_ms();
                let last = LAST_IMAGE_RECORD_MS.load(Ordering::SeqCst);
                if now.saturating_sub(last) < MIN_IMAGE_RECORD_INTERVAL_MS {
                    return;
                }

                if let Ok(record) = build_image_record(image.width, image.height, raw) {
                    if crate::database::add_record(record).is_ok() {
                        LAST_IMAGE_RECORD_MS.store(now, Ordering::SeqCst);
                        emit_history_changed(app);
                    }
                }
            }
            return;
        }
    }

    if let Ok(text) = clipboard.get_text() {
        if text.trim().is_empty() {
            return;
        }

        let signature = format!("text:{}", text);
        let should_ignore = IGNORE_NEXT_CHANGE.swap(false, Ordering::SeqCst);
        let changed = if let Ok(mut last) = last_signature.lock() {
            if *last == Some(signature.clone()) {
                false
            } else {
                *last = Some(signature);
                true
            }
        } else {
            false
        };

        if changed && !should_ignore {
            let record = build_text_record(text);
            if crate::database::add_record(record).is_ok() {
                emit_history_changed(app);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run_polling_loop(last_signature: Arc<Mutex<Option<String>>>, session_id: u64, app: AppHandle) {
    while is_monitor_session_active(session_id) {
        if PASTE_IN_PROGRESS.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(120));
            continue;
        }

        thread::sleep(Duration::from_millis(500));
        if !is_monitor_session_active(session_id) {
            break;
        }
        process_clipboard_change(&last_signature, "poll", &app);
    }
}

#[cfg(target_os = "windows")]
struct ClipboardEventHandler {
    last_signature: Arc<Mutex<Option<String>>>,
    session_id: u64,
    app: AppHandle,
}

#[cfg(target_os = "windows")]
impl ClipboardHandler for ClipboardEventHandler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        if !is_monitor_session_active(self.session_id) {
            return CallbackResult::Stop;
        }

        process_clipboard_change(&self.last_signature, "event", &self.app);
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, _error: std::io::Error) -> CallbackResult {
        if !is_monitor_session_active(self.session_id) {
            CallbackResult::Stop
        } else {
            CallbackResult::Next
        }
    }
}

#[cfg(target_os = "windows")]
fn spawn_event_driven_monitor(
    last_signature: Arc<Mutex<Option<String>>>,
    session_id: u64,
    app: AppHandle,
) {
    thread::spawn(move || {
        let mut retry_delay_ms = EVENT_MONITOR_RETRY_MIN_MS;

        while is_monitor_session_active(session_id) {
            let handler = ClipboardEventHandler {
                last_signature: last_signature.clone(),
                session_id,
                app: app.clone(),
            };

            match Master::new(handler) {
                Ok(mut master) => {
                    retry_delay_ms = EVENT_MONITOR_RETRY_MIN_MS;

                    if master.run().is_err() {
                    } else {
                        break;
                    }
                }
                Err(_) => {}
            }

            if !is_monitor_session_active(session_id) {
                break;
            }

            thread::sleep(Duration::from_millis(retry_delay_ms));
            retry_delay_ms = (retry_delay_ms.saturating_mul(2)).min(EVENT_MONITOR_RETRY_MAX_MS);
        }
    });
}

/// 启动剪贴板监听
pub async fn start_monitoring(app: AppHandle) -> Result<(), String> {
    if MONITORING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let session_id = next_monitor_session_id();
    let last_signature = Arc::new(Mutex::new(build_startup_signature()));

    #[cfg(target_os = "windows")]
    {
        spawn_event_driven_monitor(last_signature, session_id, app);
    }

    #[cfg(not(target_os = "windows"))]
    {
        thread::spawn(move || run_polling_loop(last_signature, session_id, app));
    }

    Ok(())
}

/// 使用 enigo 模拟 Ctrl+V 粘贴到焦点窗口
fn send_paste_shortcut(focus_settle_ms: u64) {
    let mut enigo = Enigo::new();

    if focus_settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(focus_settle_ms));
    }

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo.key_down(modifier);
    std::thread::sleep(Duration::from_millis(PASTE_KEY_STEP_MS));
    enigo.key_click(Key::Layout('v'));
    std::thread::sleep(Duration::from_millis(PASTE_KEY_STEP_MS));
    enigo.key_up(modifier);
}

/// 一次性完成复制并粘贴：写剪贴板 -> 隐藏窗口 -> 发送粘贴按键
#[tauri::command]
pub async fn paste_record_content(app: AppHandle, id: i64) -> Result<(), String> {
    with_paste_in_progress(|| {
        let record = crate::database::get_record_by_id(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("record not found: {}", id))?;
        let is_image = record.content_type == "image";

        // 前端点击记录时，避免监听器把本次剪贴板写入再次记录
        IGNORE_NEXT_CHANGE.store(true, Ordering::SeqCst);

        if is_image {
            let data = record
                .image_data
                .ok_or_else(|| "image record missing image_data".to_string())?;
            let (width, height, bytes) = decode_png_rgba(&data)?;
            set_clipboard_image_rgba(width, height, bytes)?;
        } else {
            set_clipboard_text(&record.content)?;
        }

        #[cfg(target_os = "windows")]
        let target_hwnd = TARGET_HWND.swap(ptr::null_mut(), Ordering::SeqCst) as isize;

        if let Some(window) = app.get_webview_window("main") {
            window.hide().map_err(|e| e.to_string())?;

            #[cfg(target_os = "windows")]
            if target_hwnd != 0 {
                let _ = force_restore_focus(target_hwnd);
                thread::sleep(Duration::from_millis(20));
            }
        }

        // 给系统时间切换焦点到原目标窗口（图片略高于文本）
        send_paste_shortcut(if is_image {
            PASTE_SETTLE_MS_IMAGE
        } else {
            PASTE_SETTLE_MS_TEXT
        });

        Ok(())
    })
}

