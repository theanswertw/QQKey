//! 前端可呼叫的 IPC 指令。

use tauri::{Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

use crate::catalog::history::ImportReport;
use crate::catalog::{Candidate, EntryPage, EntryPatch, ImportPreview, Source};
use crate::hotkey::LAUNCHER_LABEL;
use crate::i18n::{self, Lang};
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
    std::fs::write(&path, &json)
        .map_err(|error| i18n::write_failed(i18n::current(), &path, &error.to_string()))?;
    Ok(count)
}

/// 從備份檔還原。會取代目前的全部資料，前端負責先取得確認。
#[tauri::command]
pub fn restore_from_file<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<AppState>,
    path: String,
) -> Result<usize, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|error| i18n::read_failed(i18n::current(), &path, &error.to_string()))?;
    let written = state.restore(&json)?;

    // `restore()` 覆寫的是整張 `meta`，包含 `language`——備份裡的語言設定跟著
    // 生效了，但系統匣、視窗標題與兩個 webview 都還記著舊的。從前這裡什麼副作用
    // 都不推，所以還原之後畫面與資料庫要到重新啟動才對得上。
    i18n::set_current(state.active_language());
    // 備份裡的條目沒有 keywords_all（衍生資料不進備份檔），這一步同時把它補回來
    state.resync_builtin()?;
    apply_language(&app, &state);

    Ok(written)
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
    crate::tray::refresh(&app, &shortcut, state.active_language());
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
    let lang = i18n::current();
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|error| i18n::no_log_dir(lang, &error.to_string()))?;
    // 資料夾要等第一次寫入才會出現，剛裝好就來點的話得先把它建出來
    std::fs::create_dir_all(&dir)
        .map_err(|error| i18n::create_log_dir_failed(lang, &error.to_string()))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|error| i18n::open_log_dir_failed(lang, &error.to_string()))?;
    Ok(())
}

/// 開啟外部連結——關於頁的 Email 與專案頁。
///
/// 比照 `open_log_dir` 交給 explorer，它會把連結轉給系統預設的處理程式
/// （瀏覽器、郵件軟體），不必為了兩個連結多帶一個 plugin 進來。
///
/// 收的是前端傳進來的字串，所以放行條件要自己守：explorer 拿到本機路徑會開
/// 檔案總管、拿到檔案會執行關聯程式，不設限等於在網頁那一端開了一道
/// 「開啟任意本機東西」的門。關於頁需要的只有 `https://` 與 `mailto:`。
#[tauri::command]
pub fn open_external(target: String) -> Result<(), String> {
    if !is_openable(&target) {
        return Err(i18n::link_not_allowed(i18n::current(), &target));
    }
    std::process::Command::new("explorer")
        .arg(&target)
        .spawn()
        .map_err(|error| i18n::open_link_failed(i18n::current(), &error.to_string()))?;
    Ok(())
}

/// `open_external` 的放行條件。抽出來是為了測得到——真正開起來的那一步會叫出
/// 瀏覽器，測不了。
fn is_openable(target: &str) -> bool {
    // 控制字元一律擋。夾了換行的參數 explorer 會怎麼解讀不好說，
    // 而合法的連結裡本來就不該有。
    if target.chars().any(char::is_control) {
        return false;
    }
    target.starts_with("https://") || target.starts_with("mailto:")
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

/// 兩個視窗掛載時取語系。
///
/// 刻意不共用 `get_settings`：這是 i18next 初始化的前置條件，擋在第一次 render
/// 之前，能少搬一整包 `Settings` 就少搬一包。理由同 `launcher_opacity`。
#[tauri::command]
pub fn active_language(state: State<AppState>) -> Lang {
    state.active_language()
}

/// 換介面語言。`language` 收 `"auto"` 或某個語系標籤。
///
/// 比照 `set_shortcut` 與 `set_launcher_opacity`：先寫進資料庫落地，
/// 後面每一個副作用都讀新值。
#[tauri::command]
pub fn set_language<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<AppState>,
    language: String,
) -> Result<(), String> {
    state.set_language(&language)?;
    // 內建目錄換說明文字並重載候選池。放在系統匣之前是因為它是唯一可能失敗的
    // 一步，失敗時不想留下「選單換了、候選框沒換」的半套狀態。
    state.resync_builtin()?;
    apply_language(&app, &state);
    Ok(())
}

/// 把當前語系推到所有記著舊語言的地方。
///
/// 換語言與從備份還原都走這裡——後者會覆寫整張 `meta`（含 `language`），
/// 兩件事要做的善後完全一樣，分成兩份遲早有一份漏掉。
fn apply_language<R: Runtime>(app: &tauri::AppHandle<R>, state: &AppState) {
    let lang = state.active_language();
    crate::tray::refresh(app, &state.active_shortcut(), lang);
    // 設定視窗的標題是建立當下寫進去的，之後只能補一次
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.set_title(&i18n::settings_window_title(lang));
    }
    notify_language(app, lang);
}

/// 把新語系推給兩個 webview。
///
/// 跟 `notify_launcher_opacity` 不同，這裡用全域 `emit` 而不是 `emit_to`：
/// 不透明度只有候選框在意，語系是兩個視窗都在意——**包含發起這次改動的設定
/// 視窗自己**。讓它也走事件，套用語系就只有一條路徑，兩個視窗不可能顯示成
/// 不同語言。
fn notify_language<R: Runtime>(app: &tauri::AppHandle<R>, lang: Lang) {
    if let Err(error) = app.emit(i18n::EVENT_LANGUAGE, lang) {
        crate::trace("語系", &format!("推送語系失敗：{error}"));
    }
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
            .title(i18n::settings_window_title(i18n::current()))
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

#[cfg(test)]
mod tests {
    use super::is_openable;

    #[test]
    fn allows_https_and_mailto() {
        assert!(
            is_openable("https://github.com/theanswertw/QQKey"),
            "專案頁應該開得起來"
        );
        assert!(
            is_openable("mailto:jeremy@jeremywen.com"),
            "Email 應該開得起來"
        );
    }

    #[test]
    fn rejects_local_paths_and_other_schemes() {
        assert!(
            !is_openable(r"C:\Windows\System32\cmd.exe"),
            "本機路徑不該經由這條路開啟"
        );
        assert!(!is_openable("file:///C:/"), "file: 不在放行名單內");
        assert!(!is_openable("http://example.com"), "未加密的 http 不放行");
    }

    #[test]
    fn rejects_control_chars_inside_an_allowed_scheme() {
        assert!(
            !is_openable("https://example.com\nC:\\Windows"),
            "開頭合法但夾帶控制字元的連結不該放行"
        );
    }
}
