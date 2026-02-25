mod autostart;
mod clipboard;
mod commands;
mod database;
mod hotkey;
mod models;
mod tray;

use commands::{
    add_custom_favorite_record, clear_favorite_items, clear_history_only, delete_clipboard_record,
    export_favorites_to_path, get_app_settings, get_favorite_records, get_history_records,
    import_favorites_from_path, save_app_settings, search_favorite_records, search_records,
    set_frontend_ready, set_record_favorite_state, set_record_pinned_state, suspend_auto_hide,
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

static LAST_GEOMETRY_EVENT_MS: AtomicU64 = AtomicU64::new(0);
static LAST_MAIN_WINDOW_SHOW_MS: AtomicU64 = AtomicU64::new(0);
static AUTO_HIDE_SUSPEND_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static FRONTEND_READY: AtomicBool = AtomicBool::new(false);
static PENDING_SHOW_NEAR_CURSOR: AtomicBool = AtomicBool::new(false);
const GEOMETRY_FOCUS_GUARD_MS: u64 = 120;
const SHOW_GEOMETRY_SUPPRESS_MS: u64 = 260;
const CURSOR_NEAR_WINDOW_MARGIN_PX: f64 = 8.0;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn should_track_geometry_event() -> bool {
    let elapsed = now_ms().saturating_sub(LAST_MAIN_WINDOW_SHOW_MS.load(Ordering::SeqCst));
    elapsed >= SHOW_GEOMETRY_SUPPRESS_MS
}

pub fn mark_main_window_shown() {
    LAST_MAIN_WINDOW_SHOW_MS.store(now_ms(), Ordering::SeqCst);
}

pub fn mark_frontend_ready() {
    FRONTEND_READY.store(true, Ordering::SeqCst);
}

pub fn is_frontend_ready() -> bool {
    FRONTEND_READY.load(Ordering::SeqCst)
}

pub fn queue_show_near_cursor_on_ready() {
    PENDING_SHOW_NEAR_CURSOR.store(true, Ordering::SeqCst);
}

pub fn take_pending_show_near_cursor() -> bool {
    PENDING_SHOW_NEAR_CURSOR.swap(false, Ordering::SeqCst)
}

fn auto_hide_is_suspended() -> bool {
    now_ms() < AUTO_HIDE_SUSPEND_UNTIL_MS.load(Ordering::SeqCst)
}

pub fn suspend_main_window_auto_hide(ms: u64) {
    let duration_ms = ms.clamp(200, 15_000);
    AUTO_HIDE_SUSPEND_UNTIL_MS.store(now_ms().saturating_add(duration_ms), Ordering::SeqCst);
}

fn cursor_is_near_window(window: &tauri::Window) -> Option<bool> {
    let cursor = match window.cursor_position() {
        Ok(pos) => pos,
        Err(_) => return None,
    };
    let position = match window.outer_position() {
        Ok(pos) => pos,
        Err(_) => return None,
    };
    let size = match window.outer_size() {
        Ok(size) => size,
        Err(_) => return None,
    };

    let left = position.x as f64 - CURSOR_NEAR_WINDOW_MARGIN_PX;
    let top = position.y as f64 - CURSOR_NEAR_WINDOW_MARGIN_PX;
    let right = position.x as f64 + size.width as f64 + CURSOR_NEAR_WINDOW_MARGIN_PX;
    let bottom = position.y as f64 + size.height as f64 + CURSOR_NEAR_WINDOW_MARGIN_PX;

    Some(cursor.x >= left && cursor.x <= right && cursor.y >= top && cursor.y <= bottom)
}

fn clamp_main_window_to_work_area(window: &tauri::Window) {
    if window.label() != "main" {
        return;
    }

    let position = match window.outer_position() {
        Ok(pos) => pos,
        Err(_) => return,
    };
    let size = match window.outer_size() {
        Ok(size) => size,
        Err(_) => return,
    };
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };

    let work_area = monitor.work_area();
    let min_x = work_area.position.x;
    let min_y = work_area.position.y;
    let max_x = (work_area.position.x + work_area.size.width as i32 - size.width as i32).max(min_x);
    let max_y =
        (work_area.position.y + work_area.size.height as i32 - size.height as i32).max(min_y);
    let clamped_x = position.x.clamp(min_x, max_x);
    let clamped_y = position.y.clamp(min_y, max_y);

    if clamped_x != position.x || clamped_y != position.y {
        let _ = window.set_position(tauri::PhysicalPosition::new(clamped_x, clamped_y));
    }
}

