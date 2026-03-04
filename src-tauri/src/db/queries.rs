use crate::db::core::get_conn;
use crate::models::{ClipboardRecord, ContentType, Settings};
use chrono::Local;
use rusqlite::{params, OptionalExtension, Row};

// =============================================================================
// 数据映射助手
// =============================================================================

fn map_record_row(row: &Row<'_>) -> Result<ClipboardRecord, rusqlite::Error> {
    Ok(ClipboardRecord {
        id: row.get(0)?,
        content_type: row.get(1)?,
        content: row.get(2)?,
        image_data: row.get(3)?,
        is_favorite: row.get::<_, i32>(4)? > 0,
        is_pinned: row.get::<_, i32>(5)? > 0,
        created_at: row.get(6)?,
    })
}

// =============================================================================
// 设置管理与清理策略
// =============================================================================

/// 获取应用设置：直接解析 JSON
pub fn get_settings() -> Result<Settings, rusqlite::Error> {
    let conn = get_conn();
    let json_str: Result<String, _> =
        conn.query_row("SELECT config FROM app_settings WHERE id = 1", [], |row| {
            row.get(0)
        });

    match json_str {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
        Err(_) => Ok(Settings::default()),
    }
}

/// 保存应用设置
pub fn save_settings(settings: &Settings) -> Result<(), rusqlite::Error> {
    let conn = get_conn();
    let json_str = serde_json::to_string(settings).unwrap_or_default();

    conn.execute(
        "UPDATE app_settings SET config = ?1 WHERE id = 1",
        params![json_str],
    )?;

    apply_retention_policy(&conn, settings)?;
    Ok(())
}

fn apply_retention_policy(
    conn: &rusqlite::Connection,
    settings: &Settings,
) -> Result<(), rusqlite::Error> {
    let keep_days = settings.keep_days.max(0);
    let max_records = settings.max_records.max(0);

    if keep_days > 0 {
        conn.execute(
            "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 0 AND julianday(created_at) < julianday('now', ?1)",
            params![format!("-{} days", keep_days)],
        )?;
    }
    if max_records > 0 {
        conn.execute(
            "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 0 AND id NOT IN (SELECT id FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 0 ORDER BY created_at DESC, id DESC LIMIT ?1)",
            params![max_records],
        )?;
    }
    Ok(())
}

// =============================================================================
// 历史记录基础 CRUD
// =============================================================================

const SELECT_NO_IMAGE_PREFIX: &str =
    "SELECT id, content_type, COALESCE(content, '') as content, NULL as image_data, COALESCE(is_favorite, 0) as is_favorite, COALESCE(is_pinned, 0) as is_pinned, created_at FROM clipboard_history";

pub fn get_records(
    is_favorite: Option<bool>,
    limit: Option<i32>,
    offset: Option<i32>,
) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_conn();
    let mut sql = SELECT_NO_IMAGE_PREFIX.to_string();
    if let Some(fav) = is_favorite {
        sql.push_str(&format!(" WHERE COALESCE(is_favorite, 0) = {}", fav as i32));
    }
    sql.push_str(" ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC");
    if let Some(l) = limit {
        sql.push_str(&format!(" LIMIT {}", l));
    }
    if let Some(o) = offset {
        sql.push_str(&format!(" OFFSET {}", o));
    }

    let mut stmt = conn.prepare_cached(&sql)?;
    let records: Vec<ClipboardRecord> = stmt
        .query_map([], map_record_row)?
        .filter_map(Result::ok)
        .collect();
    Ok(records)
}

pub fn search_records(
    keyword: &str,
    is_favorite: Option<bool>,
    limit: i32,
) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_conn();
    let pattern = format!("%{}%", keyword.replace('%', "\\%").replace('_', "\\_"));
    let mut sql = format!(
        "{} WHERE content LIKE ?1 ESCAPE '\\'",
        SELECT_NO_IMAGE_PREFIX
    );
    if let Some(fav) = is_favorite {
        sql.push_str(&format!(" AND COALESCE(is_favorite, 0) = {}", fav as i32));
    }
    sql.push_str(" ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC LIMIT ?2");

    let mut stmt = conn.prepare_cached(&sql)?;
    let records: Vec<ClipboardRecord> = stmt
        .query_map(params![pattern, limit], map_record_row)?
        .filter_map(Result::ok)
        .collect();
    Ok(records)
}

/// 🚀 核心升级 3：基于事务的去重机制，杜绝数据损坏
pub fn add_record(record: ClipboardRecord) -> Result<i64, rusqlite::Error> {
    let mut conn = get_conn();
    let is_dedup_target =
        record.content_type != ContentType::Image && !record.content.trim().is_empty();
    let mut insert_id = 0;

    // 开启数据库事务，确保接下来的多步操作具有原子性（要么全成功，要么全失败回滚）
    let tx = conn.transaction()?;

    if is_dedup_target {
        let existing = tx.query_row(
            "SELECT id, COALESCE(is_favorite, 0), COALESCE(is_pinned, 0) 
             FROM clipboard_history 
             WHERE content_type = ?1 AND content = ?2 
             ORDER BY COALESCE(is_favorite, 0) DESC, COALESCE(is_pinned, 0) DESC, created_at DESC LIMIT 1",
            params![&record.content_type, &record.content],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i32>(1)?, row.get::<_, i32>(2)?)),
        ).optional()?;

        if let Some((keep_id, keep_favorite, keep_pinned)) = existing {
            let merged_favorite = keep_favorite > 0 || record.is_favorite;
            let merged_pinned = keep_pinned > 0 || record.is_pinned;

            tx.execute(
                "UPDATE clipboard_history SET created_at = ?1, is_favorite = ?2, is_pinned = ?3 WHERE id = ?4",
                params![&record.created_at, merged_favorite as i32, merged_pinned as i32, keep_id],
            )?;

            tx.execute(
                "DELETE FROM clipboard_history WHERE content_type = ?1 AND content = ?2 AND id <> ?3",
                params![&record.content_type, &record.content, keep_id],
            )?;

            insert_id = keep_id;
        }
    }

    if insert_id == 0 {
        tx.execute(
            "INSERT INTO clipboard_history (content_type, content, image_data, is_favorite, is_pinned, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![record.content_type, record.content, record.image_data, record.is_favorite as i32, record.is_pinned as i32, record.created_at]
        )?;
        insert_id = tx.last_insert_rowid();
    }

    tx.commit()?;

    // 脱离事务后应用清理策略
    if let Ok(settings) = get_settings() {
        let conn2 = get_conn();
        let _ = apply_retention_policy(&conn2, &settings);
    }

    Ok(insert_id)
}

