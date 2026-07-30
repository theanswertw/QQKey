//! 前端可呼叫的 IPC 指令。

use tauri::{Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

use crate::catalog::history::ImportReport;
use crate::catalog::{Candidate, EntryPage, EntryPatch, Source};
use crate::hotkey::LAUNCHER_LABEL;
use crate::state::{AppState, Settings};

const SETTINGS_LABEL: &str = "settings";

/// 一次最多回九筆，對應數字鍵 1–9。
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
    let _ = window.hide();
    crate::inject::inject_text(crate::template::injectable_prefix(&template))?;

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
    crate::hotkey::rebind(&app, &state.shortcut(), &shortcut)?;
    state.set_shortcut(&shortcut)?;
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
