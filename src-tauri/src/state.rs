//! 應用程式共用狀態：資料庫與記憶體中的候選池。

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::RwLock;

use crate::catalog::history::{self, ImportReport, SecretFilter};
use crate::catalog::{
    load_builtin, BackupEntry, BackupFile, Candidate, Entry, EntryPage, EntryPatch, EntryView,
    ImportPreview, SharedEntry, SharedFile, Source, BACKUP_FILE_VERSION, SHARED_FILE_VERSION,
};
use crate::i18n::{self, Lang};
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
/// 介面語言。存 `i18n::AUTO`（跟隨系統顯示語言）或某個 `Lang` 的標籤。
const META_LANGUAGE: &str = "language";

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
    /// 目前真正按得出來的快捷鍵。設定值被別的程式佔用時會退回預設，
    /// 這時它跟 `shortcut` 不一樣，設定畫面得講出這件事。
    /// 空字串代表一個都沒註冊成功。
    pub active_shortcut: String,
    pub history_import: bool,
    pub secret_pattern: String,
    /// 讓設定畫面能提供「還原預設」
    pub default_secret_pattern: String,
    /// 候選框背景不透明度（百分比整數）
    pub launcher_opacity: u8,
    /// 讓設定畫面能提供「還原預設」
    pub default_launcher_opacity: u8,
    /// 語言設定值：`"auto"` 或某個語系標籤。設定畫面的下拉選單認這一個。
    pub language: String,
    /// 實際生效的語系。設定值是 `"auto"` 時它就是 `system_language`。
    pub active_language: Lang,
    /// 系統顯示語言，讓選單能把「跟隨系統」寫成「跟隨系統（日本語）」——
    /// 不然使用者選了 auto 卻不知道會得到什麼。
    pub system_language: Lang,
    pub pool_size: usize,
}

pub struct AppState {
    store: Store,
    /// 啟用中的條目全載入記憶體。候選池頂多幾千筆，
    /// 這樣每敲一個字的搜尋都不必再碰資料庫。
    pool: RwLock<Vec<Entry>>,
    /// 目前真正註冊成功的快捷鍵，空字串代表一個都沒註冊上。
    ///
    /// 跟 meta 裡的設定值分開存。設定的組合被別的程式佔用時會退回預設，
    /// 兩者就此分岔——而解除舊綁定必須認這一個。認設定值的話，解除的會是
    /// 一個從來沒註冊成功的組合，退回註冊的那個就永遠賴在系統裡，
    /// 使用者反而再也設不回預設值。
    active_shortcut: RwLock<String>,
}

