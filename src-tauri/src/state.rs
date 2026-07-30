//! 應用程式共用狀態：資料庫與記憶體中的候選池。

use std::cmp::Ordering;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::RwLock;

use crate::catalog::history::{self, ImportReport, SecretFilter};
use crate::catalog::{
    load_builtin, Candidate, Entry, EntryPage, EntryPatch, EntryView, SharedEntry, SharedFile,
    Source, SHARED_FILE_VERSION,
};
use crate::ranking;
use crate::store::Store;

/// 上次讀到歷史檔的哪個位元組。下次從這裡接著讀，不必重掃整個檔案。
const META_HISTORY_OFFSET: &str = "history_offset";
/// 是否啟用歷史匯入。
const META_HISTORY_IMPORT: &str = "history_import";
/// 全域快捷鍵。
const META_SHORTCUT: &str = "shortcut";
/// 歷史匯入的機密關鍵字樣式。
const META_SECRET_PATTERN: &str = "secret_pattern";
/// 候選框背景不透明度，以百分比整數字串存放。
const META_LAUNCHER_OPACITY: &str = "launcher_opacity";

/// 候選框背景預設不透明度。留一點透視感，又不至於讓命令文字難讀。
const DEFAULT_LAUNCHER_OPACITY: u8 = 92;
/// 下限刻意不開到 0——候選框幾乎看不見時，使用者會以為程式壞了，
/// 而不是想起自己把它調透明了。
const MIN_LAUNCHER_OPACITY: u8 = 20;
const MAX_LAUNCHER_OPACITY: u8 = 100;

/// 設定畫面一次取得的所有一般設定。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub shortcut: String,
    pub history_import: bool,
    pub secret_pattern: String,
    /// 讓設定畫面能提供「還原預設」
    pub default_secret_pattern: String,
    /// 候選框背景不透明度（百分比整數）
    pub launcher_opacity: u8,
    /// 讓設定畫面能提供「還原預設」
    pub default_launcher_opacity: u8,
    pub pool_size: usize,
}

pub struct AppState {
    store: Store,
    /// 啟用中的條目全載入記憶體。候選池頂多幾千筆，
    /// 這樣每敲一個字的搜尋都不必再碰資料庫。
    pool: RwLock<Vec<Entry>>,
}

