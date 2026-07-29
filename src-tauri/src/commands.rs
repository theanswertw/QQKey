//! 前端可呼叫的 IPC 指令。

use tauri::{Manager, Runtime, WebviewWindow};

use crate::hotkey::LAUNCHER_LABEL;

/// 收起候選框（Esc 或選定後）。
#[tauri::command]
pub fn hide_launcher<R: Runtime>(window: WebviewWindow<R>) {
    let _ = window.hide();
}

/// 選定候選項目：收起候選框，把命令填回原本的視窗，但不送出 Enter。
#[tauri::command]
pub fn accept_candidate<R: Runtime>(
    window: WebviewWindow<R>,
    template: String,
) -> Result<(), String> {
    let _ = window.hide();
    crate::inject::inject_text(crate::template::injectable_prefix(&template))
}

/// 由系統匣或設定入口開啟設定視窗。
#[tauri::command]
pub fn open_settings<R: Runtime>(app: tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("settings") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();

    // 設定視窗會蓋住候選框，兩者不需要同時存在
    if let Some(launcher) = app.get_webview_window(LAUNCHER_LABEL) {
        let _ = launcher.hide();
    }
}
