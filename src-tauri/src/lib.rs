mod caret;
mod commands;
mod hotkey;
mod inject;
mod template;

use tauri::{Manager, WindowEvent};

/// 開發時的診斷輸出。定位與注入牽涉一連串可能失敗的 Win32 呼叫，
/// 出問題時需要知道是在哪一步退出的。
pub(crate) fn trace(scope: &str, message: &str) {
    #[cfg(debug_assertions)]
    eprintln!("[QQKey] {scope}：{message}");
    #[cfg(not(debug_assertions))]
    {
        let _ = (scope, message);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::hide_launcher,
            commands::accept_candidate,
            commands::open_settings
        ])
        .setup(|app| {
            let handle = app.handle();

            inject::watch_foreground();

            if let Err(error) = hotkey::register(handle, hotkey::default_shortcut()) {
                // 快捷鍵被其他程式佔用時不該讓整個應用起不來，
                // M6 的設定畫面會提供重新綁定的入口。
                eprintln!("[QQKey] 全域快捷鍵註冊失敗（可能已被其他程式佔用）：{error}");
            }

            if let Some(launcher) = app.get_webview_window(hotkey::LAUNCHER_LABEL) {
                let window = launcher.clone();
                launcher.on_window_event(move |event| {
                    // 點到別的視窗就收起，行為與輸入法候選視窗一致
                    if let WindowEvent::Focused(false) = event {
                        let _ = window.hide();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
