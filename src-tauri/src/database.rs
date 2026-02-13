use std::path::PathBuf;
use rusqlite::{Connection, OptionalExtension, Result, params};
use crate::models::{ClipboardRecord, Settings};

const DB_FILE: &str = "clipper.db";
pub const MIN_MENU_WIDTH: i32 = 280;
pub const MIN_MENU_HEIGHT: i32 = 430;

#[cfg(target_os = "windows")]
fn legacy_windows_db_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|data_dir| PathBuf::from(data_dir).join("Clipper").join(DB_FILE))
}

#[cfg(target_os = "windows")]
fn get_db_path() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            return parent.join(DB_FILE);
        }
    }

    // Fallback for unusual runtime environments.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push(DB_FILE);
    path
}

#[cfg(not(target_os = "windows"))]
fn get_db_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push(DB_FILE);
    path
}

fn get_db_connection() -> Result<Connection, rusqlite::Error> {
    let path = get_db_path();

    // 确保目录存在
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    #[cfg(target_os = "windows")]
    {
        // One-time migration from legacy APPDATA location.
        if !path.exists() {
            if let Some(old_path) = legacy_windows_db_path() {
                if old_path.exists() && old_path != path {
                    let _ = std::fs::copy(old_path, &path);
                }
            }
        }
    }

    Connection::open(path)
}

fn ensure_settings_columns(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(settings)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }

    if !columns.iter().any(|c| c == "hotkey") {
        conn.execute(
            "ALTER TABLE settings ADD COLUMN hotkey TEXT DEFAULT 'Ctrl+Shift+V'",
            [],
        )?;
    }

    if !columns.iter().any(|c| c == "menu_width") {
        conn.execute(
            "ALTER TABLE settings ADD COLUMN menu_width INTEGER DEFAULT 400",
            [],
        )?;
    }

    if !columns.iter().any(|c| c == "menu_height") {
        conn.execute(
            "ALTER TABLE settings ADD COLUMN menu_height INTEGER DEFAULT 500",
            [],
        )?;
    }

    if !columns.iter().any(|c| c == "auto_start") {
        conn.execute(
            "ALTER TABLE settings ADD COLUMN auto_start INTEGER DEFAULT 0",
            [],
        )?;
    }

    Ok(())
}

fn ensure_history_columns(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare("PRAGMA table_info(clipboard_history)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }

    if !columns.iter().any(|c| c == "is_favorite") {
        conn.execute(
            "ALTER TABLE clipboard_history ADD COLUMN is_favorite INTEGER DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|c| c == "is_pinned") {
        conn.execute(
            "ALTER TABLE clipboard_history ADD COLUMN is_pinned INTEGER DEFAULT 0",
            [],
        )?;
    }

    Ok(())
}

fn sanitize_settings(settings: &Settings) -> Settings {
    Settings {
        hotkey_modifiers: settings.hotkey_modifiers,
        hotkey_key: settings.hotkey_key,
        hotkey: if settings.hotkey.trim().is_empty() {
            "Ctrl+Shift+V".to_string()
        } else {
            settings.hotkey.trim().to_string()
        },
        theme: settings.theme.clone(),
        keep_days: settings.keep_days.max(1),
        max_records: settings.max_records.max(50),
        menu_width: settings.menu_width.max(MIN_MENU_WIDTH),
        menu_height: settings.menu_height.max(MIN_MENU_HEIGHT),
        auto_start: settings.auto_start,
    }
}

fn get_settings_from_conn(conn: &Connection) -> Result<Settings, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT
            hotkey_modifiers,
            hotkey_key,
            COALESCE(hotkey, 'Ctrl+Shift+V') as hotkey,
            theme,
            keep_days,
            max_records,
            menu_width,
            menu_height,
            auto_start
         FROM settings WHERE id = 1",
    )?;
    let mut rows = stmt.query([])?;

    if let Some(row) = rows.next()? {
        Ok(Settings {
            hotkey_modifiers: row.get(0)?,
            hotkey_key: row.get(1)?,
            hotkey: row.get(2)?,
            theme: row.get(3)?,
            keep_days: row.get(4)?,
            max_records: row.get(5)?,
            menu_width: row.get(6)?,
            menu_height: row.get(7)?,
            auto_start: row.get::<_, i32>(8)? > 0,
        })
    } else {
        Ok(Settings::default())
    }
}

fn apply_retention_policy(conn: &Connection, settings: &Settings) -> Result<(), rusqlite::Error> {
    let keep_days = settings.keep_days.max(1);
    let max_records = settings.max_records.max(50);
    let days_expr = format!("-{} days", keep_days);

    conn.execute(
        "DELETE FROM clipboard_history
         WHERE COALESCE(is_favorite, 0) = 0
           AND julianday(created_at) < julianday('now', ?1)",
        params![days_expr],
    )?;

    conn.execute(
        "DELETE FROM clipboard_history
         WHERE COALESCE(is_favorite, 0) = 0
           AND id NOT IN (
            SELECT id FROM clipboard_history
            WHERE COALESCE(is_favorite, 0) = 0
            ORDER BY created_at DESC, id DESC
            LIMIT ?1
         )",
        params![max_records],
    )?;

    Ok(())
}

