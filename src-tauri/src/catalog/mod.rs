//! 候選命令的資料型別與內建目錄。

mod builtin;
pub mod history;

pub use builtin::load_builtin;

use serde::{Deserialize, Serialize};

/// 條目來源。分數相同時的優先序為 使用者自訂 > 內建目錄 > 歷史紀錄，
/// 免得歷史紀錄的雜訊蓋過整理過的目錄項目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    User,
    Builtin,
    History,
}

impl Source {
    pub fn parse(value: &str) -> Self {
        match value {
            "user" => Source::User,
            "history" => Source::History,
            _ => Source::Builtin,
        }
    }

    /// 寫回資料庫用的字串，與 `parse()` 互為反向。
    pub fn as_str(self) -> &'static str {
        match self {
            Source::User => "user",
            Source::Builtin => "builtin",
            Source::History => "history",
        }
    }

    pub fn priority(self) -> u8 {
        match self {
            Source::User => 2,
            Source::Builtin => 1,
            Source::History => 0,
        }
    }
}

/// 資料庫中的一筆命令。
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub template: String,
    pub description: Option<String>,
    pub keywords: Option<String>,
    /// 內建目錄六個語言的 keywords 聯集，只給 [`Entry::haystack`] 用。
    ///
    /// `keywords` 是給人看的（設定畫面的「搜尋關鍵字」欄位讀它），這一欄是給
    /// 模糊比對用的。使用者自訂、歷史學來的、以及被編輯過的條目都是 `None`。
    pub keywords_all: Option<String>,
    pub source: Source,
    pub enabled: bool,
    pub score: f64,
    pub last_used: Option<i64>,
    pub boost: f64,
}

impl Entry {
    /// 模糊比對的目標。把關鍵字一起併入，
    /// 這樣輸入「掛載」也能找到 `usbipd attach --wsl`。
    ///
    /// 內建條目用六語言聯集的 `keywords_all`，所以介面切成英文之後，用中文
    /// 關鍵字一樣找得到——反之亦然。使用者編輯過的條目沒有聯集，就用他自己
    /// 填的 `keywords`。
    ///
    /// 順序是刻意的：template → 目前語言的 keywords → description。
    /// nucleo 對靠前與連續的命中給較高分，把「使用者這個語言真的會打的字」
    /// 排在前面，其他語言的關鍵字（在 `keywords_all` 的後段）就落在後面。
    pub fn haystack(&self) -> String {
        let keywords = self.keywords_all.as_ref().or(self.keywords.as_ref());
        match (keywords, &self.description) {
            (Some(keywords), Some(description)) => {
                format!("{} {keywords} {description}", self.template)
            }
            (Some(keywords), None) => format!("{} {keywords}", self.template),
            (None, Some(description)) => format!("{} {description}", self.template),
            (None, None) => self.template.clone(),
        }
    }
}

/// 尚未寫進資料庫的條目。
///
/// 由 `builtin::load_builtin()` 依當前語系從磁碟上的多語目錄組出來，
/// 不再直接對應 JSON 的形狀——那是 `builtin::CatalogEntry` 的事。
#[derive(Debug, Clone)]
pub struct NewEntry {
    pub template: String,
    pub description: Option<String>,
    /// 當前語系的關鍵字，會顯示在設定畫面的「搜尋關鍵字」欄位。
    pub keywords: Option<String>,
    /// 六語言聯集，只給 [`Entry::haystack`] 用。
    pub keywords_all: Option<String>,
}

/// 傳給前端的候選項目。
///
/// 欄位目前都是單字，`camelCase` 轉換後長得一樣——但標上去是為了日後：
/// 加一個 `last_used` 而忘了這件事的話，Rust 送 `last_used`、TS 寫 `lastUsed`，
/// `cargo build` 與 `tsc` 都不會有意見，執行期才變成 undefined。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: i64,
    pub template: String,
    pub description: Option<String>,
    pub source: Source,
    /// 衰減到當下的 frecency 分數，供 UI 顯示常用程度
    pub score: f64,
}

impl Candidate {
    /// 分數要衰減到「現在」才有意義。顯示未衰減的原始值，會讓一個三個月
    /// 沒碰過的命令標著 ★10 卻排在 ★3 的後面——使用者看到的數字解釋不了
    /// 他看到的順序。
    pub fn from_entry(entry: &Entry, now: i64) -> Self {
        Candidate {
            id: entry.id,
            template: entry.template.clone(),
            description: entry.description.clone(),
            source: entry.source,
            score: crate::ranking::decay(entry.score, entry.last_used, now),
        }
    }
}

/// 傳給設定畫面的完整條目。比候選框用的 `Candidate` 多了啟用狀態與加權，
/// 也包含停用中的條目。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub id: i64,
    pub template: String,
    pub description: Option<String>,
    pub keywords: Option<String>,
    pub source: Source,
    pub enabled: bool,
    pub score: f64,
    pub boost: f64,
    pub last_used: Option<i64>,
}

impl EntryView {
    /// 同 `Candidate::from_entry`：`score` 是衰減到當下的值，
    /// 設定畫面顯示的數字才解釋得了候選框裡的排序。
    pub fn from_entry(entry: &Entry, now: i64) -> Self {
        EntryView {
            id: entry.id,
            template: entry.template.clone(),
            description: entry.description.clone(),
            keywords: entry.keywords.clone(),
            source: entry.source,
            enabled: entry.enabled,
            score: crate::ranking::decay(entry.score, entry.last_used, now),
            boost: entry.boost,
            last_used: entry.last_used,
        }
    }
}

/// 設定畫面的條目列表結果。
#[derive(Debug, Clone, Serialize)]
pub struct EntryPage {
    pub total: usize,
    pub entries: Vec<EntryView>,
}

/// 條目的可編輯欄位。
#[derive(Debug, Clone, Deserialize)]
pub struct EntryPatch {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub boost: Option<f64>,
}

/// 匯出／匯入用的條目。
///
/// 刻意不含使用統計——分享的是整理好的命令，不是自己用了幾次。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedEntry {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub boost: f64,
}

fn default_true() -> bool {
    true
}

/// 匯出檔的格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFile {
    pub version: u32,
    pub entries: Vec<SharedEntry>,
}

/// 目前的匯出格式版本。
pub const SHARED_FILE_VERSION: u32 = 1;

/// 匯入前的試算，讓使用者知道按下去會發生什麼事。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub total: usize,
    /// 本機還沒有的
    pub added: usize,
    /// 會覆寫既有內容的
    pub overwritten: usize,
}

/// 備份檔裡的一筆命令。
///
/// 跟 `SharedEntry` 分開：分享給同事的是整理好的命令，備份要的卻是
/// 「換一台機器能回到原狀」，所以來源與使用統計一個都不能少。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Option<String>,
    pub source: Source,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub last_used: Option<i64>,
    #[serde(default)]
    pub boost: f64,
}

/// 完整備份檔。
///
/// 存的是**未衰減**的原始分數與最後使用時間，不是畫面上顯示的那個值——
/// 衰減是相對於「現在」算出來的，存快照進去等於把時間也一起凍結，
/// 還原之後排序就不對了。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub version: u32,
    pub entries: Vec<BackupEntry>,
    /// `meta` 表原樣帶走：快捷鍵、歷史匯入位移、機密過濾樣式、不透明度。
    pub settings: Vec<(String, String)>,
}

/// 目前的備份格式版本。
pub const BACKUP_FILE_VERSION: u32 = 1;
