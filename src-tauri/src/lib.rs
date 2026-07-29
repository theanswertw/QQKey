mod commands;
mod hotkey;
mod inject;
mod template;

use tauri::{Manager, WindowEvent};

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