pub fn get_record_by_id(id: i64) -> Result<Option<ClipboardRecord>, rusqlite::Error> {
    let conn = get_conn();
    let mut stmt = conn.prepare_cached(
        "SELECT id, content_type, COALESCE(content, '') as content, image_data, COALESCE(is_favorite, 0) as is_favorite, COALESCE(is_pinned, 0) as is_pinned, created_at 
         FROM clipboard_history WHERE id = ?1 LIMIT 1"
    )?;
    stmt.query_row(params![id], map_record_row).optional()
}

pub fn delete_record(id: i64) -> Result<usize, rusqlite::Error> {
    get_conn().execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])
}

pub fn clear_records(is_favorite: Option<bool>) -> Result<usize, rusqlite::Error> {
    let conn = get_conn();
    match is_favorite {
        Some(true) => conn.execute(
            "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 1",
            [],
        ),
        Some(false) => conn.execute(
            "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 0",
            [],
        ),
        None => conn.execute("DELETE FROM clipboard_history", []),
    }
}

pub fn update_record_status(
    id: i64,
    is_favorite: Option<bool>,
    is_pinned: Option<bool>,
) -> Result<(), rusqlite::Error> {
    let conn = get_conn();
    if let Some(fav) = is_favorite {
        conn.execute(
            "UPDATE clipboard_history SET is_favorite = ?1 WHERE id = ?2",
            params![fav as i32, id],
        )?;
    }
    if let Some(pin) = is_pinned {
        conn.execute(
            "UPDATE clipboard_history SET is_pinned = ?1 WHERE id = ?2",
            params![pin as i32, id],
        )?;
    }
    Ok(())
}

// =============================================================================
// 高阶封装层：向外提供统一签名的业务管道
// =============================================================================

pub fn get_history(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, String> {
    get_records(None, Some(limit), Some(offset)).map_err(|e| e.to_string())
}

pub fn search_history(keyword: &str, limit: i32) -> Result<Vec<ClipboardRecord>, String> {
    search_records(keyword, None, limit).map_err(|e| e.to_string())
}

pub fn get_favorites(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, String> {
    get_records(Some(true), Some(limit), Some(offset)).map_err(|e| e.to_string())
}

pub fn search_favorites(keyword: &str, limit: i32) -> Result<Vec<ClipboardRecord>, String> {
    search_records(keyword, Some(true), limit).map_err(|e| e.to_string())
}

pub fn delete_item(id: i64) -> Result<usize, String> {
    delete_record(id).map_err(|e| e.to_string())
}

pub fn clear_history_records() -> Result<usize, String> {
    clear_records(Some(false)).map_err(|e| e.to_string())
}

pub fn clear_favorite_records() -> Result<usize, String> {
    clear_records(Some(true)).map_err(|e| e.to_string())
}

pub fn toggle_favorite(id: i64, is_favorite: bool) -> Result<(), String> {
    update_record_status(id, Some(is_favorite), None).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn toggle_pinned(id: i64, is_pinned: bool) -> Result<(), String> {
    update_record_status(id, None, Some(is_pinned)).map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(dead_code)]
pub fn add_clipboard_record(record: ClipboardRecord) -> Result<i64, String> {
    add_record(record).map_err(|e| e.to_string())
}

// =============================================================================
// 定制化业务逻辑助手
// =============================================================================

/// 将一段内容封装为标准的收藏记录并存入数据库
pub fn add_custom_favorite_record_logic(content: String) -> Result<i64, String> {
    let text = content.trim();
    if text.is_empty() {
        return Err("content cannot be empty".to_string());
    }

    let record = ClipboardRecord {
        id: 0,
        content_type: ContentType::Text,
        content: text.to_string(),
        image_data: None,
        is_favorite: true,
        is_pinned: false,
        created_at: Local::now().to_rfc3339(),
    };

    add_record(record).map_err(|e| e.to_string())
}

/// 获取窗口保存的状态（宽、高）
pub fn get_window_state(label: &str) -> Result<Option<(u32, u32)>, rusqlite::Error> {
    let conn = get_conn();
    conn.query_row(
        "SELECT width, height FROM window_state WHERE label = ?1",
        [label],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

/// 保存窗口的状态（宽、高）
pub fn save_window_state(label: &str, width: u32, height: u32) -> Result<(), rusqlite::Error> {
    let conn = get_conn();
    conn.execute(
        "INSERT INTO window_state (label, width, height) VALUES (?1, ?2, ?3)
         ON CONFLICT(label) DO UPDATE SET width = ?2, height = ?3",
        params![label, width, height],
    )?;
    Ok(())
}
