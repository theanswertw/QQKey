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

/// 診斷輸出。定位與注入牽涉一連串可能失敗的 Win32 呼叫，
/// 出問題時需要知道是在哪一步退出的。
///
/// 從前這裡在 release 版是空實作，但 release 版正是最需要它的場合——
/// 打包後沒有 console，出問題時手上會什麼都沒有。改為一律寫進日誌檔。
///
/// 寫進去的東西會留在磁碟上，所以**不記錄視窗標題、不記錄注入內容**，
/// 那等於把使用者一整天做過什麼存起來。只記錄「走到哪一步、為什麼退出」。
pub(crate) fn trace(scope: &str, message: &str) {
    log::info!("{scope}：{message}");
}

/// 日誌檔的位置由 tauri-plugin-log 決定，在 Windows 是
/// `%LOCALAPPDATA%\com.jeremywen.qqkey\logs\`——注意跟資料庫所在的
/// Roaming `%APPDATA%` 不是同一個地方。
fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_log::{Target, TargetKind};

    tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::Stdout))
        .target(Target::new(TargetKind::LogDir {
            file_name: Some("qqkey".into()),
        }))
        // 開發時要看得到定位與注入每一步，正式版只留得下結論
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .max_file_size(512 * 1024)
        .build()
}

/// 註冊全域快捷鍵，回傳真正生效的那一個（都失敗就是空字串）。
///
/// 快捷鍵被佔用不該讓整個工具起不來——設定畫面是唯一能改綁的地方，
/// 那扇門得打得開，所以這裡只記錄不中止。但**退回預設之後，設定值與
/// 實際生效的組合就分岔了**，呼叫端必須拿回傳值去更新狀態，
/// 否則之後換綁會去解除一個根本沒註冊成功的組合。
fn register_shortcut(app: &tauri::AppHandle, desired: &str) -> String {
    if let Err(error) = hotkey::register(app, desired) {
        log::error!("{error}");
    } else {
        return desired.to_string();
    }

    if desired == hotkey::DEFAULT_SHORTCUT {
        return String::new();
    }

    match hotkey::register(app, hotkey::DEFAULT_SHORTCUT) {
        Ok(()) => {
            log::warn!(
                "已改用預設快捷鍵 {}，設定裡的 {desired} 目前沒有生效",
                hotkey::DEFAULT_SHORTCUT
            );
            hotkey::DEFAULT_SHORTCUT.to_string()
        }
        Err(error) => {
            log::error!("退回預設快捷鍵也失敗：{error}");
            String::new()
        }
    }
}

/// 開啟資料庫並載入候選池。
///
/// 錯誤訊息要讓使用者自己判斷得出是什麼問題（磁碟滿？權限？檔案壞了？），
/// 所以連資料庫路徑一起帶出去——他至少能去那個位置看一眼。
fn load_state(app: &tauri::AppHandle) -> Result<AppState, String> {
    let database = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("取不到資料夾位置：{error}"))?
        .join("qqkey.db");

    let store = Store::open(&database)
        .map_err(|error| format!("開啟資料庫失敗：{error}\n\n{}", database.display()))?;

    AppState::load(store)
        .map_err(|error| format!("載入候選池失敗：{error}\n\n{}", database.display()))
}

/// 啟動失敗時的最後手段。
///
/// 這個時間點還沒有視窗、沒有系統匣，release 版也沒有 console，
/// 系統對話框是唯一講得出話的地方。
fn fatal_dialog(message: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text = HSTRING::from(message);
    let caption = HSTRING::from("QQKey 無法啟動");
    unsafe {
        MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR);
    }
}

/// 讓 panic 留下痕跡。不裝這個的話，release 版的 panic 就是
/// 程序無聲無息地消失——使用者只會說「它不見了」，而我們無從查起。
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("程式異常中止：{info}");
        previous(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    tauri::Builder::default()
        // 官方要求這個要排第一，趕在其他 plugin 動作之前把重複的實例攔下來。
        //
        // 沒有它的話，開機自啟加上使用者自己雙擊捷徑就會跑出第二份：兩個系統匣
        // 圖示、兩份各自獨立的記憶體候選池（在這邊新增的條目那邊看不到），
        // 而後啟動的那份必定搶不到全域快捷鍵。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window(hotkey::LAUNCHER_LABEL) {
                hotkey::show_launcher(&window);
            }
        }))
        // 日誌接著裝，後面每一個 plugin 的失敗才有地方可寫
        .plugin(log_plugin())
        .plugin(tauri_plugin_dialog::init())
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
            commands::delete_entries,
            commands::set_entries_enabled,
            commands::reset_entry_score,
            commands::export_entries,
            commands::import_entries,
            commands::preview_import,
            commands::backup_to_file,
            commands::restore_from_file,
            commands::get_settings,
            commands::set_shortcut,
            commands::launcher_opacity,
            commands::set_launcher_opacity,
            commands::set_secret_pattern,
            commands::import_history,
            commands::set_history_import_enabled,
            commands::autostart_enabled,
            commands::set_autostart,
            commands::open_log_dir
        ])
        .setup(|app| {
            let handle = app.handle();

            // 資料庫是整個工具的地基，開不起來就沒有候選池可搜。原本這裡
            // 兩個 `?` 會一路傳到 run() 的 expect——release 版沒有 console、
            // 沒有視窗，使用者按下捷徑只會覺得什麼都沒發生。
            let state = match load_state(handle) {
                Ok(state) => state,
                Err(message) => {
                    log::error!("{message}");
                    fatal_dialog(&message);
                    std::process::exit(1);
                }
            };

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
                log::error!("{error}");
            }

            // 先試著註冊，再讓系統匣去顯示——順序反過來的話，選單上會寫著
            // 一個其實按了沒反應的組合。
            let active = register_shortcut(handle, &state.shortcut());
            state.set_active_shortcut(&active);

            if let Err(error) = tray::setup(handle, &active) {
                // 系統匣是唯一的常駐入口，建不起來要讓使用者知道
                log::error!("系統匣建立失敗：{error}");
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
