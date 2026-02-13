use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::tray;

pub const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+V";

static REGISTERED_SHORTCUT: OnceLock<Mutex<Option<Shortcut>>> = OnceLock::new();

fn hotkey_state() -> &'static Mutex<Option<Shortcut>> {
    REGISTERED_SHORTCUT.get_or_init(|| Mutex::new(None))
}

fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    value
        .trim()
        .parse()
        .map_err(|e| format!("invalid shortcut '{}': {}", value, e))
}

pub fn register_hotkey<R: Runtime>(app: &AppHandle<R>, value: &str) -> Result<String, String> {
    let shortcut = parse_shortcut(value)?;
    let shortcut_string = shortcut.to_string();
    let manager = app.global_shortcut();

    let mut state = hotkey_state()
        .lock()
        .map_err(|_| "failed to lock hotkey state".to_string())?;
    if let Some(old) = state.take() {
        let _ = manager.unregister(old);
    }

    manager
        .register(shortcut)
        .map_err(|e| format!("failed to register global shortcut: {}", e))?;
    *state = Some(
        shortcut_string
            .parse()
            .map_err(|e| format!("failed to store registered hotkey: {}", e))?,
    );

    Ok(shortcut_string)
}

pub fn register_from_settings_or_default<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let settings = crate::database::get_settings().map_err(|e| e.to_string())?;
    match register_hotkey(app, &settings.hotkey) {
        Ok(_) => Ok(()),
        Err(_err) => {
            register_hotkey(app, DEFAULT_SHORTCUT).map(|_| ())
        }
    }
}

pub fn on_shortcut_triggered<R: Runtime>(app: &AppHandle<R>, _shortcut: &Shortcut) {
    if !crate::is_frontend_ready() {
        crate::queue_show_near_cursor_on_ready();
        return;
    }
    let _ = tray::show_main_window_near_cursor(app);
}
