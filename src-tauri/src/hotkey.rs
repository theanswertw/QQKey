//! 全域快捷鍵註冊與候選框的顯示／隱藏。
//!
//! 預設使用 Alt+Q。刻意避開 Alt+Space —— 那是 Windows 系統視窗選單的保留鍵，
//! 在 Windows Terminal 中需要額外改設定才能傳遞給應用程式。

use std::str::FromStr;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::caret::{self, Area};

pub const LAUNCHER_LABEL: &str = "launcher";

/// 預設快捷鍵。格式是 `keyboard-types` 的 code 名稱，`Q` 要寫成 `KeyQ`。
pub const DEFAULT_SHORTCUT: &str = "Alt+KeyQ";

/// 開啟設定視窗的快捷鍵。
///
/// 刻意做成全域快捷鍵而不是候選框裡的按鍵：中文輸入法會攔截 `Ctrl+,`
/// 這類組合，把修飾鍵吃掉只留下一個全形逗號，經過 webview 的快捷鍵並不可靠。
pub const SETTINGS_SHORTCUT: &str = "Alt+Shift+KeyQ";

/// 候選框顯示時通知前端重設查詢字串與輸入焦點。
const EVENT_LAUNCHER_SHOWN: &str = "launcher:shown";

/// 不透明度改變時通知候選框覆寫 CSS 變數。
const EVENT_LAUNCHER_OPACITY: &str = "launcher:opacity";

pub fn parse(value: &str) -> Result<Shortcut, String> {
    Shortcut::from_str(value).map_err(|error| {
        crate::i18n::shortcut_parse_failed(crate::i18n::current(), value, &error.to_string())
    })
}

/// 註冊快捷鍵。回傳 Err 代表該組合已被其他程式佔用。
pub fn register<R: Runtime>(app: &AppHandle<R>, value: &str) -> Result<(), String> {
    let shortcut = parse(value)?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_launcher(app);
            }
        })
        .map_err(|error| {
            crate::i18n::shortcut_register_failed(
                crate::i18n::current(),
                value,
                &error.to_string(),
            )
        })
}

/// 註冊開啟設定視窗的快捷鍵。
pub fn register_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let shortcut = parse(SETTINGS_SHORTCUT)?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                if let Err(error) = crate::commands::show_settings_window(app) {
                    crate::trace("設定", &format!("開啟失敗：{error}"));
                }
            }
        })
        .map_err(|error| format!("註冊 {SETTINGS_SHORTCUT} 失敗：{error}"))
}

pub fn unregister<R: Runtime>(app: &AppHandle<R>, value: &str) {
    if let Ok(shortcut) = parse(value) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

/// 換綁快捷鍵。新的註冊失敗時會把舊的補回去，
/// 免得使用者輸入一個被佔用的組合之後就再也叫不出候選框。
pub fn rebind<R: Runtime>(app: &AppHandle<R>, old: &str, new: &str) -> Result<(), String> {
    // 先確認新的解析得過，免得白白解除舊的綁定
    parse(new)?;

    unregister(app, old);
    match register(app, new) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = register(app, old);
            Err(error)
        }
    }
}

/// 已顯示就收起，未顯示就叫出。快捷鍵按第二次可取消，不必伸手去按 Esc。
pub fn toggle_launcher<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(LAUNCHER_LABEL) else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        crate::inject::restore_target_focus();
    } else {
        show_launcher(&window);
    }
}

pub fn show_launcher<R: Runtime>(window: &WebviewWindow<R>) {
    // 兩者都必須在 show 之前完成——候選框一旦顯示就成了前景視窗，
    // 焦點也會從目標視窗移走，屆時就取不到它的 caret 了。
    crate::inject::remember_foreground();
    position_at_caret(window);

    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit(EVENT_LAUNCHER_SHOWN, ());
}

/// 把新的不透明度推給候選框，讓它就地覆寫 CSS 變數，不必重新啟動。
///
/// 指名 label 而不是全域 emit——設定視窗不需要這個事件。
/// 推送失敗只記錄不當成錯誤：候選框仍有可能還沒掛載完，
/// 而它掛載時會自己取一次初值，那才是保證正確的那條路。
pub fn notify_launcher_opacity<R: Runtime>(app: &AppHandle<R>, percent: u8) {
    if let Err(error) = app.emit_to(LAUNCHER_LABEL, EVENT_LAUNCHER_OPACITY, percent) {
        crate::trace("設定", &format!("推送不透明度失敗：{error}"));
    }
}

/// 把候選框移到輸入游標旁邊。
fn position_at_caret<R: Runtime>(window: &WebviewWindow<R>) {
    if try_position_at_caret(window).is_none() {
        // 沒有明確位置時置中，不要讓視窗停在系統層疊出來的隨機座標
        let _ = window.center();
    }
}

fn try_position_at_caret<R: Runtime>(window: &WebviewWindow<R>) -> Option<()> {
    let Some(target) = crate::inject::target_window() else {
        trace("沒有記錄到目標視窗");
        return None;
    };
    let Some(anchor) = caret::locate(target) else {
        trace("取不到 caret");
        return None;
    };
    let Ok(size) = window.outer_size() else {
        trace("取不到候選框尺寸");
        return None;
    };
    let Ok(Some(monitor)) = window.monitor_from_point(anchor.x as f64, anchor.bottom as f64) else {
        trace("取不到螢幕資訊");
        return None;
    };

    let origin = monitor.position();
    let extent = monitor.size();
    let area = Area {
        left: origin.x,
        top: origin.y,
        right: origin.x + extent.width as i32,
        bottom: origin.y + extent.height as i32,
    };

    let (x, y) = caret::place(anchor, (size.width as i32, size.height as i32), area);
    trace(&format!("anchor={anchor:?} → ({x}, {y})"));
    match window.set_position(PhysicalPosition::new(x, y)) {
        Ok(()) => Some(()),
        Err(error) => {
            trace(&format!("移動候選框失敗：{error}"));
            None
        }
    }
}

fn trace(message: &str) {
    crate::trace("定位", message);
}
