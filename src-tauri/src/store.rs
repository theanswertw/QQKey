//! SQLite 儲存。資料庫放在 `%APPDATA%\com.example.qqkey\qqkey.db`，不對外傳送。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::catalog::{Entry, EntryPatch, NewEntry, SharedEntry, Source};

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// 錯誤是字串而不是 `rusqlite::Error`：schema 版本比程式新並不是資料庫
    /// 出錯，而是要講給使用者聽的話，而這句話會直接進啟動失敗的對話框。
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            // 這裡失敗的話下面 Connection::open 也會失敗，錯誤由它去報；
            // 但「為什麼」建不出來（權限、磁碟滿）只有這一行看得到。
            if let Err(error) = std::fs::create_dir_all(parent) {
                crate::trace("儲存", &format!("建立資料夾失敗：{error}"));
            }
        }
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// 把內建目錄寫進資料庫。已存在的條目只更新說明與關鍵字，
    /// 保留使用統計；使用者在設定畫面改過的條目（source = user）不動。
    pub fn sync_builtin(&self, entries: &[NewEntry]) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut written = 0;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO entry (template, description, keywords, source)
                 VALUES (?1, ?2, ?3, 'builtin')
                 ON CONFLICT(template) DO UPDATE SET
                   description = excluded.description,
                   keywords = excluded.keywords
                 WHERE entry.source = 'builtin'",
            )?;
            for entry in entries {
                written += statement.execute(params![
                    entry.template,
                    entry.description,
                    entry.keywords
                ])?;
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    /// 載入所有啟用中的條目。候選池不大，一次全讀進記憶體，
    /// 之後每次敲鍵的搜尋就不必再碰資料庫。
    pub fn load_enabled(&self) -> rusqlite::Result<Vec<Entry>> {
        self.load_where("WHERE enabled = 1")
    }

    /// 載入所有條目，含停用中的。設定畫面才需要。
    pub fn load_all(&self) -> rusqlite::Result<Vec<Entry>> {
        self.load_where("")
    }

    fn load_where(&self, filter: &str) -> rusqlite::Result<Vec<Entry>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(&format!(
            "SELECT id, template, description, keywords, source, enabled, score, last_used, boost
             FROM entry {filter}"
        ))?;
        let rows = statement.query_map([], |row| {
            let source: String = row.get(4)?;
            Ok(Entry {
                id: row.get(0)?,
                template: row.get(1)?,
                description: row.get(2)?,
                keywords: row.get(3)?,
                source: Source::parse(&source),
                enabled: row.get::<_, i64>(5)? != 0,
                score: row.get(6)?,
                last_used: row.get(7)?,
                boost: row.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// 新增使用者自訂條目。回傳新條目的 id。
    pub fn create_entry(&self, patch: &EntryPatch) -> rusqlite::Result<i64> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "INSERT INTO entry (template, description, keywords, source, enabled, boost)
             VALUES (?1, ?2, ?3, 'user', 1, ?4)",
            params![
                patch.template,
                patch.description,
                patch.keywords,
                patch.boost.unwrap_or(0.0)
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// 更新條目。使用者改過的內建條目會轉為 user 來源，
    /// 之後同步內建目錄時才不會把他的修改蓋回去。
    pub fn update_entry(&self, id: i64, patch: &EntryPatch) -> rusqlite::Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "UPDATE entry SET
               template    = ?1,
               description = ?2,
               keywords    = ?3,
               enabled     = COALESCE(?4, enabled),
               boost       = COALESCE(?5, boost),
               source      = CASE source WHEN 'builtin' THEN 'user' ELSE source END
             WHERE id = ?6",
            params![
                patch.template,
                patch.description,
                patch.keywords,
                patch.enabled,
                patch.boost,
                id
            ],
        )?;
        Ok(())
    }

    pub fn delete_entry(&self, id: i64) -> rusqlite::Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute("DELETE FROM entry WHERE id = ?1", [id])?;
        Ok(())
    }

    /// 批次啟用／停用。用來一次關掉整批歷史雜訊。
    pub fn set_enabled(&self, ids: &[i64], enabled: bool) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut changed = 0;
        {
            let mut statement =
                transaction.prepare("UPDATE entry SET enabled = ?1 WHERE id = ?2")?;
            for id in ids {
                changed += statement.execute(params![enabled as i64, id])?;
            }
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// 清掉單筆的使用統計，讓它回到從未用過的狀態。
    pub fn reset_score(&self, id: i64) -> rusqlite::Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "UPDATE entry SET score = 0, last_used = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn find_template(&self, id: i64) -> rusqlite::Result<Option<String>> {
        let connection = self.connection.lock().unwrap();
        connection
            .query_row("SELECT template FROM entry WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
    }

    /// 寫入從歷史紀錄學到的命令。
    ///
    /// 已存在的條目只在來源同為 history 時更新分數，且取較大值——
    /// 內建目錄整理過的條目不該被歷史裡的同名命令蓋掉，
    /// 使用者實際累積的使用分數也不該被匯入回沖。
    pub fn import_history(&self, entries: &[(String, usize)]) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut written = 0;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO entry (template, source, score)
                 VALUES (?1, 'history', ?2)
                 ON CONFLICT(template) DO UPDATE SET
                   score = MAX(entry.score, excluded.score)
                 WHERE entry.source = 'history'",
            )?;
            for (template, count) in entries {
                written += statement.execute(params![
                    template,
                    crate::catalog::history::initial_score(*count)
                ])?;
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    /// 寫入從 JSON 匯入的使用者條目。同 template 者覆寫，並一律標記為 user 來源。
    ///
    /// 轉成 user 是刻意的：使用者選了對方那一版，效果等同他自己編輯過，
    /// 之後就不該再被內建目錄蓋回去。這跟 `sync_builtin`／`import_history`
    /// 的 `WHERE` 保護不衝突——那兩支是自動跑的，不能碰使用者整理過的東西；
    /// 這一支是使用者按下按鈕才會發生的。
    ///
    /// 但 `enabled` 不跟著覆寫：你刻意關掉的條目，不該因為匯入一份別人的
    /// 檔案就自己打開。新條目仍照檔案裡的值建立。
    pub fn upsert_user(&self, entries: &[SharedEntry]) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut written = 0;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO entry (template, description, keywords, source, enabled, boost)
                 VALUES (?1, ?2, ?3, 'user', ?4, ?5)
                 ON CONFLICT(template) DO UPDATE SET
                   description = excluded.description,
                   keywords    = excluded.keywords,
                   boost       = excluded.boost,
                   source      = 'user'",
            )?;
            for entry in entries {
                written += statement.execute(params![
                    entry.template,
                    entry.description,
                    entry.keywords,
                    entry.enabled as i64,
                    entry.boost
                ])?;
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    pub fn meta(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let connection = self.connection.lock().unwrap();
        connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 記錄一次使用並更新 frecency，回傳新的分數。
    ///
    /// 衰減在 Rust 端算——SQLite 的數學函式要編譯時另外開啟，不能假設有。
    pub fn record_use(&self, id: i64, now: i64) -> rusqlite::Result<f64> {
        let connection = self.connection.lock().unwrap();
        let (score, last_used): (f64, Option<i64>) = connection.query_row(
            "SELECT score, last_used FROM entry WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let updated = crate::ranking::bump(score, last_used, now);
        connection.execute(
            "UPDATE entry SET score = ?1, last_used = ?2 WHERE id = ?3",
            params![updated, now, id],
        )?;
        Ok(updated)
    }
}

/// 目前的 schema 版本。動到 `entry` 或 `meta` 的結構時 +1，
/// 並在 `migrate()` 的階梯尾端補上對應的一段。
const SCHEMA_VERSION: i64 = 1;

/// 建立或升級 schema。版本記在 SQLite 內建的 `user_version`，不另外開表。
///
/// 從前這裡只有一組 `CREATE TABLE IF NOT EXISTS`。那對既有資料庫等於什麼
/// 都不做——只要哪天加一個欄位，舊資料庫的 SELECT 就會撞上 `no such column`，
/// 而且是在啟動時撞，使用者只會看到工具再也打不開。匯出檔案早就有版本號了，
/// 資料庫沒理由沒有。
fn migrate(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA journal_mode = WAL;")
        .map_err(|error| format!("設定 journal 模式失敗：{error}"))?;

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("讀取 schema 版本失敗：{error}"))?;

    if version > SCHEMA_VERSION {
        // 不試著降級。新版寫進去的欄位砍掉就是弄丟資料，寧可不開。
        return Err(format!(
            "這個資料庫是較新版本的 QQKey 建立的（schema v{version}，\
             本程式只認得 v{SCHEMA_VERSION}）。請改用新版，\
             或把資料庫移到別處讓 QQKey 重建一個。"
        ));
    }

    // 既有資料庫的 user_version 是 0 但表已經在了，靠 IF NOT EXISTS 讓這一段
    // 對它是空操作，跑完一樣蓋上版本章。
    if version < 1 {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS entry (
                   id          INTEGER PRIMARY KEY,
                   template    TEXT NOT NULL UNIQUE,
                   description TEXT,
                   keywords    TEXT,
                   source      TEXT NOT NULL,
                   enabled     INTEGER NOT NULL DEFAULT 1,
                   score       REAL NOT NULL DEFAULT 0,
                   last_used   INTEGER,
                   boost       REAL NOT NULL DEFAULT 0
                 );

                 CREATE INDEX IF NOT EXISTS idx_entry_enabled ON entry(enabled);

                 CREATE TABLE IF NOT EXISTS meta (
                   key   TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );",
            )
            .map_err(|error| format!("建立資料表失敗：{error}"))?;
    }

    // 下一次改 schema 就接在這裡：if version < 2 { ...ALTER TABLE... }

    if version != SCHEMA_VERSION {
        // user_version 不吃參數綁定，只能組字串；SCHEMA_VERSION 是常數。
        connection
            .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(|error| format!("寫入 schema 版本失敗：{error}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::catalog::{NewEntry, Source};

    fn new_entry(template: &str, description: &str) -> NewEntry {
        NewEntry {
            template: template.to_string(),
            description: Some(description.to_string()),
            keywords: None,
        }
    }

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("建立暫存目錄");
        let store = Store::open(&dir.path().join("test.db")).expect("開啟資料庫");
        (store, dir)
    }

    #[test]
    fn migration_stamps_the_schema_version() {
        let (store, _dir) = temp_store();

        let version: i64 = store
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(
            version,
            super::SCHEMA_VERSION,
            "開完資料庫就該蓋上版本章，否則下次升級認不出這是哪一版"
        );
    }

    #[test]
    fn migration_leaves_an_existing_database_intact() {
        let dir = tempfile::tempdir().expect("建立暫存目錄");
        let path = dir.path().join("test.db");
        {
            let store = Store::open(&path).expect("第一次開啟");
            store
                .sync_builtin(&[new_entry("usbipd list", "列出裝置")])
                .unwrap();
        }

        let store = Store::open(&path).expect("同一個資料庫再開一次不該失敗");

        assert_eq!(
            store.load_enabled().unwrap().len(),
            1,
            "重跑 migration 不該動到既有資料"
        );
    }

    #[test]
    fn refuses_a_database_written_by_a_newer_version() {
        let dir = tempfile::tempdir().expect("建立暫存目錄");
        let path = dir.path().join("test.db");
        Store::open(&path).expect("先建立一個正常的資料庫");
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute_batch(&format!(
                    "PRAGMA user_version = {};",
                    super::SCHEMA_VERSION + 1
                ))
                .unwrap();
        }

        let error = match Store::open(&path) {
            Ok(_) => panic!("版本比程式新就不該硬開下去"),
            Err(error) => error,
        };

        assert!(
            error.contains("較新版本"),
            "訊息要讓使用者看得懂發生什麼事，實際拿到：{error}"
        );
    }

    #[test]
    fn sync_builtin_is_idempotent() {
        let (store, _dir) = temp_store();
        let entries = vec![new_entry("usbipd list", "列出裝置")];

        store.sync_builtin(&entries).unwrap();
        store.sync_builtin(&entries).unwrap();

        let loaded = store.load_enabled().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].source, Source::Builtin);
    }

    #[test]
    fn sync_builtin_refreshes_the_description() {
        let (store, _dir) = temp_store();
        store
            .sync_builtin(&[new_entry("usbipd list", "舊說明")])
            .unwrap();
        store
            .sync_builtin(&[new_entry("usbipd list", "新說明")])
            .unwrap();

        let loaded = store.load_enabled().unwrap();
        assert_eq!(loaded[0].description.as_deref(), Some("新說明"));
    }

    #[test]
    fn record_use_accumulates_and_persists() {
        let (store, _dir) = temp_store();
        store
            .sync_builtin(&[new_entry("usbipd list", "列出裝置")])
            .unwrap();
        let id = store.load_enabled().unwrap()[0].id;

        let now = 1_800_000_000;
        assert!((store.record_use(id, now).unwrap() - 1.0).abs() < 1e-9);
        assert!((store.record_use(id, now).unwrap() - 2.0).abs() < 1e-9);

        let loaded = store.load_enabled().unwrap();
        assert!((loaded[0].score - 2.0).abs() < 1e-9);
        assert_eq!(loaded[0].last_used, Some(now));
    }

    #[test]
    fn find_template_returns_none_for_unknown_id() {
        let (store, _dir) = temp_store();
        assert_eq!(store.find_template(999).unwrap(), None);
    }

    #[test]
    fn history_import_does_not_overwrite_builtin_entries() {
        let (store, _dir) = temp_store();
        store
            .sync_builtin(&[new_entry("usbipd list", "列出裝置")])
            .unwrap();
        store
            .import_history(&[("usbipd list".to_string(), 50)])
            .unwrap();

        let loaded = store.load_enabled().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].source, Source::Builtin, "來源不該被歷史蓋掉");
        assert_eq!(loaded[0].score, 0.0, "內建條目的分數不該被匯入回沖");
        assert_eq!(loaded[0].description.as_deref(), Some("列出裝置"));
    }

    #[test]
    fn history_import_does_not_reset_accumulated_usage() {
        let (store, _dir) = temp_store();
        store
            .import_history(&[("usbipd attach --wsl --busid 2-3".to_string(), 2)])
            .unwrap();
        let id = store.load_enabled().unwrap()[0].id;

        let now = 1_800_000_000;
        for _ in 0..10 {
            store.record_use(id, now).unwrap();
        }
        let accumulated = store.load_enabled().unwrap()[0].score;

        // 再匯入一次，次數不變
        store
            .import_history(&[("usbipd attach --wsl --busid 2-3".to_string(), 2)])
            .unwrap();

        assert_eq!(
            store.load_enabled().unwrap()[0].score,
            accumulated,
            "重複匯入不該把實際使用累積的分數壓回去"
        );
    }

    #[test]
    fn meta_round_trips() {
        let (store, _dir) = temp_store();
        assert_eq!(store.meta("history_offset").unwrap(), None);

        store.set_meta("history_offset", "1024").unwrap();
        assert_eq!(
            store.meta("history_offset").unwrap().as_deref(),
            Some("1024")
        );

        store.set_meta("history_offset", "2048").unwrap();
        assert_eq!(
            store.meta("history_offset").unwrap().as_deref(),
            Some("2048")
        );
    }
}
