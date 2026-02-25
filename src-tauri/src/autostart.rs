use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let currently_enabled = manager.is_enabled().unwrap_or(false);
    if enabled == currently_enabled {
        return Ok(());
    }
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("failed to enable auto start: {}", e))?;
    } else {
        if let Err(e) = manager.disable() {
            eprintln!("[WARN] auto start disable skipped: {}", e);
        }
    }
    Ok(())
}

pub fn sync_from_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let settings = crate::database::get_settings().map_err(|e| e.to_string())?;
    set_enabled(app, settings.auto_start)
}
