mod caret;
mod catalog;
mod commands;
mod hotkey;
mod inject;
mod ranking;
mod state;
mod store;
mod template;
mod tray;

use tauri::{Manager, WindowEvent};

use crate::state::AppState;
use crate::store::Store;

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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::search_candidates,
            commands::hide_launcher,
            commands::accept_candidate,
            commands::open_settings,
            commands::list_entries,
            commands::create_entry,
            commands::update_entry,
            commands::delete_entry,
            commands::set_entries_enabled,
            commands::reset_entry_score,
            commands::export_entries,
            commands::import_entries,
            commands::get_settings,
            commands::set_shortcut,
            commands::launcher_opacity,
            commands::set_launcher_opacity,
            commands::set_secret_pattern,
            commands::import_history,
            commands::set_history_import_enabled,
            commands::autostart_enabled,
            commands::set_autostart
        ])
        .setup(|app| {
            let handle = app.handle();

            let database = app.path().app_data_dir()?.join("qqkey.db");
            let state = AppState::load(Store::open(&database)?)?;

            if state.history_import_enabled() {
                match state.import_history() {
                    Ok(report) => trace(
                        "歷史",
                        &format!(
                            "掃描 {} 行，匯入 {} 筆，略過 {} 筆疑似含憑證、{} 筆雜訊；候選池共 {} 筆",
                            report.scanned,
                            report.imported,
                            report.skipped_secret,
                            report.skipped_noise,
                            state.pool_size()
                        ),
                    ),
                    // 匯入失敗不該擋住整個應用，內建目錄仍然可用
                    Err(error) => trace("歷史", &format!("匯入失敗：{error}")),
                }
            }

            inject::watch_foreground();

            if let Err(error) = hotkey::register_settings(handle) {
                eprintln!("[QQKey] {error}");
            }

            let shortcut = state.shortcut();
            if let Err(error) = tray::setup(handle, &shortcut) {
                // 系統匣是唯一的常駐入口，建不起來要讓使用者知道
                eprintln!("[QQKey] 系統匣建立失敗：{error}");
            }

            if let Err(error) = hotkey::register(handle, &shortcut) {
                // 快捷鍵被佔用時不該讓整個應用起不來——設定畫面可以改綁，
                // 但那扇門也得打得開，所以這裡只記錄不中止。
                eprintln!("[QQKey] {error}");
                if shortcut != hotkey::DEFAULT_SHORTCUT {
                    if let Err(error) = hotkey::register(handle, hotkey::DEFAULT_SHORTCUT) {
                        eprintln!("[QQKey] 退回預設快捷鍵也失敗：{error}");
                    }
                }
            }

            app.manage(state);

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