fn schedule_hide_recheck(window: tauri::Window, delay_ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        if auto_hide_is_suspended() {
            return;
        }
        let unfocused = window.is_focused().map(|focused| !focused).unwrap_or(true);
        let near = cursor_is_near_window(&window);
        if unfocused && near == Some(false) {
            let _ = window.hide();
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    FRONTEND_READY.store(false, Ordering::SeqCst);
    PENDING_SHOW_NEAR_CURSOR.store(false, Ordering::SeqCst);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        hotkey::on_shortcut_triggered(app, shortcut);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_history_records,
            search_records,
            get_favorite_records,
            search_favorite_records,
            add_custom_favorite_record,
            delete_clipboard_record,
            clear_history_only,
            clear_favorite_items,
            set_record_favorite_state,
            set_record_pinned_state,
            export_favorites_to_path,
            import_favorites_from_path,
            get_app_settings,
            save_app_settings,
            suspend_auto_hide,
            set_frontend_ready,
            commands::open_url,
            clipboard::paste_record_content,
        ])
        .setup(|app| {
            // Initialize database on startup
            let _ = database::init_database();

            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.set_skip_taskbar(true);

                let scale_factor = main_window.scale_factor().unwrap_or(1.0);

                // 使用 PhysicalSize 设置最小尺寸
                let min_width = (280.0 * scale_factor) as u32;
                let min_height = (430.0 * scale_factor) as u32;
                let min_size =
                    tauri::Size::Physical(tauri::PhysicalSize::new(min_width, min_height));
                let _ = main_window.set_min_size(Some(min_size));

                // 恢复保存的窗口尺寸 - 需要先显示窗口才能设置尺寸
                if let Ok(Some((width, height))) = database::get_window_size() {
                    let _ = main_window.show();
                    let size = tauri::Size::Physical(tauri::PhysicalSize::new(
                        width as u32,
                        height as u32,
                    ));
                    let _ = main_window.set_size(size);
                    let _ = main_window.hide();
                } else {
                    let _ = main_window.hide();
                }
            }

            // Create system tray
            let handle = app.handle();
            let _ = tray::create_tray(handle);
            let _ = hotkey::register_from_settings_or_default(handle);
            let _ = autostart::sync_from_settings(handle);

            // Start clipboard monitoring automatically
            let monitor_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let _ = clipboard::start_monitoring(monitor_handle).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // Prevent close, hide to tray instead
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Moved(_) => {
                if window.label() == "main" && should_track_geometry_event() {
                    LAST_GEOMETRY_EVENT_MS.store(now_ms(), Ordering::SeqCst);
                }
                clamp_main_window_to_work_area(window);
            }
            tauri::WindowEvent::Resized(_size) => {
                if window.label() == "main" && should_track_geometry_event() {
                    LAST_GEOMETRY_EVENT_MS.store(now_ms(), Ordering::SeqCst);
                }
                if window.label() == "main" {
                    clamp_main_window_to_work_area(window);
                }
            }
            tauri::WindowEvent::Focused(false) => {
                if window.label() != "main" {
                    return;
                }
                if auto_hide_is_suspended() {
                    return;
                }

                // 标题栏拖动/缩放场景优先保护：鼠标仍贴近窗口时不立即隐藏。
                match cursor_is_near_window(window) {
                    Some(true) | None => {
                        schedule_hide_recheck(window.clone(), 120);
                        schedule_hide_recheck(window.clone(), 240);
                        return;
                    }
                    Some(false) => {}
                }

                // 缩放/移动窗口会触发短暂失焦，忽略该时间窗内的自动隐藏。
                let elapsed =
                    now_ms().saturating_sub(LAST_GEOMETRY_EVENT_MS.load(Ordering::SeqCst));
                if elapsed < GEOMETRY_FOCUS_GUARD_MS {
                    let delay = GEOMETRY_FOCUS_GUARD_MS.saturating_sub(elapsed) + 16;
                    schedule_hide_recheck(window.clone(), delay);
                    schedule_hide_recheck(window.clone(), delay + 96);
                    return;
                }

                // 常规失焦直接隐藏，确保点击外部几乎即时关闭。
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
