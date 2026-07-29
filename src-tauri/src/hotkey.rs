//! 全域快捷鍵註冊與候選框的顯示／隱藏。
//!
//! 預設使用 Alt+Q。刻意避開 Alt+Space —— 那是 Windows 系統視窗選單的保留鍵，
//! 在 Windows Terminal 中需要額外改設定才能傳遞給應用程式。

use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

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
    } else {
        show_launcher(&window);
    }
}

pub fn show_launcher<R: Runtime>(window: &WebviewWindow<R>) {
    // 必須在 show 之前記錄——候選框一旦顯示就成了前景視窗
    crate::inject::remember_foreground();

    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit(EVENT_LAUNCHER_SHOWN, ());
}
