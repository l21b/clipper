use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    image::Image,
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime,
};
use image::load_from_memory;

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        crate::mark_main_window_shown();
        // 在窗口显示之前发送事件，让前端先设置滚动位置
        let _ = app.emit("main-window-opened", ());
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

pub fn show_main_window_near_cursor<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        crate::mark_main_window_shown();
        if let Ok(cursor) = window.cursor_position() {
            let mut x = cursor.x + 12.0;
            let mut y = cursor.y + 16.0;

            if let Ok(size) = window.outer_size() {
                let work_area = window
                    .available_monitors()
                    .ok()
                    .and_then(|monitors| {
                        monitors.into_iter().find(|m| {
                            let rect = m.work_area();
                            let left = rect.position.x as f64;
                            let top = rect.position.y as f64;
                            let right = left + rect.size.width as f64;
                            let bottom = top + rect.size.height as f64;
                            cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
                        })
                    })
                    .or_else(|| window.current_monitor().ok().flatten())
                    .map(|m| *m.work_area());

                if let Some(rect) = work_area {
                    let margin = 6.0;
                    let left = rect.position.x as f64 + margin;
                    let top = rect.position.y as f64 + margin;
                    let right = rect.position.x as f64 + rect.size.width as f64 - margin;
                    let bottom = rect.position.y as f64 + rect.size.height as f64 - margin;

                    // 默认显示在鼠标下方，靠近底部时改为在鼠标上方显示，避免压住任务栏
                    if y + size.height as f64 > bottom {
                        y = cursor.y - size.height as f64 - 12.0;
                    }

                    let max_x = (right - size.width as f64).max(left);
                    let max_y = (bottom - size.height as f64).max(top);
                    x = x.clamp(left, max_x);
                    y = y.clamp(top, max_y);
                } else {
                    x = x.max(0.0);
                    y = y.max(0.0);
                }
            }

            let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }

        // 在窗口显示之前发送事件，让前端先设置滚动位置
        let _ = app.emit("main-window-opened", ());
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

/// 创建系统托盘
pub fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // 创建菜单项
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "关于", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;

    // 创建菜单
    let menu = Menu::with_items(app, &[&settings, &about, &quit])?;

    // 托盘图标优先使用应用默认图标；若缺失则从文件解码。
    let icon = if let Some(default_icon) = app.default_window_icon() {
        default_icon.clone()
    } else {
        #[cfg(target_os = "windows")]
        let icon_data = include_bytes!("../icons/icon.ico").as_ref();
        #[cfg(not(target_os = "windows"))]
        let icon_data = include_bytes!("../icons/32x32.png").as_ref();
        let rgba = load_from_memory(icon_data)
            .expect("failed to decode tray icon bytes")
            .to_rgba8();
        let (width, height) = rgba.dimensions();
        Image::new_owned(rgba.into_raw(), width, height)
    };

    // 创建托盘
    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Clipper - 历史记录")
        .menu(&menu)
        .on_menu_event(move |app, event| {
            match event.id.as_ref() {
                "settings" => {
                    let _ = show_main_window(app);
                    let _ = app.emit("open-settings", ());
                }
                "about" => {
                    let _ = show_main_window(app);
                    let _ = app.emit("open-about", ());
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 左键点击托盘图标显示窗口
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event {
                let app_handle = tray.app_handle();
                let _ = show_main_window(app_handle);
            }
        })
        .build(app)?;

    Ok(())
}