impl AppState {
    /// 同步內建目錄後載入候選池。
    pub fn load(store: Store) -> rusqlite::Result<Self> {
        let written = store.sync_builtin(&load_builtin())?;
        let pool = store.load_enabled()?;
        crate::trace(
            "目錄",
            &format!("內建目錄寫入 {written} 筆，候選池共 {} 筆", pool.len()),
        );
        Ok(Self {
            store,
            pool: RwLock::new(pool),
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Candidate> {
        let pool = self.pool.read().unwrap();
        ranking::rank(&pool, query, ranking::now(), limit)
            .into_iter()
            .map(Candidate::from)
            .collect()
    }

    pub fn template_of(&self, id: i64) -> Result<String, String> {
        self.store
            .find_template(id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("找不到 id 為 {id} 的命令"))
    }

    /// 記下一次使用。記憶體中的候選池也要同步更新，
    /// 否則排序要等到下次啟動才會反映。
    pub fn record_use(&self, id: i64) -> Result<(), String> {
        let now = ranking::now();
        let score = self
            .store
            .record_use(id, now)
            .map_err(|error| error.to_string())?;

        if let Some(entry) = self
            .pool
            .write()
            .unwrap()
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            entry.score = score;
            entry.last_used = Some(now);
        }
        Ok(())
    }

    /// 歷史匯入是否啟用。預設開啟——「自動學習歷史紀錄」本來就是選定的命令來源之一。
    pub fn history_import_enabled(&self) -> bool {
        self.store
            .meta(META_HISTORY_IMPORT)
            .ok()
            .flatten()
            .map(|value| value != "off")
            .unwrap_or(true)
    }

    pub fn set_history_import_enabled(&self, enabled: bool) -> Result<(), String> {
        self.store
            .set_meta(META_HISTORY_IMPORT, if enabled { "on" } else { "off" })
            .map_err(|error| error.to_string())
    }

    /// 從上次讀到的位置繼續匯入 PSReadLine 歷史。
    pub fn import_history(&self) -> Result<ImportReport, String> {
        let Some(path) = history::history_path() else {
            return Ok(ImportReport::default());
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            // 沒有歷史檔（例如全新的機器）就當作沒東西可匯
            return Ok(ImportReport::default());
        };
        let size = metadata.len();

        let mut offset = self
            .store
            .meta(META_HISTORY_OFFSET)
            .map_err(|error| error.to_string())?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        // PSReadLine 超過保留筆數時會重寫整個檔案，檔案變短就代表要從頭讀
        if size < offset {
            offset = 0;
        }
        if size == offset {
            return Ok(ImportReport::default());
        }

        let text = read_from(&path, offset).map_err(|error| error.to_string())?;
        let (entries, report) = history::parse(&text, &self.secret_filter());

        self.store
            .import_history(&entries)
            .map_err(|error| error.to_string())?;
        self.store
            .set_meta(META_HISTORY_OFFSET, &size.to_string())
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;

        Ok(report)
    }

    pub fn pool_size(&self) -> usize {
        self.pool.read().unwrap().len()
    }

    // ---------------------------------------------------------- 設定畫面

    /// 列出條目（含停用中的）供設定畫面編輯。
    ///
    /// 直接查資料庫而不是走記憶體的候選池——候選池只有啟用中的條目，
    /// 而設定畫面正需要看到被停用的那些。
    pub fn list_entries(
        &self,
        query: &str,
        source: Option<Source>,
        offset: usize,
        limit: usize,
    ) -> Result<EntryPage, String> {
        let all = self.store.load_all().map_err(|error| error.to_string())?;
        let needle = query.trim().to_lowercase();

        let mut filtered: Vec<&Entry> = all
            .iter()
            .filter(|entry| source.is_none_or(|wanted| entry.source == wanted))
            .filter(|entry| needle.is_empty() || entry.haystack().to_lowercase().contains(&needle))
            .collect();

        // 自己整理的排前面，其次是常用的，最後才照命令字母序
        filtered.sort_by(|left, right| {
            right
                .source
                .priority()
                .cmp(&left.source.priority())
                .then_with(|| {
                    right
                        .score
                        .partial_cmp(&left.score)
                        .unwrap_or(Ordering::Equal)
                })
                .then_with(|| left.template.cmp(&right.template))
        });

        Ok(EntryPage {
            total: filtered.len(),
            entries: filtered
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(EntryView::from)
                .collect(),
        })
    }

    pub fn create_entry(&self, patch: &EntryPatch) -> Result<i64, String> {
        let id = self
            .store
            .create_entry(patch)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(id)
    }

    pub fn update_entry(&self, id: i64, patch: &EntryPatch) -> Result<(), String> {
        self.store
            .update_entry(id, patch)
            .map_err(|error| error.to_string())?;
        self.reload_pool()
    }

    pub fn delete_entry(&self, id: i64) -> Result<(), String> {
        self.store
            .delete_entry(id)
            .map_err(|error| error.to_string())?;
        self.reload_pool()
    }

    pub fn set_enabled(&self, ids: &[i64], enabled: bool) -> Result<usize, String> {
        let changed = self
            .store
            .set_enabled(ids, enabled)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(changed)
    }

    pub fn reset_score(&self, id: i64) -> Result<(), String> {
        self.store
            .reset_score(id)
            .map_err(|error| error.to_string())?;
        self.reload_pool()
    }

    /// 匯出自訂條目。
    ///
    /// 只含 user 來源：內建目錄對方也有，不必傳；歷史學來的可能夾帶
    /// 工作內容或路徑，不該隨手分享出去。
    pub fn export_entries(&self) -> Result<String, String> {
        let all = self.store.load_all().map_err(|error| error.to_string())?;
        let file = SharedFile {
            version: SHARED_FILE_VERSION,
            entries: all
                .iter()
                .filter(|entry| entry.source == Source::User)
                .map(|entry| SharedEntry {
                    template: entry.template.clone(),
                    description: entry.description.clone(),
                    keywords: entry.keywords.clone(),
                    enabled: entry.enabled,
                    boost: entry.boost,
                })
                .collect(),
        };
        serde_json::to_string_pretty(&file).map_err(|error| error.to_string())
    }

    pub fn import_entries(&self, json: &str) -> Result<usize, String> {
        let file: SharedFile =
            serde_json::from_str(json).map_err(|error| format!("JSON 格式不正確：{error}"))?;
        if file.version > SHARED_FILE_VERSION {
            return Err(format!(
                "這個檔案是較新的格式（version {}），請先更新 QQKey",
                file.version
            ));
        }

        let written = self
            .store
            .upsert_user(&file.entries)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(written)
    }

    // ---------------------------------------------------------- 一般設定

    pub fn settings(&self) -> Settings {
        Settings {
            shortcut: self.shortcut(),
            history_import: self.history_import_enabled(),
            secret_pattern: self.secret_pattern(),
            default_secret_pattern: history::DEFAULT_SECRET_PATTERN.to_string(),
            launcher_opacity: self.launcher_opacity(),
            default_launcher_opacity: DEFAULT_LAUNCHER_OPACITY,
            pool_size: self.pool_size(),
        }
    }

    pub fn shortcut(&self) -> String {
        self.store
            .meta(META_SHORTCUT)
            .ok()
            .flatten()
            .unwrap_or_else(|| crate::hotkey::DEFAULT_SHORTCUT.to_string())
    }

    pub fn set_shortcut(&self, value: &str) -> Result<(), String> {
        self.store
            .set_meta(META_SHORTCUT, value)
            .map_err(|error| error.to_string())
    }

    pub fn secret_pattern(&self) -> String {
        self.store
            .meta(META_SECRET_PATTERN)
            .ok()
            .flatten()
            .unwrap_or_else(|| history::DEFAULT_SECRET_PATTERN.to_string())
    }

    pub fn set_secret_pattern(&self, pattern: &str) -> Result<(), String> {
        SecretFilter::from_pattern(pattern)
            .map_err(|error| format!("不是有效的正規表示式：{error}"))?;
        self.store
            .set_meta(META_SECRET_PATTERN, pattern)
            .map_err(|error| error.to_string())
    }

    /// 候選框背景不透明度（百分比）。
    ///
    /// 解析不出來或超出範圍時靜默退回預設——比照 `secret_filter()` 的策略，
    /// 不讓一個壞掉的設定值把候選框變成看不見的東西。
    pub fn launcher_opacity(&self) -> u8 {
        self.store
            .meta(META_LAUNCHER_OPACITY)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|percent| (MIN_LAUNCHER_OPACITY..=MAX_LAUNCHER_OPACITY).contains(percent))
            .unwrap_or(DEFAULT_LAUNCHER_OPACITY)
    }

    pub fn set_launcher_opacity(&self, percent: u8) -> Result<(), String> {
        if !(MIN_LAUNCHER_OPACITY..=MAX_LAUNCHER_OPACITY).contains(&percent) {
            return Err(format!(
                "不透明度必須介於 {MIN_LAUNCHER_OPACITY}–{MAX_LAUNCHER_OPACITY}%，收到 {percent}%"
            ));
        }
        self.store
            .set_meta(META_LAUNCHER_OPACITY, &percent.to_string())
            .map_err(|error| error.to_string())
    }

    /// 依設定建立過濾器。設定壞掉時退回預設，不要讓匯入整個停擺。
    fn secret_filter(&self) -> SecretFilter {
        SecretFilter::from_pattern(&self.secret_pattern()).unwrap_or_else(|error| {
            crate::trace("歷史", &format!("自訂過濾規則無效，改用預設：{error}"));
            SecretFilter::new()
        })
    }

    fn reload_pool(&self) -> Result<(), String> {
        let pool = self.store.load_enabled().map_err(|error| error.to_string())?;
        *self.pool.write().unwrap() = pool;
        Ok(())
    }
}

/// 從指定位元組讀到檔尾。切到半個 UTF-8 字元時交給 lossy 轉換處理，
/// 那一行本來就會被雜訊或機密過濾擋下。
fn read_from(path: &Path, offset: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, DEFAULT_LAUNCHER_OPACITY, MAX_LAUNCHER_OPACITY, META_LAUNCHER_OPACITY,
        MIN_LAUNCHER_OPACITY,
    };
    use crate::catalog::{EntryPatch, Source};
    use crate::store::Store;

