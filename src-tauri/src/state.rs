//! 應用程式共用狀態：資料庫與記憶體中的候選池。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::RwLock;

use crate::catalog::history::{self, ImportReport, SecretFilter};
use crate::catalog::{load_builtin, Candidate, Entry};
use crate::ranking;
use crate::store::Store;

/// 上次讀到歷史檔的哪個位元組。下次從這裡接著讀，不必重掃整個檔案。
const META_HISTORY_OFFSET: &str = "history_offset";
/// 是否啟用歷史匯入。
const META_HISTORY_IMPORT: &str = "history_import";

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
        let (entries, report) = history::parse(&text, &SecretFilter::new());

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
