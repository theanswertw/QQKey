//! 前端可呼叫的 IPC 指令。

use tauri::{Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

use crate::catalog::history::ImportReport;
use crate::catalog::{Candidate, EntryPage, EntryPatch, ImportPreview, Source};
use crate::hotkey::LAUNCHER_LABEL;
use crate::state::{AppState, Settings};

const SETTINGS_LABEL: &str = "settings";

/// 一次最多回九筆，對應直選鍵 Alt+1–9。
const MAX_CANDIDATES: usize = 9;

/// 依查詢字串取得候選命令。查詢為空時回傳最常用的幾筆。
#[tauri::command]
pub fn search_candidates(state: State<AppState>, query: String) -> Vec<Candidate> {
    state.search(&query, MAX_CANDIDATES)
}

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
    state: State<AppState>,
    id: i64,
) -> Result<(), String> {
    let template = state.template_of(id)?;

    // 成功路徑維持原樣——先收起候選框，焦點才回得去，這個順序是調過的。
    // 但失敗時要把框叫回來：以系統管理員身分開的終端機會擋下 SendInput，
    // 而那正是 usbipd 這類命令的日常情境。框收了又沒有字，使用者只會
    // 以為工具壞了。
    let text = crate::template::sanitize(crate::template::injectable_prefix(&template));

    let _ = window.hide();
    if let Err(error) = crate::inject::inject_text(&text) {
        crate::trace("注入", &format!("失敗：{error}"));
        let _ = window.show();
        let _ = window.set_focus();
        return Err(error);
    }

    // 注入成功才算數，免得把失敗的嘗試也拉高排序
    state.record_use(id)
}

// ------------------------------------------------------------ 設定畫面：條目

/// 列出條目供編輯，含停用中的。`source` 傳 null 表示不篩選。
#[tauri::command]
pub fn list_entries(
    state: State<AppState>,
    query: String,
    source: Option<Source>,
    offset: usize,
    limit: usize,
) -> Result<EntryPage, String> {
    state.list_entries(&query, source, offset, limit)
}

#[tauri::command]
pub fn create_entry(state: State<AppState>, patch: EntryPatch) -> Result<i64, String> {
    state.create_entry(&patch)
}

#[tauri::command]
pub fn update_entry(state: State<AppState>, id: i64, patch: EntryPatch) -> Result<(), String> {
    state.update_entry(id, &patch)
}

#[tauri::command]
pub fn delete_entry(state: State<AppState>, id: i64) -> Result<(), String> {
    state.delete_entry(id)
}

/// 批次刪除，回傳實際刪掉的筆數。前端負責先取得使用者確認。
#[tauri::command]
pub fn delete_entries(state: State<AppState>, ids: Vec<i64>) -> Result<usize, String> {
    state.delete_entries(&ids)
}

#[tauri::command]
pub fn set_entries_enabled(
    state: State<AppState>,
    ids: Vec<i64>,
    enabled: bool,
) -> Result<usize, String> {
    state.set_enabled(&ids, enabled)
}

#[tauri::command]
pub fn reset_entry_score(state: State<AppState>, id: i64) -> Result<(), String> {
    state.reset_score(id)
}

#[tauri::command]
pub fn export_entries(state: State<AppState>) -> Result<String, String> {
    state.export_entries()
}

#[tauri::command]
pub fn import_entries(state: State<AppState>, json: String) -> Result<usize, String> {
    state.import_entries(&json)
}

/// 匯入前的試算，讓使用者知道會新增幾筆、覆寫幾筆。
#[tauri::command]
pub fn preview_import(state: State<AppState>, json: String) -> Result<ImportPreview, String> {
    state.preview_import(&json)
}

/// 完整備份到檔案。路徑由前端的存檔對話框選定。
///
/// 檔案由後端寫而不是把 JSON 送回前端再寫——備份可能有上千筆，
/// 沒必要在 IPC 邊界上來回搬一大包字串。
#[tauri::command]
pub fn backup_to_file(state: State<AppState>, path: String) -> Result<usize, String> {
    let (json, count) = state.backup()?;
    std::fs::write(&path, &json).map_err(|error| format!("寫入 {path} 失敗：{error}"))?;
    Ok(count)
}

/// 從備份檔還原。會取代目前的全部資料，前端負責先取得確認。
#[tauri::command]
pub fn restore_from_file(state: State<AppState>, path: String) -> Result<usize, String> {
    let json =
        std::fs::read_to_string(&path).map_err(|error| format!("讀取 {path} 失敗：{error}"))?;
    state.restore(&json)
}

