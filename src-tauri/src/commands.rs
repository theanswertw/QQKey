//! 前端可呼叫的 IPC 指令。

use tauri::{Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::hotkey::LAUNCHER_LABEL;

const SETTINGS_LABEL: &str = "settings";

/// 取消（Esc）：收起候選框並把焦點還給原本的視窗。
#[tauri::command]
pub fn hide_launcher<R: Runtime>(window: WebviewWindow<R>) {
    let _ = window.hide();
    crate::inject::restore_target_focus();
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
///
/// 設定視窗刻意不在 `tauri.conf.json` 裡宣告，而是等到真的要開才建立——
/// 即使設為 `visible: false`，它在啟動時仍會被 Windows 當成前景視窗，
/// 害候選框把它誤認為要注入的目標。
#[tauri::command]
pub fn open_settings<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    match app.get_webview_window(SETTINGS_LABEL) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        None => {
            WebviewWindowBuilder::new(
                &app,
                SETTINGS_LABEL,
                WebviewUrl::App("settings.html".into()),
            )
            .title("QQKey 設定")
            .inner_size(960.0, 680.0)
            .min_inner_size(720.0, 480.0)
            .build()
            .map_err(|error| error.to_string())?;
        }
    }

    // 設定視窗會蓋住候選框，兩者不需要同時存在
    if let Some(launcher) = app.get_webview_window(LAUNCHER_LABEL) {
        let _ = launcher.hide();
    }
    Ok(())
}
