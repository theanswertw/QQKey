//! SQLite 儲存。資料庫放在 `%APPDATA%\com.jeremywen.qqkey\qqkey.db`，不對外傳送。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::catalog::{BackupEntry, Entry, EntryPatch, NewEntry, SharedEntry, Source};

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
                "INSERT INTO entry (template, description, keywords, keywords_all, source)
                 VALUES (?1, ?2, ?3, ?4, 'builtin')
                 ON CONFLICT(template) DO UPDATE SET
                   description   = excluded.description,
                   keywords      = excluded.keywords,
                   keywords_all  = excluded.keywords_all
                 WHERE entry.source = 'builtin'",
            )?;
            for entry in entries {
                written += statement.execute(params![
                    entry.template,
                    entry.description,
                    entry.keywords,
                    entry.keywords_all
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
            "SELECT id, template, description, keywords, keywords_all,
                    source, enabled, score, last_used, boost
             FROM entry {filter}"
        ))?;
        let rows = statement.query_map([], |row| {
            let source: String = row.get(5)?;
            Ok(Entry {
                id: row.get(0)?,
                template: row.get(1)?,
                description: row.get(2)?,
                keywords: row.get(3)?,
                keywords_all: row.get(4)?,
                source: Source::parse(&source),
                enabled: row.get::<_, i64>(6)? != 0,
                score: row.get(7)?,
                last_used: row.get(8)?,
                boost: row.get(9)?,
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
            // keywords_all 一併清掉。使用者把關鍵字改成 foo 之後，內建目錄那份
            // 六語言聯集必須失效——不清的話德文的舊關鍵字照樣命中這一筆，
            // 而他從設定畫面上看不出來為什麼。
            "UPDATE entry SET
               template     = ?1,
               description  = ?2,
               keywords     = ?3,
               keywords_all = NULL,
               enabled      = COALESCE(?4, enabled),
               boost        = COALESCE(?5, boost),
               source       = CASE source WHEN 'builtin' THEN 'user' ELSE source END
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

    /// 批次刪除，回傳實際刪掉的筆數。
    ///
    /// 歷史匯入一次會帶進上千筆，要清掉整批雜訊時一頁 40 筆逐一點不切實際。
    /// 包在同一個交易裡，中途失敗不會刪一半。
    pub fn delete_entries(&self, ids: &[i64]) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut deleted = 0;
        {
            let mut statement = transaction.prepare("DELETE FROM entry WHERE id = ?1")?;
            for id in ids {
                deleted += statement.execute([id])?;
            }
        }
        transaction.commit()?;
        Ok(deleted)
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
                   description  = excluded.description,
                   keywords     = excluded.keywords,
                   -- 理由同 update_entry：匯入的是對方整理過的單語關鍵字，
                   -- 內建目錄的六語言聯集不再適用這一筆。
                   keywords_all = NULL,
                   boost        = excluded.boost,
                   source       = 'user'",
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

    /// 整張 `meta` 表，供備份帶走。
    pub fn all_meta(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT key, value FROM meta")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// 以備份內容取代目前的資料。
    ///
    /// 是「取代」不是「合併」——還原備份的意思就是回到當時的狀態，
    /// 留下現在多出來的東西反而不是使用者要的。整段包在同一個交易裡，
    /// 中途失敗會整個回捲，不會留下一個清空到一半的資料庫。
    pub fn restore(
        &self,
        entries: &[BackupEntry],
        settings: &[(String, String)],
    ) -> rusqlite::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let mut written = 0;
        {
            transaction.execute("DELETE FROM entry", [])?;
            transaction.execute("DELETE FROM meta", [])?;

            // keywords_all 刻意不在備份檔裡，所以這裡留 NULL：它是從內建目錄
            // 算出來的衍生資料，還原後由 `AppState::resync_builtin()` 補回，
            // 存進備份只是讓每筆多背 60 個字元。
            let mut statement = transaction.prepare(
                "INSERT INTO entry
                   (template, description, keywords, source, enabled, score, last_used, boost)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for entry in entries {
                written += statement.execute(params![
                    entry.template,
                    entry.description,
                    entry.keywords,
                    entry.source.as_str(),
                    entry.enabled as i64,
                    entry.score,
                    entry.last_used,
                    entry.boost
                ])?;
            }

            let mut meta = transaction
                .prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
            for (key, value) in settings {
                meta.execute(params![key, value])?;
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
const SCHEMA_VERSION: i64 = 2;

/// 建立或升級 schema。版本記在 SQLite 內建的 `user_version`，不另外開表。
///
/// 從前這裡只有一組 `CREATE TABLE IF NOT EXISTS`。那對既有資料庫等於什麼
/// 都不做——只要哪天加一個欄位，舊資料庫的 SELECT 就會撞上 `no such column`，
/// 而且是在啟動時撞，使用者只會看到工具再也打不開。匯出檔案早就有版本號了，
/// 資料庫沒理由沒有。
fn migrate(connection: &Connection) -> Result<(), String> {
    // 這裡跑得比 `AppState` 早，語系只能讀 process 全域的那一份
    // （由 `lib.rs` 的 setup 開頭以系統語系填好）。
    let lang = crate::i18n::current();

    connection
        .execute_batch("PRAGMA journal_mode = WAL;")
        .map_err(|error| crate::i18n::db_journal_mode_failed(lang, &error.to_string()))?;

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| crate::i18n::db_read_version_failed(lang, &error.to_string()))?;

    if version > SCHEMA_VERSION {
        // 不試著降級。新版寫進去的欄位砍掉就是弄丟資料，寧可不開。
        return Err(crate::i18n::db_newer_version(lang, version, SCHEMA_VERSION));
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
            .map_err(|error| crate::i18n::db_create_tables_failed(lang, &error.to_string()))?;
    }

    if version < 2 {
        // 內建目錄六個語言的 keywords 聯集，只給 `Entry::haystack()` 用，
        // 讓「輸入『掛載』找到 usbipd attach」在介面切成英文之後仍然成立。
        //
        // 跟 `keywords` 分成兩欄而不是塞在一起：`keywords` 是設定畫面
        // 「搜尋關鍵字」輸入框讀的那一份，把六語言塞進去的話使用者一按存檔
        // 就把那團字變成自己的關鍵字，而條目同時轉成 user 來源、從此不再被
        // 內建目錄更新。
        connection
            .execute_batch("ALTER TABLE entry ADD COLUMN keywords_all TEXT;")
            .map_err(|error| crate::i18n::db_create_tables_failed(lang, &error.to_string()))?;
    }

    // 下一次改 schema 就接在這裡：if version < 3 { ...ALTER TABLE... }

    if version != SCHEMA_VERSION {
        // user_version 不吃參數綁定，只能組字串；SCHEMA_VERSION 是常數。
        connection
            .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(|error| crate::i18n::db_write_version_failed(lang, &error.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Store;
    use crate::catalog::{EntryPatch, NewEntry, Source};

    fn new_entry(template: &str, description: &str) -> NewEntry {
        NewEntry {
            template: template.to_string(),
            description: Some(description.to_string()),
            keywords: None,
            keywords_all: None,
        }
    }

    fn temp_store() -> (Store, tempfile::TempDir) {
        crate::i18n::pin_for_tests();
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
        crate::i18n::pin_for_tests();
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
        crate::i18n::pin_for_tests();
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

    /// schema 階梯第一次真正被走過。
    ///
    /// 失敗的症狀不是測試紅字而是「使用者升版之後工具再也打不開」——舊資料庫
    /// 少了新欄位，`load_where()` 的 SELECT 會撞上 `no such column`，而那發生在
    /// 啟動時，一路傳到啟動失敗對話框。
    #[test]
    fn migration_adds_the_keywords_all_column_to_a_v1_database() {
        crate::i18n::pin_for_tests();
        let dir = tempfile::tempdir().expect("建立暫存目錄");
        let path = dir.path().join("test.db");

        // 手工造一個 v1 的資料庫：舊的表結構、舊的版本章、以及一筆既有資料
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE entry (
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
                     CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                     INSERT INTO entry (template, description, keywords, source, score)
                       VALUES ('usbipd list', '列出裝置', 'usb 列表', 'builtin', 3.5);
                     INSERT INTO meta (key, value) VALUES ('shortcut', 'Alt+KeyW');
                     PRAGMA user_version = 1;",
                )
                .unwrap();
        }

        let store = Store::open(&path).expect("v1 資料庫必須升級得上來");

        let loaded = store.load_enabled().unwrap();
        assert_eq!(loaded.len(), 1, "升級不該弄丟既有資料");
        assert_eq!(loaded[0].template, "usbipd list");
        assert_eq!(loaded[0].score, 3.5, "使用統計要留著");
        assert_eq!(
            loaded[0].keywords_all, None,
            "舊資料的新欄位是 NULL，等下一次 sync_builtin 補"
        );
        assert_eq!(
            store.meta("shortcut").unwrap().as_deref(),
            Some("Alt+KeyW"),
            "meta 也不該被動到"
        );
    }

    /// 使用者改了關鍵字之後，內建目錄那份六語言聯集必須失效。
    ///
    /// 不清的話他把關鍵字改成 foo，德文的舊關鍵字照樣命中這一筆，
    /// 而他從設定畫面上看不出來為什麼。
    #[test]
    fn editing_keywords_clears_the_multilingual_union() {
        let (store, _dir) = temp_store();
        store
            .sync_builtin(&[NewEntry {
                template: "usbipd list".to_string(),
                description: Some("列出裝置".to_string()),
                keywords: Some("列表".to_string()),
                keywords_all: Some("列表 list liste 一覧 목록".to_string()),
            }])
            .unwrap();
        let id = store.load_enabled().unwrap()[0].id;

        store
            .update_entry(
                id,
                &EntryPatch {
                    template: "usbipd list".to_string(),
                    description: Some("列出裝置".to_string()),
                    keywords: Some("foo".to_string()),
                    enabled: None,
                    boost: None,
                },
            )
            .unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded[0].keywords, Some("foo".to_string()));
        assert_eq!(
            loaded[0].keywords_all, None,
            "使用者接手之後就只吃他填的關鍵字"
        );
        assert_eq!(
            loaded[0].haystack(),
            "usbipd list foo 列出裝置",
            "haystack 也不該再認得舊的多語關鍵字"
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