// ------------------------------------------------------------ 設定畫面：一般設定

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings()
}

/// 換綁全域快捷鍵。註冊失敗時舊的會被補回去，設定也不會被寫入。
#[tauri::command]
pub fn set_shortcut<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    // 解除的必須是「目前真正註冊著的」而不是「設定裡寫的」——啟動時若因為
    // 被佔用而退回了預設，兩者並不相同，拿設定值去解除會讓退回註冊的那個
    // 賴在系統裡，使用者從此設不回預設值。
    crate::hotkey::rebind(&app, &state.active_shortcut(), &shortcut)?;
    state.set_shortcut(&shortcut)?;
    state.set_active_shortcut(&shortcut);
    crate::tray::refresh_tooltip(&app, &shortcut);
    Ok(())
}

/// 是否已設定開機自動啟動。
#[tauri::command]
pub fn autostart_enabled<R: Runtime>(app: tauri::AppHandle<R>) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_autostart<R: Runtime>(app: tauri::AppHandle<R>, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| error.to_string())
}

/// 開啟日誌資料夾。
///
/// 日誌是出問題時唯一交得出來的東西，但它躺在 `%LOCALAPPDATA%` 底下，
/// 要使用者自己找不切實際，所以給一個點得到的入口。
#[tauri::command]
pub fn open_log_dir<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("取不到日誌資料夾位置：{error}"))?;
    // 資料夾要等第一次寫入才會出現，剛裝好就來點的話得先把它建出來
    std::fs::create_dir_all(&dir).map_err(|error| format!("建立日誌資料夾失敗：{error}"))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|error| format!("開啟日誌資料夾失敗：{error}"))?;
    Ok(())
}

#[tauri::command]
pub fn set_secret_pattern(state: State<AppState>, pattern: String) -> Result<(), String> {
    state.set_secret_pattern(&pattern)
}

/// 候選框掛載時取初值。
///
/// 刻意不共用 `get_settings`——候選框只需要這一個數字，不必認識整個 `Settings`。
/// CSS 裡寫的那個不透明度只在取到這個值之前撐著，所以這條路不是備援而是必要的。
#[tauri::command]
pub fn launcher_opacity(state: State<AppState>) -> u8 {
    state.launcher_opacity()
}

/// 更新候選框背景不透明度。
///
/// 比照 `set_shortcut`：先寫進資料庫，再把副作用推到別處——這裡是把新值
/// 推給候選框，讓它就地覆寫 CSS 變數，不必等到重新啟動。
#[tauri::command]
pub fn set_launcher_opacity<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<AppState>,
    percent: u8,
) -> Result<(), String> {
    state.set_launcher_opacity(percent)?;
    crate::hotkey::notify_launcher_opacity(&app, percent);
    Ok(())
}

/// 手動觸發一次歷史匯入，回傳這次掃描與過濾的統計。
#[tauri::command]
pub fn import_history(state: State<AppState>) -> Result<ImportReport, String> {
    state.import_history()
}

#[tauri::command]
pub fn set_history_import_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.set_history_import_enabled(enabled)
}

/// 由系統匣或設定入口開啟設定視窗。
///
/// 設定視窗刻意不在 `tauri.conf.json` 裡宣告，而是等到真的要開才建立——
/// 即使設為 `visible: false`，它在啟動時仍會被 Windows 當成前景視窗，
/// 害候選框把它誤認為要注入的目標。
#[tauri::command]
pub fn open_settings<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    show_settings_window(&app)
}

/// 顯示設定視窗，沒有就建立一個。全域快捷鍵與 IPC 都走這裡。
pub fn show_settings_window<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    match app.get_webview_window(SETTINGS_LABEL) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        None => {
            let window = WebviewWindowBuilder::new(
                app,
                SETTINGS_LABEL,
                WebviewUrl::App("settings.html".into()),
            )
            .title("QQKey 設定")
            .inner_size(960.0, 680.0)
            .min_inner_size(720.0, 480.0)
            .build()
            .map_err(|error| error.to_string())?;

            // 關掉設定視窗只是收起來。QQKey 是常駐工具，關窗不等於結束，
            // 保留視窗也省得下次開啟又重新載入一次。
            let handle = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.hide();
                }
            });
        }
    }

    // 設定視窗會蓋住候選框，兩者不需要同時存在
    if let Some(launcher) = app.get_webview_window(LAUNCHER_LABEL) {
        let _ = launcher.hide();
    }
    Ok(())
}
