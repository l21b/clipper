use crate::database::{
    add_record, clear_favorite_history, clear_non_favorite_history, delete_record, favorite_exists,
    get_all_favorite_history, get_favorite_history, get_history, get_settings, save_settings,
    search_favorite_history, search_history, set_record_favorite, set_record_pinned,
};
use crate::models::{ClipboardRecord, Settings};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteTransferItem {
    pub content_type: String,
    pub content: String,
    pub is_pinned: bool,
    pub source_app: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteTransferPackage {
    pub version: u32,
    pub exported_at: String,
    pub favorites: Vec<FavoriteTransferItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteExportResult {
    pub count: i32,
    pub path: String,
}

fn collect_favorites_package() -> Result<FavoriteTransferPackage, String> {
    let records = get_all_favorite_history().map_err(|e| e.to_string())?;
    let favorites = records
        .into_iter()
        .filter(|r| r.content_type != "image")
        .map(|r| FavoriteTransferItem {
            content_type: r.content_type,
            content: r.content,
            is_pinned: r.is_pinned,
            source_app: r.source_app,
        })
        .collect();

    Ok(FavoriteTransferPackage {
        version: 1,
        exported_at: chrono::Local::now().to_rfc3339(),
        favorites,
    })
}

fn import_favorites_from_payload(payload: &str) -> Result<i32, String> {
    let parsed: FavoriteTransferPackage =
        serde_json::from_str(payload).map_err(|e| format!("invalid json: {}", e))?;

    if parsed.version != 1 {
        return Err(format!("unsupported version: {}", parsed.version));
    }

    let mut imported: i32 = 0;
    for item in parsed.favorites {
        let content = item.content.trim();
        if content.is_empty() {
            continue;
        }

        let mut content_type = item.content_type.trim().to_string();
        if content_type.is_empty() {
            content_type = "text".to_string();
        }

        if !matches!(content_type.as_str(), "text" | "link" | "html") {
            content_type = "text".to_string();
        }

        if favorite_exists(&content_type, content).map_err(|e| e.to_string())? {
            continue;
        }

        let source_app = if item.source_app.trim().is_empty() {
            "Import".to_string()
        } else {
            item.source_app.trim().to_string()
        };

        let record = ClipboardRecord {
            id: 0,
            content_type,
            content: content.to_string(),
            image_data: None,
            is_favorite: true,
            is_pinned: item.is_pinned,
            source_app,
            created_at: chrono::Local::now().to_rfc3339(),
        };

        add_record(record).map_err(|e| e.to_string())?;
        imported += 1;
    }

    Ok(imported)
}

#[tauri::command]
pub fn get_history_records(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, String> {
    let records = get_history(limit, offset).map_err(|e| e.to_string())?;
    Ok(records)
}

#[tauri::command]
pub fn search_records(keyword: String, limit: i32) -> Result<Vec<ClipboardRecord>, String> {
    let records = search_history(&keyword, limit).map_err(|e| e.to_string())?;
    Ok(records)
}

#[tauri::command]
pub fn get_favorite_records(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, String> {
    let records = get_favorite_history(limit, offset).map_err(|e| e.to_string())?;
    Ok(records)
}

#[tauri::command]
pub fn search_favorite_records(
    keyword: String,
    limit: i32,
) -> Result<Vec<ClipboardRecord>, String> {
    let records = search_favorite_history(&keyword, limit).map_err(|e| e.to_string())?;
    Ok(records)
}

#[tauri::command]
pub fn add_custom_favorite_record(content: String) -> Result<i64, String> {
    use chrono::Local;

    let text = content.trim();
    if text.is_empty() {
        return Err("content cannot be empty".to_string());
    }

    let record = ClipboardRecord {
        id: 0,
        content_type: "text".to_string(),
        content: text.to_string(),
        image_data: None,
        is_favorite: true,
        is_pinned: false,
        source_app: "Manual".to_string(),
        created_at: Local::now().to_rfc3339(),
    };

    add_record(record).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_clipboard_record(id: i64) -> Result<(), String> {
    delete_record(id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn clear_history_only() -> Result<(), String> {
    clear_non_favorite_history().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn clear_favorite_items() -> Result<(), String> {
    clear_favorite_history().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_record_favorite_state(id: i64, favorite: bool) -> Result<(), String> {
    set_record_favorite(id, favorite).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_record_pinned_state(id: i64, pinned: bool) -> Result<(), String> {
    set_record_pinned(id, pinned).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn export_favorites_to_path(path: String) -> Result<FavoriteExportResult, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("path is empty".to_string());
    }

    let mut output = PathBuf::from(path);
    if output.is_dir() {
        let filename = format!(
            "snappaste-favorites-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        output.push(filename);
    } else if output.extension().is_none() {
        output.set_extension("json");
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let payload = collect_favorites_package()?;
    let count = payload.favorites.len() as i32;
    let json = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
    std::fs::write(&output, json).map_err(|e| e.to_string())?;
    Ok(FavoriteExportResult {
        count,
        path: output.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn import_favorites_from_path(path: String) -> Result<i32, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("path is empty".to_string());
    }

    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    import_favorites_from_payload(&json)
}

#[tauri::command]
pub fn get_app_settings() -> Result<Settings, String> {
    get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    // 1. 先尝试注册快捷键，获取标准化后的字符串
    let normalized = crate::hotkey::register_hotkey(&app, &settings.hotkey)?;
    let mut updated = settings;
    updated.hotkey = normalized;

    // 2. 先保存设置到数据库（最重要的一步，不能被后续操作阻断）
    save_settings(&updated).map_err(|e| e.to_string())?;

    // 3. 自启动设置为尽力而为，失败不阻断（开发模式下 disable 常因文件不存在而失败）
    if let Err(e) = crate::autostart::set_enabled(&app, updated.auto_start) {
        eprintln!("[WARN] auto start toggle failed (non-fatal): {}", e);
    }

    Ok(())
}

#[tauri::command]
pub fn suspend_auto_hide(ms: Option<u64>) {
    crate::suspend_main_window_auto_hide(ms.unwrap_or(4000));
}

#[tauri::command]
pub fn set_frontend_ready(app: AppHandle) {
    crate::mark_frontend_ready();
    if crate::take_pending_show_near_cursor() {
        let _ = crate::tray::show_main_window_near_cursor(&app);
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    opener::open(&url).map_err(|e| e.to_string())
}