pub fn init_database() -> Result<()> {
    let conn = get_db_connection()?;

    // 创建剪贴板历史表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS clipboard_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content_type TEXT NOT NULL,
            content TEXT,
            image_data BLOB,
            is_favorite INTEGER DEFAULT 0,
            is_pinned INTEGER DEFAULT 0,
            source_app TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    // 创建设置表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            id INTEGER PRIMARY KEY,
            hotkey_modifiers INTEGER DEFAULT 0,
            hotkey_key INTEGER DEFAULT 0,
            hotkey TEXT DEFAULT 'Ctrl+Shift+V',
            theme TEXT DEFAULT 'system',
            keep_days INTEGER DEFAULT 30,
            max_records INTEGER DEFAULT 500,
            menu_width INTEGER DEFAULT 400,
            menu_height INTEGER DEFAULT 500,
            auto_start INTEGER DEFAULT 0
        )",
        [],
    )?;

    // 创建索引
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_created_at ON clipboard_history(created_at DESC)",
        [],
    )?;

    // 初始化默认设置
    conn.execute(
        "INSERT OR IGNORE INTO settings (id) VALUES (1)",
        [],
    )?;

    ensure_settings_columns(&conn)?;
    ensure_history_columns(&conn)?;

    Ok(())
}

pub fn get_history(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;

    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                NULL as image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC
         LIMIT ?1 OFFSET ?2"
    )?;

    let records = stmt.query_map(params![limit, offset], |row| {
        Ok(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(records)
}

pub fn search_history(keyword: &str, limit: i32) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;
    let pattern = format!("%{}%", keyword);

    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                NULL as image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         WHERE content LIKE ?1 OR source_app LIKE ?1
         ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC
         LIMIT ?2"
    )?;

    let records = stmt.query_map(params![pattern, limit], |row| {
        Ok(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(records)
}

pub fn get_favorite_history(limit: i32, offset: i32) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;

    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                NULL as image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         WHERE COALESCE(is_favorite, 0) = 1
         ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC
         LIMIT ?1 OFFSET ?2"
    )?;

    let records = stmt.query_map(params![limit, offset], |row| {
        Ok(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(records)
}

pub fn get_all_favorite_history() -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;

    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                NULL as image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         WHERE COALESCE(is_favorite, 0) = 1
         ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC"
    )?;

    let records = stmt.query_map([], |row| {
        Ok(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(records)
}

pub fn search_favorite_history(keyword: &str, limit: i32) -> Result<Vec<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;
    let pattern = format!("%{}%", keyword);

    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                NULL as image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         WHERE COALESCE(is_favorite, 0) = 1
           AND (content LIKE ?1 OR source_app LIKE ?1)
         ORDER BY COALESCE(is_pinned, 0) DESC, created_at DESC
         LIMIT ?2"
    )?;

    let records = stmt.query_map(params![pattern, limit], |row| {
        Ok(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(records)
}

pub fn add_record(record: ClipboardRecord) -> Result<i64, rusqlite::Error> {
    let conn = get_db_connection()?;

    // 文本类记录去重：命中则更新时间并复用原记录，保持列表简洁。
    let dedup_target = record.content_type != "image" && !record.content.trim().is_empty();
    if dedup_target {
        let existing = conn
            .query_row(
                "SELECT id,
                        COALESCE(is_favorite, 0) as is_favorite,
                        COALESCE(is_pinned, 0) as is_pinned
                 FROM clipboard_history
                 WHERE content_type = ?1 AND content = ?2
                 ORDER BY COALESCE(is_favorite, 0) DESC,
                          COALESCE(is_pinned, 0) DESC,
                          created_at DESC,
                          id DESC
                 LIMIT 1",
                params![&record.content_type, &record.content],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, i32>(2)?,
                    ))
                },
            )
            .optional()?;

        if let Some((keep_id, keep_favorite, keep_pinned)) = existing {
            let merged_favorite = keep_favorite > 0 || record.is_favorite;
            let merged_pinned = keep_pinned > 0 || record.is_pinned;

            conn.execute(
                "UPDATE clipboard_history
                 SET created_at = ?1,
                     source_app = ?2,
                     is_favorite = ?3,
                     is_pinned = ?4
                 WHERE id = ?5",
                params![
                    &record.created_at,
                    &record.source_app,
                    merged_favorite as i32,
                    merged_pinned as i32,
                    keep_id
                ],
            )?;

            // 清理历史上可能已存在的重复文本记录，只保留一条。
            conn.execute(
                "DELETE FROM clipboard_history
                 WHERE content_type = ?1
                   AND content = ?2
                   AND id <> ?3",
                params![&record.content_type, &record.content, keep_id],
            )?;

            if let Ok(settings) = get_settings_from_conn(&conn) {
                let _ = apply_retention_policy(&conn, &settings);
            }
            return Ok(keep_id);
        }
    }

    conn.execute(
        "INSERT INTO clipboard_history (content_type, content, image_data, is_favorite, is_pinned, source_app, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            record.content_type,
            record.content,
            record.image_data,
            record.is_favorite as i32,
            record.is_pinned as i32,
            record.source_app,
            record.created_at,
        ],
    )?;

    if let Ok(settings) = get_settings_from_conn(&conn) {
        let _ = apply_retention_policy(&conn, &settings);
    }

    Ok(conn.last_insert_rowid())
}

pub fn get_record_by_id(id: i64) -> Result<Option<ClipboardRecord>, rusqlite::Error> {
    let conn = get_db_connection()?;
    let mut stmt = conn.prepare(
        "SELECT id, content_type, COALESCE(content, '') as content,
                image_data, COALESCE(is_favorite, 0) as is_favorite,
                COALESCE(is_pinned, 0) as is_pinned,
                COALESCE(source_app, '') as source_app,
                created_at
         FROM clipboard_history
         WHERE id = ?1
         LIMIT 1"
    )?;

    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ClipboardRecord {
            id: row.get(0)?,
            content_type: row.get(1)?,
            content: row.get(2)?,
            image_data: row.get(3)?,
            is_favorite: row.get::<_, i32>(4)? > 0,
            is_pinned: row.get::<_, i32>(5)? > 0,
            source_app: row.get(6)?,
            created_at: row.get(7)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn delete_record(id: i64) -> Result<usize, rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute("DELETE FROM clipboard_history WHERE id = ?1", [id])
}

pub fn clear_history() -> Result<usize, rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute("DELETE FROM clipboard_history", [])
}

pub fn clear_non_favorite_history() -> Result<usize, rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute(
        "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 0",
        [],
    )
}

pub fn clear_favorite_history() -> Result<usize, rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute(
        "DELETE FROM clipboard_history WHERE COALESCE(is_favorite, 0) = 1",
        [],
    )
}

