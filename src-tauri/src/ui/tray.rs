use tauri::image::Image;
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ICON_DATA: &[u8] = if cfg!(target_os = "windows") {
    include_bytes!("../../icons/icon.ico")
} else {
    include_bytes!("../../icons/32x32.png")
};

const MENU_ID_SETTINGS: &str = "settings";
const MENU_ID_ABOUT: &str = "about";
const MENU_ID_QUIT: &str = "quit";

/// 创建系统托盘，应在应用启动时调用（如 `setup` 钩子）。
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    // 创建菜单
    let settings =
        tauri::menu::MenuItem::with_id(app, MENU_ID_SETTINGS, "设置", true, None::<&str>)?;
    let about = tauri::menu::MenuItem::with_id(app, MENU_ID_ABOUT, "关于", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, MENU_ID_QUIT, "退出程序", true, None::<&str>)?;

    let menu = tauri::menu::MenuBuilder::new(app)
        .items(&[&settings, &about, &quit])
        .build()?;

    // 加载图标
    let icon = app.default_window_icon().cloned().unwrap_or_else(|| {
        let img = image::load_from_memory(TRAY_ICON_DATA).expect("无法解码内嵌托盘图标");
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        Image::new_owned(rgba.into_raw(), width, height)
    });

    // 构建托盘 (纯粹的事件发射器)
    let tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("SnapPaste")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_ID_SETTINGS => {
                let _ = crate::ui::window_manager::show_main_window(app);
                let _ = app.emit("open-settings", ());
            }
            MENU_ID_ABOUT => {
                let _ = crate::ui::window_manager::show_main_window(app);
                let _ = app.emit("open-about", ());
            }
            MENU_ID_QUIT => {
                crate::ui::window_manager::save_size_and_exit(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = crate::ui::window_manager::show_main_window_near_cursor(app);
            }
        })
        .build(app)?;

    app.manage(tray);
    Ok(())
}
