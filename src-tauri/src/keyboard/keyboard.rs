use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

const KEY_STEP_MS: u64 = 2;

// ==========================================
// 辅助与初始化 (Initialization & Helpers)
// ==========================================

/// 创建一个 Enigo 实例 (适配 enigo 0.6.1 的 Settings API)
fn create_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("无法初始化键盘系统: {}", e))
}

/// 清除当前所有被按下的修饰键 (Ctrl, Alt, Shift, Meta)
pub fn clear_modifiers() -> Result<(), String> {
    let mut enigo = create_enigo()?;

    let _ = enigo.key(Key::Control, Direction::Release);
    let _ = enigo.key(Key::Alt, Direction::Release);
    let _ = enigo.key(Key::Shift, Direction::Release);
    let _ = enigo.key(Key::Meta, Direction::Release);

    // 给操作系统一小段缓冲时间来响应按键释放状态
    thread::sleep(Duration::from_millis(1));
    Ok(())
}

// ==========================================
// 核心模拟操作 (Simulation API)
// ==========================================

/// 模拟按下系统粘贴快捷键 (Windows/Linux: Ctrl+V, macOS: Cmd+V)
pub fn simulate_paste(delay_ms: u64) -> Result<(), String> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms));
    }

    let mut enigo = create_enigo()?;
    clear_modifiers()?;

    #[cfg(target_os = "macos")]
    {
        let _ = enigo.key(Key::Meta, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('v'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Meta, Direction::Release);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = enigo.key(Key::Control, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('v'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Control, Direction::Release);
    }

    Ok(())
}

/// 模拟按下系统复制快捷键 (Windows/Linux: Ctrl+C, macOS: Cmd+C)
pub fn simulate_copy(delay_ms: u64) -> Result<(), String> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms));
    }

    let mut enigo = create_enigo()?;
    clear_modifiers()?;

    #[cfg(target_os = "macos")]
    {
        let _ = enigo.key(Key::Meta, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('c'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Meta, Direction::Release);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = enigo.key(Key::Control, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('c'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Control, Direction::Release);
    }

    Ok(())
}

/// 直接模拟键盘逐字输入纯文本
#[allow(dead_code)]
pub fn type_text(text: &str, delay_ms: u64) -> Result<(), String> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms));
    }

    let mut enigo = create_enigo()?;
    clear_modifiers()?;

    enigo
        .text(text)
        .map_err(|e| format!("由于系统错误导致的输入失败: {}", e))?;
    Ok(())
}

/// 模拟全选快捷键 (系统适配版本)
pub fn simulate_select_all() -> Result<(), String> {
    let mut enigo = create_enigo()?;
    clear_modifiers()?;

    #[cfg(target_os = "macos")]
    {
        let _ = enigo.key(Key::Meta, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('a'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Meta, Direction::Release);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = enigo.key(Key::Control, Direction::Press);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Unicode('a'), Direction::Click);
        thread::sleep(Duration::from_millis(KEY_STEP_MS));
        let _ = enigo.key(Key::Control, Direction::Release);
    }

    Ok(())
}