impl AppState {
    /// 同步內建目錄後載入候選池。
    pub fn load(store: Store) -> rusqlite::Result<Self> {
        let written = store.sync_builtin(&load_builtin(i18n::current()))?;
        let pool = store.load_enabled()?;
        crate::trace(
            "目錄",
            &format!("內建目錄寫入 {written} 筆，候選池共 {} 筆", pool.len()),
        );
        Ok(Self {
            store,
            pool: RwLock::new(pool),
            // 這時還沒註冊過，等啟動流程試完才知道哪一個真的生效
            active_shortcut: RwLock::new(String::new()),
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Candidate> {
        let pool = self.pool.read().unwrap();
        let now = ranking::now();
        ranking::rank(&pool, query, now, limit)
            .into_iter()
            .map(|entry| Candidate::from_entry(entry, now))
            .collect()
    }

    pub fn template_of(&self, id: i64) -> Result<String, String> {
        self.store
            .find_template(id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| i18n::entry_not_found(i18n::current(), id))
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

        let now = ranking::now();
        Ok(EntryPage {
            total: filtered.len(),
            entries: filtered
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|entry| EntryView::from_entry(entry, now))
                .collect(),
        })
    }

    pub fn create_entry(&self, patch: &EntryPatch) -> Result<i64, String> {
        check_template(&patch.template)?;
        if let Some(boost) = patch.boost {
            check_boost(boost)?;
        }
        let id = self
            .store
            .create_entry(patch)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(id)
    }

    pub fn update_entry(&self, id: i64, patch: &EntryPatch) -> Result<(), String> {
        check_template(&patch.template)?;
        if let Some(boost) = patch.boost {
            check_boost(boost)?;
        }
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

    pub fn delete_entries(&self, ids: &[i64]) -> Result<usize, String> {
        let deleted = self
            .store
            .delete_entries(ids)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(deleted)
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

    /// 解析並驗證分享檔。預覽與實際匯入走同一段，免得兩邊的判準飄開。
    fn parse_shared(&self, json: &str) -> Result<SharedFile, String> {
        let file: SharedFile = serde_json::from_str(json)
            .map_err(|error| i18n::invalid_json(i18n::current(), &error.to_string()))?;
        if file.version > SHARED_FILE_VERSION {
            return Err(i18n::shared_file_newer(i18n::current(), file.version));
        }

        // 整批擋掉而不是跳過有問題的那幾筆：匯入是信任邊界，
        // 沉默地改掉別人給的東西比直接說「這個檔案有問題」更難追。
        for entry in &file.entries {
            check_template(&entry.template)?;
            check_boost(entry.boost)?;
        }
        Ok(file)
    }

    /// 匯入前的試算。
    ///
    /// 從前是直接寫進去、事後才回報筆數，使用者沒有機會知道自己即將覆蓋掉
    /// 多少本機的東西——而覆蓋是沒有 undo 的。
    pub fn preview_import(&self, json: &str) -> Result<ImportPreview, String> {
        let file = self.parse_shared(json)?;
        let existing: HashSet<String> = self
            .store
            .load_all()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| entry.template)
            .collect();

        let overwritten = file
            .entries
            .iter()
            .filter(|entry| existing.contains(&entry.template))
            .count();

        Ok(ImportPreview {
            total: file.entries.len(),
            added: file.entries.len() - overwritten,
            overwritten,
        })
    }

    pub fn import_entries(&self, json: &str) -> Result<usize, String> {
        let file = self.parse_shared(json)?;
        let written = self
            .store
            .upsert_user(&file.entries)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(written)
    }

    /// 完整備份：所有來源的條目、使用統計，以及整張 `meta` 設定。
    ///
    /// 跟 `export_entries()` 是兩件事——那個只匯出 user 來源，是拿來分享的；
    /// 這個是為了換機器時能回到原狀。歷史學來的上千筆與累積的 frecency
    /// 只有這條路帶得走。
    /// 回傳 JSON 與帶走的筆數。
    pub fn backup(&self) -> Result<(String, usize), String> {
        let all = self.store.load_all().map_err(|error| error.to_string())?;
        let settings = self.store.all_meta().map_err(|error| error.to_string())?;
        let count = all.len();

        let file = BackupFile {
            version: BACKUP_FILE_VERSION,
            entries: all
                .iter()
                .map(|entry| BackupEntry {
                    template: entry.template.clone(),
                    description: entry.description.clone(),
                    keywords: entry.keywords.clone(),
                    source: entry.source,
                    enabled: entry.enabled,
                    // 存原始分數而不是衰減後的：衰減是相對於「現在」算的，
                    // 存快照等於把時間凍結，還原之後排序就不對了。
                    score: entry.score,
                    last_used: entry.last_used,
                    boost: entry.boost,
                })
                .collect(),
            settings,
        };
        let json = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
        Ok((json, count))
    }

    /// 從備份還原。會**取代**目前的全部資料。
    pub fn restore(&self, json: &str) -> Result<usize, String> {
        let file: BackupFile = serde_json::from_str(json)
            .map_err(|error| i18n::invalid_backup(i18n::current(), &error.to_string()))?;
        if file.version > BACKUP_FILE_VERSION {
            return Err(i18n::backup_newer(i18n::current(), file.version));
        }
        for entry in &file.entries {
            check_template(&entry.template)?;
            check_boost(entry.boost)?;
        }

        let written = self
            .store
            .restore(&file.entries, &file.settings)
            .map_err(|error| error.to_string())?;
        self.reload_pool()?;
        Ok(written)
    }

    // ---------------------------------------------------------- 一般設定

    pub fn settings(&self) -> Settings {
        Settings {
            shortcut: self.shortcut(),
            active_shortcut: self.active_shortcut(),
            history_import: self.history_import_enabled(),
            secret_pattern: self.secret_pattern(),
            default_secret_pattern: history::DEFAULT_SECRET_PATTERN.to_string(),
            launcher_opacity: self.launcher_opacity(),
            default_launcher_opacity: DEFAULT_LAUNCHER_OPACITY,
            language: self.language(),
            active_language: self.active_language(),
            system_language: i18n::system_language(),
            pool_size: self.pool_size(),
        }
    }

    /// 語言設定值：`"auto"` 或某個語系標籤。
    pub fn language(&self) -> String {
        self.store
            .meta(META_LANGUAGE)
            .ok()
            .flatten()
            .unwrap_or_else(|| i18n::AUTO.to_string())
    }

    /// 實際生效的語系。
    ///
    /// 刻意不像 `active_shortcut` 那樣另外存一份：那個會跟設定值分岔是因為
    /// 作業系統拒絕註冊，是算不回來的外部事實；語系只是「設定值 + 系統語系」
    /// 的純函數，多存一份就多一個要同步的地方。
    pub fn active_language(&self) -> Lang {
        i18n::resolve(self.store.meta(META_LANGUAGE).ok().flatten().as_deref())
    }

    /// `meta` 與 `i18n` 全域快取的唯一寫入口。
    ///
    /// 分開寫的話，某條路徑只更新一邊，症狀會是「重開才生效」或
    /// 「重開又變回去」，而兩者都很難歸因。
    pub fn set_language(&self, value: &str) -> Result<(), String> {
        // 正規化之後才存，免得資料庫裡同時出現 zh-hant 與 zh-Hant
        let stored = if value == i18n::AUTO {
            i18n::AUTO
        } else {
            Lang::parse(value)
                .ok_or_else(|| i18n::unsupported_language(i18n::current(), value))?
                .as_tag()
        };
        self.store
            .set_meta(META_LANGUAGE, stored)
            .map_err(|error| error.to_string())?;
        i18n::set_current(self.active_language());
        Ok(())
    }

    /// 換語言之後重新同步內建目錄的說明文字。
    ///
    /// 只有 `source = 'builtin'` 的條目會被換掉（`sync_builtin()` 的 `WHERE`
    /// 保護）。使用者編輯過而轉成 user 的那些會**停在他當初寫的內容**，不跟著
    /// 換語言——那些字是他自己選的，而覆寫等於切一次語言就靜默毀掉他寫的說明，
    /// 比留下一句舊語言的說明糟得多。想拿回內建版本的話刪掉那一筆，
    /// 下一次同步會把它重建回來。
    pub fn resync_builtin(&self) -> Result<(), String> {
        let written = self
            .store
            .sync_builtin(&load_builtin(i18n::current()))
            .map_err(|error| error.to_string())?;
        crate::trace("目錄", &format!("換語言後重新同步 {written} 筆"));
        // 不重載候選池的話，候選框裡的說明文字要等到下次啟動才換得掉
        self.reload_pool()
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

    /// 目前真正註冊成功的快捷鍵，空字串代表一個都沒有。
    pub fn active_shortcut(&self) -> String {
        self.active_shortcut.read().unwrap().clone()
    }

    /// 由啟動流程與換綁流程在確定註冊結果之後呼叫。
    pub fn set_active_shortcut(&self, value: &str) {
        *self.active_shortcut.write().unwrap() = value.to_string();
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
            .map_err(|error| i18n::invalid_regex(i18n::current(), &error.to_string()))?;
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
            return Err(i18n::opacity_out_of_range(
                i18n::current(),
                MIN_LAUNCHER_OPACITY,
                MAX_LAUNCHER_OPACITY,
                percent,
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

/// 樣板寫進資料庫之前先擋掉控制字元。
///
/// 注入端還有一道 `template::sanitize()` 兜底，但那一道是無聲的——
/// 使用者會納悶存進去的命令為什麼跟送出來的不一樣。問題在入口就講清楚，
/// 比事後默默改掉他的東西好。
/// 手動加權只收非負的有限數。
///
/// 負的 boost 會讓排序權重的 `ln()` 得出 NaN，而 NaN 在比較時被
/// `partial_cmp` 判為 `None` 吞掉——那筆命令會卡在原始順序上，
/// 完全不受查詢相關度影響。症狀隱晦到使用者幾乎不可能歸因到
/// 自己在某個欄位填了 -5。想讓命令往後排該用「停用」。
fn check_boost(boost: f64) -> Result<(), String> {
    if !boost.is_finite() {
        return Err(i18n::boost_not_finite(i18n::current()));
    }
    if boost < 0.0 {
        return Err(i18n::boost_negative(i18n::current(), boost));
    }
    Ok(())
}

fn check_template(template: &str) -> Result<(), String> {
    if crate::template::has_control_chars(template) {
        return Err(i18n::template_has_control_chars(
            i18n::current(),
            &template.escape_debug().to_string(),
        ));
    }
    Ok(())
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
        // 錯誤訊息的斷言認的是繁中字眼，不釘的話會跟著開發機的顯示語言跑
        crate::i18n::pin_for_tests();
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
    fn create_rejects_a_template_carrying_a_newline() {
        let (state, _dir) = temp_state();

        let error = state
            .create_entry(&patch("git push --force\n", None))
            .expect_err("含換行的樣板不該進得了資料庫");

        assert!(
            error.contains("控制字元"),
            "訊息要說得出問題在哪，實際拿到：{error}"
        );
    }

    #[test]
    fn import_rejects_a_shared_file_carrying_a_newline() {
        let (state, _dir) = temp_state();
        // 設定畫面的單行輸入框擋得住手打，但剪貼簿匯入完全繞過它
        let json = r#"{"version":1,"entries":[{"template":"git push --force\n"}]}"#;
        let before = state.pool_size();

        let error = state
            .import_entries(json)
            .expect_err("挾帶換行的分享檔整份都不該收下");

        assert!(
            error.contains("控制字元"),
            "訊息要說得出問題在哪，實際拿到：{error}"
        );
        assert_eq!(state.pool_size(), before, "整批拒絕，不該有半筆漏進去");
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
    fn rejects_a_negative_boost_that_would_nan_the_ranking() {
        let (state, _dir) = temp_state();
        let mut bad = patch("qqkey demo --flag", None);
        bad.boost = Some(-5.0);

        let error = state
            .create_entry(&bad)
            .expect_err("負的加權會讓排序權重變成 NaN，該在入口擋下");

        assert!(
            error.contains("負數"),
            "訊息要說得出問題在哪，實際拿到：{error}"
        );
    }

    #[test]
    fn ranking_survives_a_negative_boost_already_in_the_database() {
        // 入口擋得住新資料，但舊資料庫裡可能已經有負值
        let entry = crate::catalog::Entry {
            id: 1,
            template: "qqkey demo".into(),
            description: None,
            keywords: None,
            keywords_all: None,
            source: Source::User,
            enabled: true,
            score: 0.0,
            last_used: None,
            boost: -5.0,
        };

        let weight = crate::ranking::frecency_weight(&entry, 0);

        assert!(
            weight.is_finite(),
            "排序權重不能是 NaN，否則那筆會卡在原位且不受查詢影響"
        );
    }

    #[test]
    fn import_does_not_reopen_an_entry_you_disabled() {
        let (state, _dir) = temp_state();
        let id = state.create_entry(&patch("qqkey demo --flag", None)).unwrap();
        state.set_enabled(&[id], false).unwrap();

        // 對方那份檔案裡這筆是啟用的
        let json = r#"{"version":1,"entries":[{"template":"qqkey demo --flag","description":"對方的說明"}]}"#;
        state.import_entries(json).unwrap();

        let page = state.list_entries("qqkey demo", None, 0, 1).unwrap();
        assert!(
            !page.entries[0].enabled,
            "你刻意關掉的條目不該因為匯入別人的檔案就自己打開"
        );
        assert_eq!(
            page.entries[0].description.as_deref(),
            Some("對方的說明"),
            "內容本身還是要更新，不然匯入等於沒做"
        );
    }

    #[test]
    fn toggling_enabled_leaves_a_builtin_entry_builtin() {
        let (state, _dir) = temp_state();
        let page = state
            .list_entries("usbipd list", Some(Source::Builtin), 0, 1)
            .unwrap();
        let target = page.entries[0].id;

        state.set_enabled(&[target], false).unwrap();

        let page = state.list_entries("usbipd list", None, 0, 1).unwrap();
        assert_eq!(
            page.entries[0].source,
            Source::Builtin,
            "單純開關啟用不該改變來源——只有真的編輯內容才轉成 user"
        );
        assert!(!page.entries[0].enabled, "停用本身還是要生效");
    }

    #[test]
    fn export_skips_a_disabled_builtin_entry() {
        let (state, _dir) = temp_state();
        let page = state
            .list_entries("usbipd list", Some(Source::Builtin), 0, 1)
            .unwrap();
        state.set_enabled(&[page.entries[0].id], false).unwrap();

        let json = state.export_entries().unwrap();

        assert!(
            !json.contains("usbipd list"),
            "停用內建條目不該讓它混進分享檔——那是承諾只含自己新增或編輯過的"
        );
    }

    #[test]
    fn backup_carries_what_export_deliberately_leaves_behind() {
        let (state, _dir) = temp_state();
        state
            .create_entry(&patch("qqkey demo --flag", Some("示範命令")))
            .unwrap();
        state.set_launcher_opacity(55).unwrap();

        let (json, count) = state.backup().unwrap();

        assert!(count > 1, "備份要含所有來源，不只自訂的那一筆");
        assert!(
            json.contains("usbipd list"),
            "內建條目也要帶走——這是備份不是分享"
        );
        assert!(
            json.contains("launcher_opacity"),
            "meta 設定要一起備份，換機器才回得到原狀"
        );
    }

    #[test]
    fn restore_replaces_everything_and_brings_settings_back() {
        let (state, _dir) = temp_state();
        state
            .create_entry(&patch("qqkey demo --flag", Some("備份前")))
            .unwrap();
        state.set_launcher_opacity(55).unwrap();
        let (json, _) = state.backup().unwrap();

        // 備份之後又動了一輪
        state
            .create_entry(&patch("qqkey after-backup", None))
            .unwrap();
        state.set_launcher_opacity(88).unwrap();

        state.restore(&json).unwrap();

        let page = state
            .list_entries("qqkey after-backup", None, 0, 10)
            .unwrap();
        assert_eq!(page.total, 0, "還原是取代——備份之後新增的東西不該留下");
        assert_eq!(
            state.launcher_opacity(),
            55,
            "設定也要回到備份當時的樣子"
        );
        let page = state.list_entries("qqkey demo", None, 0, 10).unwrap();
        assert_eq!(page.entries[0].description.as_deref(), Some("備份前"));
    }

    #[test]
    fn restore_refuses_a_backup_from_a_newer_version() {
        let (state, _dir) = temp_state();
        let json = r#"{"version":99,"entries":[],"settings":[]}"#;

        let error = state
            .restore(json)
            .expect_err("格式比程式新就不該硬還原下去");

        assert!(error.contains("較新"), "訊息要說得出原因，實際拿到：{error}");
    }

    #[test]
    fn preview_tells_you_how_much_will_be_overwritten() {
        let (state, _dir) = temp_state();
        state
            .create_entry(&patch("qqkey demo --flag", None))
            .unwrap();
        let json = r#"{"version":1,"entries":[
            {"template":"qqkey demo --flag","description":"對方的說明"},
            {"template":"qqkey brand-new"}
        ]}"#;

        let preview = state.preview_import(json).unwrap();

        assert_eq!(preview.total, 2);
        assert_eq!(preview.added, 1, "本機沒有的算新增");
        assert_eq!(preview.overwritten, 1, "撞名的要先講，覆寫是沒有 undo 的");
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
