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
    pub source: Source,
    pub enabled: bool,
    pub score: f64,
    pub last_used: Option<i64>,
    pub boost: f64,
}

impl Entry {
    /// 模糊比對的目標。把中文關鍵字一起併入，
    /// 這樣輸入「掛載」也能找到 `usbipd attach --wsl`。
    pub fn haystack(&self) -> String {
        match (&self.keywords, &self.description) {
            (Some(keywords), Some(description)) => {
                format!("{} {keywords} {description}", self.template)
            }
            (Some(keywords), None) => format!("{} {keywords}", self.template),
            (None, Some(description)) => format!("{} {description}", self.template),
            (None, None) => self.template.clone(),
        }
    }
}

/// 尚未寫進資料庫的條目，來自內建目錄或設定畫面。
#[derive(Debug, Clone, Deserialize)]
pub struct NewEntry {
    pub template: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Option<String>,
}

/// 傳給前端的候選項目。
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: i64,
    pub template: String,
    pub description: Option<String>,
    pub source: Source,
    /// frecency 累計分數，供 UI 顯示常用程度
    pub score: f64,
}

impl From<&Entry> for Candidate {
    fn from(entry: &Entry) -> Self {
        Candidate {
            id: entry.id,
            template: entry.template.clone(),
            description: entry.description.clone(),
            source: entry.source,
            score: entry.score,
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

impl From<&Entry> for EntryView {
    fn from(entry: &Entry) -> Self {
        EntryView {
            id: entry.id,
            template: entry.template.clone(),
            description: entry.description.clone(),
            keywords: entry.keywords.clone(),
            source: entry.source,
            enabled: entry.enabled,
            score: entry.score,
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
