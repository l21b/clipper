use crate::db::core::get_conn;
use crate::db::queries::{get_records, get_settings, save_settings};
use crate::models::{
    ContentType, FavoriteExportResult, FavoriteTransferItem, FavoriteTransferPackage,
};
use chrono::Local;
use rusqlite::params;
use std::collections::HashSet;
use std::path::PathBuf;

/// 导出逻辑：将当前收藏和设置打包为 JSON 结构
pub fn collect_favorites_package() -> Result<FavoriteTransferPackage, String> {
    let records = get_records(Some(true), None, None).map_err(|e| e.to_string())?;
    let favorites = records
        .into_iter()
        .filter(|r| r.content_type != ContentType::Image)
        .map(|r| FavoriteTransferItem {
            content_type: r.content_type,
            content: r.content,
            is_pinned: r.is_pinned,
        })
        .collect();

    let settings = get_settings().map_err(|e| e.to_string())?;
    Ok(FavoriteTransferPackage {
        favorites,
        settings,
    })
}

/// 导出到文件
pub fn export_favorites_to_path_logic(path: String) -> Result<FavoriteExportResult, String> {
    let mut output = PathBuf::from(path.trim());
    if output.as_os_str().is_empty() {
        return Err("path is empty".to_string());
    }

    if output.is_dir() {
        output.push(format!(
            "snappaste-favorites-{}.json",
            Local::now().format("%Y%m%d-%H%M%S")
        ));
    } else if output.extension().is_none() {
        output.set_extension("json");
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let pkg = collect_favorites_package()?;
    let count = pkg.favorites.len() as i32;
    let json = serde_json::to_string_pretty(&pkg).map_err(|e| e.to_string())?;

    std::fs::write(&output, json).map_err(|e| e.to_string())?;

    Ok(FavoriteExportResult {
        count,
        path: output.to_string_lossy().to_string(),
    })
}

/// 从 JSON 数据导入收藏和设置（极速批量插入优化版）
#[allow(dead_code)]
pub fn import_favorites_from_payload(payload: &str) -> Result<(i32, bool), String> {
    let parsed: FavoriteTransferPackage =
        serde_json::from_str(payload).map_err(|e| format!("invalid json: {}", e))?;

    let existing_records = get_records(Some(true), None, None).map_err(|e| e.to_string())?;
    let mut existing_set: HashSet<String> =
        existing_records.into_iter().map(|r| r.content).collect();

    let mut insert_params = Vec::new();

    for item in parsed.favorites {
        let text = item.content.trim();
        if text.is_empty() || item.content_type == ContentType::Image || existing_set.contains(text)
        {
            continue;
        }

        insert_params.push((item.content_type.clone(), text.to_string(), item.is_pinned));
        existing_set.insert(text.to_string());
    }

    let imported_count = insert_params.len() as i32;

    // 🏆 核心修复：开启事务进行高速安全批量插入，解决静默失败问题！
    if imported_count > 0 {
        let mut conn = get_conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        let created_at = Local::now().to_rfc3339();

        {
            let mut stmt = tx.prepare(
                "INSERT INTO clipboard_history (content_type, content, image_data, is_favorite, is_pinned, created_at) VALUES (?1, ?2, NULL, 1, ?3, ?4)"
            ).map_err(|e| e.to_string())?;

            for (ctype, content, is_pinned) in insert_params {
                stmt.execute(params![ctype, content, is_pinned as i32, &created_at])
                    .map_err(|e| format!("导入失败: {}", e))?;
            }
        }

        tx.commit().map_err(|e| format!("事务提交失败: {}", e))?;
    }

    let settings_imported = if !parsed.settings.hotkey.is_empty() {
        save_settings(&parsed.settings).map_err(|e| e.to_string())?;
        true
    } else {
        false
    };

    Ok((imported_count, settings_imported))
}

pub async fn import_favorites_from_path_logic(
    app: tauri::AppHandle,
    path: String,
) -> Result<(i32, bool), String> {
    let path = path.trim().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let json = std::fs::read_to_string(&path)
            .map_err(|e| format!("无法读取文件 {}: {}", path, e))?;

        let parsed: FavoriteTransferPackage =
            serde_json::from_str(&json).map_err(|e| format!("无效的 JSON 格式: {}", e))?;

        let imported_count = {
            let mut conn = get_conn();

            // 1. 获取现有收藏进行去重
            let existing_records = get_records(Some(true), None, None).map_err(|e| e.to_string())?;
            let mut existing_set: HashSet<String> =
                existing_records.into_iter().map(|r| r.content).collect();

            let mut insert_params = Vec::new();
            for item in parsed.favorites {
                let text = item.content.trim();
                if text.is_empty()
                    || item.content_type == ContentType::Image
                    || existing_set.contains(text)
                {
                    continue;
                }
                insert_params.push((item.content_type.clone(), text.to_string(), item.is_pinned));
                existing_set.insert(text.to_string());
            }

            let count = insert_params.len() as i32;

            // 2. 批量插入
            if count > 0 {
                let tx = conn.transaction().map_err(|e| e.to_string())?;
                let created_at = Local::now().to_rfc3339();
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO clipboard_history (content_type, content, image_data, is_favorite, is_pinned, created_at) VALUES (?1, ?2, NULL, 1, ?3, ?4)"
                    ).map_err(|e| e.to_string())?;

                    for (ctype, content, is_pinned) in insert_params {
                        stmt.execute(params![ctype, content, is_pinned as i32, &created_at])
                            .map_err(|e| format!("插入失败: {}", e))?;
                    }
                }
                tx.commit().map_err(|e| format!("提交失败: {}", e))?;
            }
            count
        };

        // 3. 处理设置导入
        let settings_imported = if !parsed.settings.hotkey.is_empty() {
            crate::clipboard::services::save_app_settings_logic(&app, parsed.settings)?;
            true
        } else {
            false
        };

        Ok((imported_count, settings_imported))
    })
    .await
    .map_err(|e| e.to_string())?
}
