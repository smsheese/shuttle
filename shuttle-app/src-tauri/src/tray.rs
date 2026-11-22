use crate::commands::APP_HANDLE;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

const TRAY_ID: &str = "main-tray";

fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let show_i = MenuItem::with_id(app, "tray-show", "Show Shuttle", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &separator, &quit_i])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .or_else(|| Image::from_bytes(include_bytes!("../icons/icon.png")).ok());

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Shuttle")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => show_main(app),
            "tray-quit" => {
                if let Some(state) = app.try_state::<crate::commands::AppState>() {
                    state.connectors.shutdown_all();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }

    let _tray = builder.build(app)?;
    Ok(())
}

#[tauri::command]
pub fn update_tray_unread(count: i64) -> Result<(), String> {
    let app = APP_HANDLE
        .lock()
        .clone()
        .ok_or_else(|| "app not ready".to_string())?;
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        window
            .set_overlay_icon(if count > 0 {
                Some(unread_overlay(count))
            } else {
                None
            })
            .map_err(|e| e.to_string())?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        window
            .set_badge_count(if count > 0 { Some(count) } else { None })
            .map_err(|e| e.to_string())?;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let tooltip = if count > 0 {
        format!("Shuttle ({count} unread)")
    } else {
        "Shuttle".to_string()
    };
    tray.set_tooltip(Some(tooltip)).map_err(|e| e.to_string())
}

// Windows taskbars support an overlay image rather than a native count badge.
#[cfg(target_os = "windows")]
fn unread_overlay(count: i64) -> Image<'static> {
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32i32 {
        for x in 0..32i32 {
            if (x - 15).pow(2) + (y - 15).pow(2) <= 15 * 15 {
                let i = ((y * 32 + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[220, 38, 38, 255]);
            }
        }
    }
    let scale = 2usize;
    let start = (32 - (label.len() * 4 - 1) * scale) / 2;
    for (n, c) in label.chars().enumerate() {
        let glyph = if c == '+' {
            [0, 2, 7, 2, 0]
        } else {
            DIGITS[c.to_digit(10).unwrap() as usize]
        };
        for (y, row) in glyph.iter().enumerate() {
            for x in 0..3 {
                if row & (1 << (2 - x)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = start + n * 4 * scale + x * scale + dx;
                            let py = 11 + y * scale + dy;
                            let i = (py * 32 + px) * 4;
                            pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                        }
                    }
                }
            }
        }
    }
    Image::new_owned(pixels, 32, 32)
}