    fn patch(template: &str, description: Option<&str>) -> EntryPatch {
        EntryPatch {
            template: template.to_string(),
            description: description.map(str::to_string),
            keywords: None,
            enabled: None,
            boost: None,
        }
    }

    fn temp_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("建立暫存目錄");
        let store = Store::open(&dir.path().join("test.db")).expect("開啟資料庫");
        (AppState::load(store).expect("載入狀態"), dir)
    }

    #[test]
    fn created_entry_is_searchable_right_away() {
        let (state, _dir) = temp_state();
        state
            .create_entry(&patch("qqkey demo --flag", Some("示範命令")))
            .unwrap();

        let found = state.search("qqkey demo", 9);
        assert_eq!(found.len(), 1, "新增後應該立刻搜得到，不必等重新啟動");
        assert_eq!(found[0].source, Source::User);
    }

    #[test]
    fn disabled_entry_disappears_from_search_but_stays_in_the_list() {
        let (state, _dir) = temp_state();
        let id = state
            .create_entry(&patch("qqkey demo --flag", None))
            .unwrap();

        state.set_enabled(&[id], false).unwrap();
        assert!(state.search("qqkey demo", 9).is_empty(), "停用後不該出現在候選框");

        let page = state.list_entries("qqkey demo", None, 0, 40).unwrap();
        assert_eq!(page.total, 1, "設定畫面仍要看得到停用的條目");
        assert!(!page.entries[0].enabled);
    }

    #[test]
    fn editing_a_builtin_entry_turns_it_into_a_user_entry() {
        let (state, _dir) = temp_state();
        let page = state.list_entries("usbipd list", Some(Source::Builtin), 0, 1).unwrap();
        let target = page.entries[0].id;

        state
            .update_entry(target, &patch("usbipd list", Some("我自己的說明")))
            .unwrap();

        let page = state.list_entries("usbipd list", None, 0, 1).unwrap();
        assert_eq!(
            page.entries[0].source,
            Source::User,
            "改過的條目要轉成 user，之後同步內建目錄才不會蓋回去"
        );
        assert_eq!(page.entries[0].description.as_deref(), Some("我自己的說明"));
    }

    #[test]
    fn export_only_carries_user_entries() {
        let (state, _dir) = temp_state();
        state
            .create_entry(&patch("qqkey demo --flag", Some("示範命令")))
            .unwrap();

        let json = state.export_entries().unwrap();
        assert!(json.contains("qqkey demo --flag"));
        assert!(
            !json.contains("usbipd list"),
            "內建目錄對方也有，不該混進分享檔"
        );
    }

    #[test]
    fn import_round_trips_through_a_fresh_database() {
        let (source, _source_dir) = temp_state();
        source
            .create_entry(&patch("qqkey demo --flag", Some("示範命令")))
            .unwrap();
        let json = source.export_entries().unwrap();

        let (target, _target_dir) = temp_state();
        assert_eq!(target.import_entries(&json).unwrap(), 1);

        let found = target.search("qqkey demo", 9);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].description.as_deref(), Some("示範命令"));
    }

    #[test]
    fn import_rejects_a_newer_format() {
        let (state, _dir) = temp_state();
        let error = state
            .import_entries(r#"{"version": 99, "entries": []}"#)
            .unwrap_err();
        assert!(error.contains("較新的格式"), "實際訊息：{error}");
    }

    #[test]
    fn rejects_a_secret_pattern_that_does_not_compile() {
        let (state, _dir) = temp_state();
        assert!(state.set_secret_pattern(r"(未閉合").is_err());
        assert_eq!(
            state.secret_pattern(),
            crate::catalog::history::DEFAULT_SECRET_PATTERN,
            "設定失敗時不該把壞掉的規則寫進去"
        );
    }

    #[test]
    fn launcher_opacity_falls_back_to_the_default_when_never_set() {
        let (state, _dir) = temp_state();
        assert_eq!(
            state.launcher_opacity(),
            DEFAULT_LAUNCHER_OPACITY,
            "沒設定過時要用預設值，不能是 0——那等於候選框整個不見"
        );
        assert_eq!(
            state.settings().launcher_opacity,
            DEFAULT_LAUNCHER_OPACITY,
            "整包設定要帶同一個值，設定畫面才不會顯示成另一個數字"
        );
    }

    #[test]
    fn launcher_opacity_accepts_both_ends_of_the_allowed_range() {
        let (state, _dir) = temp_state();

        state.set_launcher_opacity(MIN_LAUNCHER_OPACITY).unwrap();
        assert_eq!(
            state.launcher_opacity(),
            MIN_LAUNCHER_OPACITY,
            "下限是合法的選擇，不該被自己的驗證擋掉"
        );

        state.set_launcher_opacity(MAX_LAUNCHER_OPACITY).unwrap();
        assert_eq!(
            state.launcher_opacity(),
            MAX_LAUNCHER_OPACITY,
            "上限即完全不透明，也是合法的選擇"
        );
    }

    #[test]
    fn rejects_a_launcher_opacity_outside_the_allowed_range() {
        let (state, _dir) = temp_state();
        state.set_launcher_opacity(80).unwrap();

        let error = state
            .set_launcher_opacity(MIN_LAUNCHER_OPACITY - 1)
            .unwrap_err();
        assert!(
            error.contains("不透明度"),
            "錯誤訊息要講清楚是哪個設定出問題，實際訊息：{error}"
        );
        assert!(
            state.set_launcher_opacity(MAX_LAUNCHER_OPACITY + 1).is_err(),
            "超過 100% 沒有意義，應該被擋下"
        );
        assert_eq!(
            state.launcher_opacity(),
            80,
            "設定失敗時不該把超出範圍的值寫進去"
        );
    }

    #[test]
    fn a_corrupted_launcher_opacity_falls_back_to_the_default() {
        let (state, _dir) = temp_state();

        state
            .store
            .set_meta(META_LAUNCHER_OPACITY, "半透明")
            .unwrap();
        assert_eq!(
            state.launcher_opacity(),
            DEFAULT_LAUNCHER_OPACITY,
            "手動改壞資料庫時要退回預設，不讓候選框變成看不見"
        );

        state.store.set_meta(META_LAUNCHER_OPACITY, "0").unwrap();
        assert_eq!(
            state.launcher_opacity(),
            DEFAULT_LAUNCHER_OPACITY,
            "超出範圍的舊值同樣要退回預設，而不是照著用"
        );
    }
}
