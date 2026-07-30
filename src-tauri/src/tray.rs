//! 系統匣圖示與選單。
//!
//! QQKey 平常沒有可見視窗，系統匣是它唯一的常駐入口——
//! 也是使用者忘記快捷鍵時找得回來的地方。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

use crate::hotkey::SETTINGS_SHORTCUT;

pub fn setup<R: Runtime>(app: &AppHandle<R>, shortcut: &str) -> tauri::Result<()> {
    let show = MenuItem::with_id(
        app,
        "show",
        format!("叫出候選框（{}）", pretty(shortcut)),
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        format!("設定（{}）", pretty(SETTINGS_SHORTCUT)),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "結束 QQKey", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &separator, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::UnknownPath)?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip(format!("QQKey — {} 叫出候選框", pretty(shortcut)))
        .menu(&menu)
        // 左鍵留給「叫出候選框」，選單走右鍵
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => crate::hotkey::toggle_launcher(app),
            "settings" => {
                if let Err(error) = crate::commands::show_settings_window(app) {
                    crate::trace("設定", &format!("開啟失敗：{error}"));
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::hotkey::toggle_launcher(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// 把 `Alt+KeyQ` 這種內部表示法改寫成給人看的 `Alt+Q`。
fn pretty(shortcut: &str) -> String {
    shortcut
        .split('+')
        .map(|part| part.strip_prefix("Key").unwrap_or(part))
        .collect::<Vec<_>>()
        .join("+")
}

/// 快捷鍵改綁後同步系統匣上的提示文字。
pub fn refresh_tooltip<R: Runtime>(app: &AppHandle<R>, shortcut: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("QQKey — {} 叫出候選框", pretty(shortcut))));
    }
}

#[cfg(test)]
mod tests {
    use super::pretty;

    #[test]
    fn strips_the_key_prefix_for_display() {
        assert_eq!(pretty("Alt+KeyQ"), "Alt+Q");
        assert_eq!(pretty("Alt+Shift+KeyQ"), "Alt+Shift+Q");
        assert_eq!(pretty("Control+Space"), "Control+Space");
    }
}
