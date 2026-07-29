//! 候選命令的資料型別與內建目錄。

mod builtin;

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
