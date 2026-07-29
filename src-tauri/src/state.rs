//! 應用程式共用狀態：資料庫與記憶體中的候選池。

use std::sync::RwLock;

use crate::catalog::{load_builtin, Candidate, Entry};
use crate::ranking;
use crate::store::Store;

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
}
