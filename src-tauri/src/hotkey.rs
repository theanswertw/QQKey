//! 全域快捷鍵註冊與候選框的顯示／隱藏。
//!
//! 預設使用 Alt+Q。刻意避開 Alt+Space —— 那是 Windows 系統視窗選單的保留鍵，
//! 在 Windows Terminal 中需要額外改設定才能傳遞給應用程式。

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::caret::{self, Area};

pub const LAUNCHER_LABEL: &str = "launcher";

/// 候選框顯示時通知前端重設查詢字串與輸入焦點。
const EVENT_LAUNCHER_SHOWN: &str = "launcher:shown";

pub fn default_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::KeyQ)
}

/// 註冊快捷鍵。回傳 Err 代表該組合已被其他程式佔用，呼叫端應提示使用者改綁。
pub fn register<R: Runtime>(app: &AppHandle<R>, shortcut: Shortcut) -> tauri::Result<()> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_launcher(app);
            }
        })
        .map_err(|error| tauri::Error::Anyhow(error.into()))
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
