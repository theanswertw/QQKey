//! SQLite 儲存。資料庫放在 `%APPDATA%\QQKey\qqkey.db`，不對外傳送。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::catalog::{Entry, NewEntry, Source};

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let connection = Connection::open(path)?;
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
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT id, template, description, keywords, source, score, last_used, boost
             FROM entry WHERE enabled = 1",
        )?;
        let rows = statement.query_map([], |row| {
            let source: String = row.get(4)?;
            Ok(Entry {
                id: row.get(0)?,
                template: row.get(1)?,
                description: row.get(2)?,
                keywords: row.get(3)?,
                source: Source::parse(&source),
                score: row.get(5)?,
                last_used: row.get(6)?,
                boost: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    pub fn find_template(&self, id: i64) -> rusqlite::Result<Option<String>> {
        let connection = self.connection.lock().unwrap();
        connection
            .query_row("SELECT template FROM entry WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
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

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;

         CREATE TABLE IF NOT EXISTS entry (
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

         CREATE INDEX IF NOT EXISTS idx_entry_enabled ON entry(enabled);",
    )
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
}
