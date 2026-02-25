use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipboardRecord {
    pub id: i64,
    pub content_type: String, // "text" | "image" | "html" | "link"
    pub content: String,
    pub image_data: Option<Vec<u8>>,
    pub is_favorite: bool,
    pub is_pinned: bool,
    pub source_app: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub hotkey_modifiers: u32,
    pub hotkey_key: u32,
    pub hotkey: String, // global shortcut like "Ctrl+Shift+V"
    pub theme: String,  // "light" | "dark" | "system"
    pub keep_days: i32,
    pub max_records: i32,
    pub auto_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey_modifiers: 0,
            hotkey_key: 0,
            hotkey: "Ctrl+Shift+V".to_string(),
            theme: "system".to_string(),
            keep_days: 1,
            max_records: 500,
            auto_start: false,
        }
    }
}