pub fn set_record_favorite(id: i64, favorite: bool) -> Result<(), rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute(
        "UPDATE clipboard_history SET is_favorite = ?1 WHERE id = ?2",
        params![favorite as i32, id],
    )?;
    Ok(())
}

pub fn set_record_pinned(id: i64, pinned: bool) -> Result<(), rusqlite::Error> {
    let conn = get_db_connection()?;
    conn.execute(
        "UPDATE clipboard_history SET is_pinned = ?1 WHERE id = ?2",
        params![pinned as i32, id],
    )?;
    Ok(())
}

pub fn favorite_exists(content_type: &str, content: &str) -> Result<bool, rusqlite::Error> {
    let conn = get_db_connection()?;
    let exists: i32 = conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM clipboard_history
            WHERE COALESCE(is_favorite, 0) = 1
              AND content_type = ?1
              AND content = ?2
            LIMIT 1
        )",
        params![content_type, content],
        |row| row.get(0),
    )?;
    Ok(exists > 0)
}

pub fn get_settings() -> Result<Settings, rusqlite::Error> {
    let conn = get_db_connection()?;
    get_settings_from_conn(&conn)
}

pub fn save_settings(settings: &Settings) -> Result<(), rusqlite::Error> {
    let conn = get_db_connection()?;
    let settings = sanitize_settings(settings);

    conn.execute(
        "UPDATE settings SET
            hotkey_modifiers = ?1,
            hotkey_key = ?2,
            hotkey = ?3,
            theme = ?4,
            keep_days = ?5,
            max_records = ?6,
            menu_width = ?7,
            menu_height = ?8,
            auto_start = ?9
         WHERE id = 1",
        params![
            settings.hotkey_modifiers as i32,
            settings.hotkey_key as i32,
            settings.hotkey,
            settings.theme,
            settings.keep_days,
            settings.max_records,
            settings.menu_width,
            settings.menu_height,
            settings.auto_start as i32,
        ],
    )?;

    apply_retention_policy(&conn, &settings)?;

    Ok(())
}

pub fn save_menu_size(width: i32, height: i32) -> Result<(), rusqlite::Error> {
    let conn = get_db_connection()?;
    let width = width.max(MIN_MENU_WIDTH);
    let height = height.max(MIN_MENU_HEIGHT);

    conn.execute(
        "INSERT OR IGNORE INTO settings (id) VALUES (1)",
        [],
    )?;

    conn.execute(
        "UPDATE settings
         SET menu_width = ?1, menu_height = ?2
         WHERE id = 1",
        params![width, height],
    )?;

    Ok(())
}
