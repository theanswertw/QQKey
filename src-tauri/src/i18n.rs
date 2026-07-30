//! 介面語系的判定與保存。
//!
//! 三個時間點需要語系，而各自可用的基礎設施不同：
//!
//! 1. `store.rs::migrate()` 與 `lib.rs::fatal_dialog()`——資料庫還開不起來，
//!    `meta` 讀不到，只有系統語系可依。
//! 2. `tray.rs::setup()`——狀態已載入，但 `app.manage(state)` 還沒發生，
//!    `app.state::<AppState>()` 會 panic。
//! 3. 前端 render——什麼都有。
//!
//! 所以判定一律不依賴 `AppHandle` 也不依賴資料庫，當前語系放在本模組的
//! process 級全域裡，拿不到 `AppState` 的地方都讀它。

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// 支援的語系。
///
/// 序列化字串是正規的 BCP 47 標籤，而且**與前端 i18next 的 locale code 逐字相同**。
/// 不一致的話兩邊會各自 fallback 而且都不報錯，畫面上只會看到一半換了語言。
/// 用正規標籤的另一個好處是前端可以直接餵給 `Intl.*` 與 `<html lang>`，
/// 兩邊都不需要轉換表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "zh-Hant")]
    ZhHant,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "de")]
    De,
    #[serde(rename = "ko")]
    Ko,
}

impl Lang {
    /// 順序即設定畫面下拉選單的順序。也供「每個語系都要有」的測試列舉用。
    pub const ALL: [Lang; 6] = [
        Lang::ZhHant,
        Lang::Ja,
        Lang::En,
        Lang::Fr,
        Lang::De,
        Lang::Ko,
    ];

    pub fn as_tag(self) -> &'static str {
        match self {
            Lang::ZhHant => "zh-Hant",
            Lang::Ja => "ja",
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Ko => "ko",
        }
    }

    /// 只認支援清單裡的標籤，用於驗證 `meta` 的值與 `set_language` 的輸入。
    /// 系統回報的 `zh-Hant-TW` 這種要走 [`match_tag`]。
    pub fn parse(tag: &str) -> Option<Self> {
        Lang::ALL
            .into_iter()
            .find(|lang| lang.as_tag().eq_ignore_ascii_case(tag))
    }
}

/// `meta.language` 存這個值代表「跟隨系統顯示語言」。也是預設值。
pub const AUTO: &str = "auto";

/// 語系改變時推給兩個 webview 的事件名。
///
/// 既有事件 `launcher:shown`、`launcher:opacity` 的前綴表示的是**受眾**而非主題，
/// 而語系的受眾是整個應用，所以用 `app:`。
pub const EVENT_LANGUAGE: &str = "app:language";

/// 認不出系統語系、或使用者選了一個我們沒有的語系時落到這裡。
///
/// 落英文而不是繁中：Windows 顯示語言是西班牙文／葡萄牙文的使用者讀英文的
/// 機率遠高於讀繁中，這也與前端 i18next 的 `fallbackLng` 一致。
///
/// 注意**內建目錄的說明文字不是這樣**——那條 fallback 鏈是
/// `<當前> → en → zh-Hant`（見 `catalog::builtin::LangMap::get`）。介面外框缺譯
/// 是不該發生的事（有測試擋），目錄缺一句德文說明卻很可能，而那時繁中比空白好。
/// 這個不對稱是刻意的。
pub const FALLBACK: Lang = Lang::En;

/// 當前語系。
///
/// `inject.rs`、`hotkey.rs`、`store.rs::migrate()`、`lib.rs::fatal_dialog()` 這些
/// 拿不到 `AppState` 的地方都讀這裡。唯一的寫入者是 `AppState::set_language()`
/// 與 `lib.rs` 的啟動流程——分散寫入的話某條路徑只更新一邊，症狀會是
/// 「重開才生效」或「重開又變回去」，兩者都很難歸因。
static CURRENT: RwLock<Lang> = RwLock::new(Lang::ZhHant);

/// 讀不到就退回英文而不 panic：這支會在 `fatal_dialog` 的路徑上被呼叫，
/// 那時 panic 等於使用者連「為什麼開不起來」都看不到。
pub fn current() -> Lang {
    CURRENT.read().map(|guard| *guard).unwrap_or(FALLBACK)
}

pub fn set_current(lang: Lang) {
    if let Ok(mut guard) = CURRENT.write() {
        *guard = lang;
    }
}

/// 測試一律把語系釘在繁中。
///
/// 兩個理由。一是 `AppState::active_language()` 在設定值為 `auto` 時會回報系統
/// 語系，不釘的話測試結果取決於開發機的 Windows 顯示語言。二是 [`CURRENT`] 是
/// process 級的，`cargo test` 的執行緒共用它——**所有測試都釘同一個值**，
/// 競爭才是無害的。
///
/// 要驗證別的語言請用顯式收 `Lang` 的純函式（`tray::show_label`、`messages!`
/// 產出的每一支都是），不要動這個全域。
#[cfg(test)]
pub(crate) fn pin_for_tests() {
    set_current(Lang::ZhHant);
}

/// 系統顯示語言。
///
/// 只查一次。Windows 的顯示語言要重新登入才會變，而同一次執行中前後不一致
/// 比稍微過期更糟——系統匣寫日文、候選框寫英文。
pub fn system_language() -> Lang {
    static CACHED: OnceLock<Lang> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let raw = detect().unwrap_or_default();
        let resolved = match_tag(&raw);
        // 這一行是日後唯一查得出「為什麼變成英文」的線索。
        // 呼叫點必須在 tauri 的 setup() 之內——plugin 是在那之前才初始化的，
        // 更早呼叫 log 巨集會被靜默丟掉。
        crate::trace("語系", &format!("系統回報 {raw:?} → {}", resolved.as_tag()));
        resolved
    })
}

/// `meta.language` 的值 → 實際生效的語系。
pub fn resolve(setting: Option<&str>) -> Lang {
    match setting {
        None | Some(AUTO) | Some("") => system_language(),
        // 認不出來的值（手改壞的資料庫、還原了較新版本的備份）比照
        // `AppState::launcher_opacity()` 的策略靜默退回，不讓一個壞值
        // 把整個介面變成一堆原始的 key。
        Some(value) => Lang::parse(value).unwrap_or_else(system_language),
    }
}

/// 把系統回報的標籤對到支援的六個之一。
///
/// 規則只有三條，刻意**不解析 script 與 region 子標籤**：
///
/// 1. 完全相同（不分大小寫）
/// 2. 主要語言子標籤相同：`fr-CA`→`fr`、`de-AT`→`de`、`zh-Hant-TW`→`zh-Hant`
/// 3. 其餘落到 [`FALLBACK`]
///
/// `zh` 一律對到 `zh-Hant`，**包含 `zh-Hans` 與 `zh-CN`**。我們沒有簡體，
/// 但對簡體讀者而言繁體遠比英文可讀；而且這條規則讓整個函式不必碰 script
/// 子標籤，少掉一整類標籤解析的錯誤。
fn match_tag(raw: &str) -> Lang {
    // Windows 給的是連字號形式，但備援路徑與某些設定會給底線，一併吃下來
    let tag = raw.replace('_', "-");
    if let Some(exact) = Lang::parse(&tag) {
        return exact;
    }
    match tag
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "zh" => Lang::ZhHant,
        primary => Lang::parse(primary).unwrap_or(FALLBACK),
    }
}

/// 取 Windows 的**顯示語言**（不是地區格式）。
///
/// `GetUserPreferredUILanguages` 給的才是「Windows 顯示語言」清單，也就是使用者
/// 期待應用程式跟著走的那一個。`GetUserDefaultLocaleName` 給的是地區格式
/// （日期、數字、貨幣）——一台顯示語言設英文、地區設台灣的機器兩者會給出不同
/// 答案，而那在台灣的企業環境很常見。**介面要跟前者**，後者只當取不到時的備援。
///
/// 別把這兩者的優先序調換：測試只覆蓋 `match_tag`，抓不到這個錯，
/// 而症狀是所有「顯示英文 + 地區台灣」的機器突然變成中文。
fn detect() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Globalization::{
        GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
    };

    unsafe {
        let mut count: u32 = 0;
        let mut chars: u32 = 0;
        // 第一次呼叫傳 None 且長度為 0，只是問緩衝區要多大
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut chars).is_ok()
            && chars > 0
        {
            let mut buffer = vec![0u16; chars as usize];
            if GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut chars,
            )
            .is_ok()
            {
                // 回傳的是以 NUL 分隔、雙 NUL 結尾的清單，第一個就是首選
                let first: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
                if !first.is_empty() {
                    return Some(String::from_utf16_lossy(&first));
                }
            }
        }

        // 備援。LOCALE_NAME_MAX_LENGTH 是 85，但它掛在 Win32_System_SystemServices
        // 底下，為一個常數多開一個 feature 不值得，直接寫死並註明來源。
        let mut name = [0u16; 85];
        let length = GetUserDefaultLocaleName(&mut name);
        if length > 1 {
            // 回傳長度含結尾的 NUL
            return Some(String::from_utf16_lossy(&name[..(length - 1) as usize]));
        }
    }
    None
}

// ------------------------------------------------------------ 使用者可見字串

/// 宣告一條使用者可見訊息，六個語系各一份。
///
/// 刻意不用 `rust-i18n` 這類以 YAML／JSON 查表的方案：那裡 key 打錯、少一個
/// 語系、插值參數掉一個，三者都是執行期靜默失敗，而症狀是使用者看到空字串
/// 或 `%{shortcut}` 字面。展開成「每語系一個 match 臂 + `format!`」之後三者
/// 全由編譯器擋下：key 不存在等於函式不存在、少一個 `Ko:` 等於巨集不匹配、
/// 插值名字打錯等於 `format!` 編譯錯誤。
///
/// 片段必須是 `literal` 而不是 `expr`——`format!` 只接受字面量當格式字串，
/// 收到 `expr` 片段會報「format argument must be a string literal」。
macro_rules! messages {
    ($(
        $(#[$doc:meta])*
        $name:ident ( $($arg:ident : $ty:ty),* ) {
            ZhHant: $zh:literal,
            Ja: $ja:literal,
            En: $en:literal,
            Fr: $fr:literal,
            De: $de:literal,
            Ko: $ko:literal,
        }
    )*) => { $(
        $(#[$doc])*
        pub fn $name(lang: Lang $(, $arg: $ty)*) -> String {
            match lang {
                Lang::ZhHant => format!($zh),
                Lang::Ja => format!($ja),
                Lang::En => format!($en),
                Lang::Fr => format!($fr),
                Lang::De => format!($de),
                Lang::Ko => format!($ko),
            }
        }
    )* };
}

messages! {
    // ---------------------------------------------------------------- 系統匣

    /// 系統匣選單：一個快捷鍵都沒註冊成功時。
    ///
    /// 不能照寫一個按了沒反應的組合——系統匣是使用者唯一找得回設定畫面的
    /// 地方，得讓他知道要去改綁。
    tray_show_inactive() {
        ZhHant: "叫出候選框（快捷鍵未生效）",
        Ja: "候補ウィンドウを開く（ショートカット無効）",
        En: "Open launcher (shortcut inactive)",
        Fr: "Ouvrir le lanceur (raccourci inactif)",
        De: "Auswahlfenster öffnen (Tastenkürzel inaktiv)",
        Ko: "후보 창 열기(단축키 작동 안 함)",
    }

    tray_show(shortcut: &str) {
        ZhHant: "叫出候選框（{shortcut}）",
        Ja: "候補ウィンドウを開く（{shortcut}）",
        En: "Open launcher ({shortcut})",
        Fr: "Ouvrir le lanceur ({shortcut})",
        De: "Auswahlfenster öffnen ({shortcut})",
        Ko: "후보 창 열기({shortcut})",
    }

    tray_settings(shortcut: &str) {
        ZhHant: "設定（{shortcut}）",
        Ja: "設定（{shortcut}）",
        En: "Settings ({shortcut})",
        Fr: "Paramètres ({shortcut})",
        De: "Einstellungen ({shortcut})",
        Ko: "설정({shortcut})",
    }

    tray_quit() {
        ZhHant: "結束 QQKey",
        Ja: "QQKey を終了",
        En: "Quit QQKey",
        Fr: "Quitter QQKey",
        De: "QQKey beenden",
        Ko: "QQKey 종료",
    }

    tray_tooltip_inactive() {
        ZhHant: "QQKey — 快捷鍵未生效，請從設定改綁",
        Ja: "QQKey — ショートカットが無効です。設定で変更してください",
        En: "QQKey — shortcut inactive, rebind it in Settings",
        Fr: "QQKey — raccourci inactif, redéfinissez-le dans les paramètres",
        De: "QQKey — Tastenkürzel inaktiv, in den Einstellungen neu belegen",
        Ko: "QQKey — 단축키가 작동하지 않습니다. 설정에서 다시 지정하세요",
    }

    tray_tooltip(shortcut: &str) {
        ZhHant: "QQKey — {shortcut} 叫出候選框",
        Ja: "QQKey — {shortcut} で候補ウィンドウを開く",
        En: "QQKey — {shortcut} opens the launcher",
        Fr: "QQKey — {shortcut} ouvre le lanceur",
        De: "QQKey — {shortcut} öffnet das Auswahlfenster",
        Ko: "QQKey — {shortcut} 로 후보 창 열기",
    }

    // ---------------------------------------------------------------- 視窗標題

    settings_window_title() {
        ZhHant: "QQKey 設定",
        Ja: "QQKey 設定",
        En: "QQKey Settings",
        Fr: "Paramètres QQKey",
        De: "QQKey-Einstellungen",
        Ko: "QQKey 설정",
    }

    // ------------------------------------------------------------ 啟動失敗對話框
    //
    // 這一整段只能用系統語系（`system_language()`）：它們在資料庫開起來之前
    // 就產生，`meta` 讀不到。別把它改成讀使用者設定值——那條路連資料夾位置
    // 都還取不到。

    fatal_caption() {
        ZhHant: "QQKey 無法啟動",
        Ja: "QQKey を起動できません",
        En: "QQKey cannot start",
        Fr: "QQKey ne peut pas démarrer",
        De: "QQKey kann nicht starten",
        Ko: "QQKey를 시작할 수 없습니다",
    }

    /// 錯誤訊息要讓使用者自己判斷得出是什麼問題（磁碟滿？權限？檔案壞了？），
    /// 所以連資料庫路徑一起帶出去——他至少能去那個位置看一眼。
    fatal_no_data_dir(error: &str) {
        ZhHant: "取不到資料夾位置：{error}",
        Ja: "フォルダーの場所を取得できません：{error}",
        En: "Could not determine the data folder: {error}",
        Fr: "Impossible de déterminer le dossier de données : {error}",
        De: "Der Datenordner konnte nicht ermittelt werden: {error}",
        Ko: "데이터 폴더 위치를 확인할 수 없습니다: {error}",
    }

    fatal_open_database(error: &str, path: &str) {
        ZhHant: "開啟資料庫失敗：{error}\n\n{path}",
        Ja: "データベースを開けませんでした：{error}\n\n{path}",
        En: "Failed to open the database: {error}\n\n{path}",
        Fr: "Échec de l'ouverture de la base de données : {error}\n\n{path}",
        De: "Die Datenbank konnte nicht geöffnet werden: {error}\n\n{path}",
        Ko: "데이터베이스를 열지 못했습니다: {error}\n\n{path}",
    }

    fatal_load_pool(error: &str, path: &str) {
        ZhHant: "載入候選池失敗：{error}\n\n{path}",
        Ja: "候補プールの読み込みに失敗しました：{error}\n\n{path}",
        En: "Failed to load the candidate pool: {error}\n\n{path}",
        Fr: "Échec du chargement du pool de candidats : {error}\n\n{path}",
        De: "Der Kandidatenpool konnte nicht geladen werden: {error}\n\n{path}",
        Ko: "후보 목록을 불러오지 못했습니다: {error}\n\n{path}",
    }

    // ------------------------------------------------------------ 資料庫 migration
    //
    // `store.rs::migrate()` 刻意回傳 `String` 而不是 `rusqlite::Error`，
    // 就是為了讓這幾句話直接進上面那個對話框。

    db_journal_mode_failed(error: &str) {
        ZhHant: "設定 journal 模式失敗：{error}",
        Ja: "journal モードの設定に失敗しました：{error}",
        En: "Failed to set the journal mode: {error}",
        Fr: "Échec de la configuration du mode journal : {error}",
        De: "Der Journal-Modus konnte nicht gesetzt werden: {error}",
        Ko: "journal 모드를 설정하지 못했습니다: {error}",
    }

    db_read_version_failed(error: &str) {
        ZhHant: "讀取 schema 版本失敗：{error}",
        Ja: "スキーマバージョンの読み取りに失敗しました：{error}",
        En: "Failed to read the schema version: {error}",
        Fr: "Échec de la lecture de la version du schéma : {error}",
        De: "Die Schema-Version konnte nicht gelesen werden: {error}",
        Ko: "스키마 버전을 읽지 못했습니다: {error}",
    }

    /// 不試著降級：新版寫進去的欄位砍掉就是弄丟資料，寧可不開。
    db_newer_version(found: i64, known: i64) {
        ZhHant: "這個資料庫是較新版本的 QQKey 建立的（schema v{found}，本程式只認得 v{known}）。請改用新版，或把資料庫移到別處讓 QQKey 重建一個。",
        Ja: "このデータベースは新しいバージョンの QQKey が作成したものです（schema v{found}、このプログラムが認識できるのは v{known} まで）。新しいバージョンを使うか、データベースを別の場所へ移して QQKey に作り直させてください。",
        En: "This database was created by a newer version of QQKey (schema v{found}; this build only understands v{known}). Please use the newer version, or move the database elsewhere so QQKey can rebuild one.",
        Fr: "Cette base de données a été créée par une version plus récente de QQKey (schéma v{found} ; cette version ne reconnaît que v{known}). Utilisez la version plus récente, ou déplacez la base de données ailleurs pour que QQKey en recrée une.",
        De: "Diese Datenbank wurde von einer neueren QQKey-Version erstellt (Schema v{found}; dieses Programm kennt nur v{known}). Verwenden Sie die neuere Version, oder verschieben Sie die Datenbank, damit QQKey eine neue anlegt.",
        Ko: "이 데이터베이스는 더 새로운 버전의 QQKey가 만든 것입니다(schema v{found}, 이 프로그램이 아는 것은 v{known}까지). 새 버전을 사용하거나, 데이터베이스를 다른 곳으로 옮겨 QQKey가 새로 만들도록 하세요.",
    }

    db_create_tables_failed(error: &str) {
        ZhHant: "建立資料表失敗：{error}",
        Ja: "テーブルの作成に失敗しました：{error}",
        En: "Failed to create the tables: {error}",
        Fr: "Échec de la création des tables : {error}",
        De: "Die Tabellen konnten nicht erstellt werden: {error}",
        Ko: "테이블을 만들지 못했습니다: {error}",
    }

    db_write_version_failed(error: &str) {
        ZhHant: "寫入 schema 版本失敗：{error}",
        Ja: "スキーマバージョンの書き込みに失敗しました：{error}",
        En: "Failed to write the schema version: {error}",
        Fr: "Échec de l'écriture de la version du schéma : {error}",
        De: "Die Schema-Version konnte nicht geschrieben werden: {error}",
        Ko: "스키마 버전을 기록하지 못했습니다: {error}",
    }

    // ------------------------------------------------------------ 條目與匯入匯出

    entry_not_found(id: i64) {
        ZhHant: "找不到 id 為 {id} 的命令",
        Ja: "id が {id} のコマンドが見つかりません",
        En: "No command with id {id}",
        Fr: "Aucune commande avec l'identifiant {id}",
        De: "Kein Befehl mit der ID {id}",
        Ko: "id가 {id}인 명령을 찾을 수 없습니다",
    }

    invalid_json(error: &str) {
        ZhHant: "JSON 格式不正確：{error}",
        Ja: "JSON の形式が正しくありません：{error}",
        En: "Malformed JSON: {error}",
        Fr: "JSON mal formé : {error}",
        De: "Ungültiges JSON: {error}",
        Ko: "JSON 형식이 올바르지 않습니다: {error}",
    }

    shared_file_newer(version: u32) {
        ZhHant: "這個檔案是較新的格式（version {version}），請先更新 QQKey",
        Ja: "このファイルは新しい形式です（version {version}）。先に QQKey を更新してください",
        En: "This file uses a newer format (version {version}); please update QQKey first",
        Fr: "Ce fichier utilise un format plus récent (version {version}) ; mettez d'abord QQKey à jour",
        De: "Diese Datei verwendet ein neueres Format (Version {version}); aktualisieren Sie zuerst QQKey",
        Ko: "이 파일은 더 새로운 형식입니다(version {version}). QQKey를 먼저 업데이트하세요",
    }

    invalid_backup(error: &str) {
        ZhHant: "不是有效的備份檔：{error}",
        Ja: "有効なバックアップファイルではありません：{error}",
        En: "Not a valid backup file: {error}",
        Fr: "Fichier de sauvegarde non valide : {error}",
        De: "Keine gültige Sicherungsdatei: {error}",
        Ko: "올바른 백업 파일이 아닙니다: {error}",
    }

    backup_newer(version: u32) {
        ZhHant: "這個備份是較新的格式（version {version}），請先更新 QQKey",
        Ja: "このバックアップは新しい形式です（version {version}）。先に QQKey を更新してください",
        En: "This backup uses a newer format (version {version}); please update QQKey first",
        Fr: "Cette sauvegarde utilise un format plus récent (version {version}) ; mettez d'abord QQKey à jour",
        De: "Diese Sicherung verwendet ein neueres Format (Version {version}); aktualisieren Sie zuerst QQKey",
        Ko: "이 백업은 더 새로운 형식입니다(version {version}). QQKey를 먼저 업데이트하세요",
    }

    /// 手動加權只收非負的有限數。負值會讓排序權重的 `ln()` 得出 NaN，
    /// 而那筆命令會卡在原始順序上——症狀隱晦到使用者幾乎不可能自己歸因，
    /// 所以要明講替代做法。
    boost_not_finite() {
        ZhHant: "手動加權要是一個有限的數字",
        Ja: "手動の重みづけは有限の数値でなければなりません",
        En: "The manual boost must be a finite number",
        Fr: "La pondération manuelle doit être un nombre fini",
        De: "Die manuelle Gewichtung muss eine endliche Zahl sein",
        Ko: "수동 가중치는 유한한 숫자여야 합니다",
    }

    boost_negative(boost: f64) {
        ZhHant: "手動加權不能是負數（收到 {boost}）。想讓某筆命令不要出現，請改用「停用」。",
        Ja: "手動の重みづけに負の数は使えません（{boost} を受け取りました）。特定のコマンドを表示させたくない場合は「無効」を使ってください。",
        En: "The manual boost cannot be negative (got {boost}). To keep a command out of the launcher, disable it instead.",
        Fr: "La pondération manuelle ne peut pas être négative ({boost} reçu). Pour qu'une commande n'apparaisse pas, désactivez-la plutôt.",
        De: "Die manuelle Gewichtung darf nicht negativ sein ({boost} erhalten). Um einen Befehl auszublenden, deaktivieren Sie ihn stattdessen.",
        Ko: "수동 가중치는 음수일 수 없습니다({boost}을 받았습니다). 특정 명령을 나타나지 않게 하려면 대신 사용 해제하세요.",
    }

    /// 換行送進終端機就等同按下 Enter，命令會直接執行——這是「填入而不執行」
    /// 的第一道防線，所以要在入口就講出問題，而不是事後默默改掉使用者的東西。
    template_has_control_chars(escaped: &str) {
        ZhHant: "命令裡有換行或 Tab 這類控制字元，不能存下來——送進終端機時換行等同按下 Enter。\n\n{escaped}",
        Ja: "コマンドに改行やタブなどの制御文字が含まれているため保存できません。ターミナルに送ると改行は Enter と同じ働きをします。\n\n{escaped}",
        En: "The command contains control characters such as a newline or tab and cannot be saved — sent to a terminal, a newline is the same as pressing Enter.\n\n{escaped}",
        Fr: "La commande contient des caractères de contrôle (retour à la ligne, tabulation) et ne peut pas être enregistrée — envoyé à un terminal, un retour à la ligne équivaut à appuyer sur Entrée.\n\n{escaped}",
        De: "Der Befehl enthält Steuerzeichen wie Zeilenumbruch oder Tabulator und kann nicht gespeichert werden — an ein Terminal gesendet entspricht ein Zeilenumbruch der Eingabetaste.\n\n{escaped}",
        Ko: "명령에 줄바꿈이나 탭 같은 제어 문자가 있어 저장할 수 없습니다. 터미널로 보내면 줄바꿈은 Enter를 누른 것과 같습니다.\n\n{escaped}",
    }

    // ---------------------------------------------------------------- 一般設定

    invalid_regex(error: &str) {
        ZhHant: "不是有效的正規表示式：{error}",
        Ja: "有効な正規表現ではありません：{error}",
        En: "Not a valid regular expression: {error}",
        Fr: "Expression régulière non valide : {error}",
        De: "Kein gültiger regulärer Ausdruck: {error}",
        Ko: "올바른 정규 표현식이 아닙니다: {error}",
    }

    opacity_out_of_range(min: u8, max: u8, got: u8) {
        ZhHant: "不透明度必須介於 {min}–{max}%，收到 {got}%",
        Ja: "不透明度は {min}–{max}% の範囲でなければなりません（{got}% を受け取りました）",
        En: "Opacity must be between {min}% and {max}%, got {got}%",
        Fr: "L'opacité doit être comprise entre {min} % et {max} %, reçu {got} %",
        De: "Die Deckkraft muss zwischen {min} % und {max} % liegen, erhalten {got} %",
        Ko: "불투명도는 {min}–{max}% 사이여야 합니다({got}%를 받았습니다)",
    }

    unsupported_language(value: &str) {
        ZhHant: "不支援的語言：{value}",
        Ja: "サポートされていない言語です：{value}",
        En: "Unsupported language: {value}",
        Fr: "Langue non prise en charge : {value}",
        De: "Nicht unterstützte Sprache: {value}",
        Ko: "지원하지 않는 언어입니다: {value}",
    }

    // ------------------------------------------------------------ 檔案與外部程式

    write_failed(path: &str, error: &str) {
        ZhHant: "寫入 {path} 失敗：{error}",
        Ja: "{path} への書き込みに失敗しました：{error}",
        En: "Failed to write {path}: {error}",
        Fr: "Échec de l'écriture de {path} : {error}",
        De: "{path} konnte nicht geschrieben werden: {error}",
        Ko: "{path}에 쓰지 못했습니다: {error}",
    }

    read_failed(path: &str, error: &str) {
        ZhHant: "讀取 {path} 失敗：{error}",
        Ja: "{path} の読み込みに失敗しました：{error}",
        En: "Failed to read {path}: {error}",
        Fr: "Échec de la lecture de {path} : {error}",
        De: "{path} konnte nicht gelesen werden: {error}",
        Ko: "{path}을 읽지 못했습니다: {error}",
    }

    no_log_dir(error: &str) {
        ZhHant: "取不到日誌資料夾位置：{error}",
        Ja: "ログフォルダーの場所を取得できません：{error}",
        En: "Could not determine the log folder: {error}",
        Fr: "Impossible de déterminer le dossier des journaux : {error}",
        De: "Der Protokollordner konnte nicht ermittelt werden: {error}",
        Ko: "로그 폴더 위치를 확인할 수 없습니다: {error}",
    }

    create_log_dir_failed(error: &str) {
        ZhHant: "建立日誌資料夾失敗：{error}",
        Ja: "ログフォルダーの作成に失敗しました：{error}",
        En: "Failed to create the log folder: {error}",
        Fr: "Échec de la création du dossier des journaux : {error}",
        De: "Der Protokollordner konnte nicht erstellt werden: {error}",
        Ko: "로그 폴더를 만들지 못했습니다: {error}",
    }

    open_log_dir_failed(error: &str) {
        ZhHant: "開啟日誌資料夾失敗：{error}",
        Ja: "ログフォルダーを開けませんでした：{error}",
        En: "Failed to open the log folder: {error}",
        Fr: "Échec de l'ouverture du dossier des journaux : {error}",
        De: "Der Protokollordner konnte nicht geöffnet werden: {error}",
        Ko: "로그 폴더를 열지 못했습니다: {error}",
    }

    /// 收的是前端傳進來的字串，放行條件要自己守——explorer 拿到本機路徑會開
    /// 檔案總管、拿到檔案會執行關聯程式。
    link_not_allowed(target: &str) {
        ZhHant: "不允許開啟這個連結：{target}",
        Ja: "このリンクは開けません：{target}",
        En: "Opening this link is not allowed: {target}",
        Fr: "L'ouverture de ce lien n'est pas autorisée : {target}",
        De: "Das Öffnen dieses Links ist nicht erlaubt: {target}",
        Ko: "이 링크는 열 수 없습니다: {target}",
    }

    open_link_failed(error: &str) {
        ZhHant: "開啟連結失敗：{error}",
        Ja: "リンクを開けませんでした：{error}",
        En: "Failed to open the link: {error}",
        Fr: "Échec de l'ouverture du lien : {error}",
        De: "Der Link konnte nicht geöffnet werden: {error}",
        Ko: "링크를 열지 못했습니다: {error}",
    }

    // ---------------------------------------------------------------- 注入
    //
    // 這三條會顯示在候選框裡（注入失敗時框會重新叫回來），不是 toast。

    no_target_window() {
        ZhHant: "沒有記錄到要送回的視窗",
        Ja: "送り先のウィンドウが記録されていません",
        En: "No target window was recorded",
        Fr: "Aucune fenêtre cible n'a été enregistrée",
        De: "Es wurde kein Zielfenster erfasst",
        Ko: "보낼 대상 창이 기록되지 않았습니다",
    }

    restore_focus_failed() {
        ZhHant: "無法把焦點還原到原視窗",
        Ja: "元のウィンドウにフォーカスを戻せません",
        En: "Could not return focus to the original window",
        Fr: "Impossible de redonner le focus à la fenêtre d'origine",
        De: "Der Fokus konnte nicht zum ursprünglichen Fenster zurückgegeben werden",
        Ko: "원래 창으로 포커스를 되돌릴 수 없습니다",
    }

    /// 以系統管理員身分開的終端機會擋下 `SendInput`，而那正是 usbipd 這類
    /// 命令的日常情境。
    input_partially_sent(sent: u32, expected: u32) {
        ZhHant: "鍵盤事件只送出 {sent}/{expected} 個，可能被攔截",
        Ja: "キーボードイベントを {sent}/{expected} 個しか送信できませんでした。ブロックされている可能性があります",
        En: "Only {sent} of {expected} keyboard events were sent; something may be blocking them",
        Fr: "Seuls {sent} événements clavier sur {expected} ont été envoyés ; ils sont peut-être bloqués",
        De: "Nur {sent} von {expected} Tastaturereignissen wurden gesendet; möglicherweise werden sie blockiert",
        Ko: "키보드 이벤트를 {expected}개 중 {sent}개만 보냈습니다. 차단되었을 수 있습니다",
    }

    // ---------------------------------------------------------------- 快捷鍵

    shortcut_parse_failed(value: &str, error: &str) {
        ZhHant: "無法解析快捷鍵 {value:?}：{error}",
        Ja: "ショートカット {value:?} を解析できません：{error}",
        En: "Could not parse the shortcut {value:?}: {error}",
        Fr: "Impossible d'analyser le raccourci {value:?} : {error}",
        De: "Das Tastenkürzel {value:?} konnte nicht ausgewertet werden: {error}",
        Ko: "단축키 {value:?}를 해석할 수 없습니다: {error}",
    }

    shortcut_register_failed(value: &str, error: &str) {
        ZhHant: "註冊 {value} 失敗（可能已被其他程式佔用）：{error}",
        Ja: "{value} の登録に失敗しました（他のプログラムが使用している可能性があります）：{error}",
        En: "Failed to register {value} (another program may already be using it): {error}",
        Fr: "Échec de l'enregistrement de {value} (un autre programme l'utilise peut-être déjà) : {error}",
        De: "{value} konnte nicht registriert werden (möglicherweise wird es von einem anderen Programm verwendet): {error}",
        Ko: "{value} 등록에 실패했습니다(다른 프로그램이 사용 중일 수 있습니다): {error}",
    }
}

#[cfg(test)]
mod tests {
    use super::{match_tag, resolve, system_language, Lang};

    #[test]
    fn maps_the_tags_windows_actually_reports() {
        // 實測：繁中版 Windows 11 的 GetUserPreferredUILanguages 回報的是舊式的
        // `zh-TW`，**不是** `zh-Hant-TW`。所以規則二（只看主要語言子標籤）不是
        // 備用路徑而是主路徑，別把它當成可以簡化掉的東西。
        assert_eq!(match_tag("zh-TW"), Lang::ZhHant);
        assert_eq!(match_tag("zh-Hant-TW"), Lang::ZhHant);
        assert_eq!(match_tag("ja-JP"), Lang::Ja);
        assert_eq!(match_tag("en-GB"), Lang::En);
        assert_eq!(
            match_tag("de-AT"),
            Lang::De,
            "奧地利德語沒有自己的語系檔，要落到 de"
        );
        assert_eq!(match_tag("fr-CA"), Lang::Fr);
        assert_eq!(match_tag("ko-KR"), Lang::Ko);
    }

    #[test]
    fn simplified_chinese_prefers_traditional_over_english() {
        assert_eq!(
            match_tag("zh-Hans-CN"),
            Lang::ZhHant,
            "簡體讀者看繁體遠比看英文可讀"
        );
        assert_eq!(match_tag("zh-CN"), Lang::ZhHant);
    }

    #[test]
    fn unsupported_languages_land_on_english() {
        assert_eq!(match_tag("es-ES"), Lang::En);
        assert_eq!(match_tag("pt-BR"), Lang::En);
        assert_eq!(match_tag(""), Lang::En, "取不到系統語系時不能 panic");
        assert_eq!(match_tag("???"), Lang::En);
    }

    #[test]
    fn tolerates_the_underscore_form_and_odd_casing() {
        assert_eq!(match_tag("ja_JP"), Lang::Ja);
        assert_eq!(match_tag("ZH-HANT"), Lang::ZhHant);
        assert_eq!(Lang::parse("EN"), Some(Lang::En));
        assert_eq!(Lang::parse("zh-Hant-TW"), None, "parse 只認支援清單裡的標籤");
    }

    /// 每個語系都要把插值真的印出來。
    ///
    /// 這擋的是 `format!` 擋不到的那一類錯：把 `"Open launcher ({shortcut})"`
    /// 寫成 `"Open launcher"` 是合法的 `format!`，編譯得過，但使用者會看到一個
    /// 沒有快捷鍵的選單項、或一則沒說是哪個檔案失敗的錯誤訊息。
    ///
    /// 順便驗證沒有哪個語系是空字串——巨集擋得住「少一個語系」，擋不住
    /// 「填了一個空字串」。
    macro_rules! keeps {
        ($name:ident ( $($arg:expr),* ) $(, $needle:expr)* ) => {
            for lang in Lang::ALL {
                let rendered = super::$name(lang $(, $arg)*);
                assert!(
                    !rendered.trim().is_empty(),
                    "{} 的 {} 是空的",
                    lang.as_tag(),
                    stringify!($name)
                );
                $(
                    assert!(
                        rendered.contains($needle),
                        "{} 的 {} 沒有印出 {}：{rendered}",
                        lang.as_tag(),
                        stringify!($name),
                        $needle
                    );
                )*
            }
        };
    }

    /// 給插值用的哨兵。挑一個絕不會出現在任何譯文裡的字串。
    const SENTINEL: &str = "§SC§";

    #[test]
    fn every_language_keeps_the_interpolated_values() {
        keeps!(tray_show_inactive());
        keeps!(tray_show(SENTINEL), SENTINEL);
        keeps!(tray_settings(SENTINEL), SENTINEL);
        keeps!(tray_quit());
        keeps!(tray_tooltip_inactive());
        keeps!(tray_tooltip(SENTINEL), SENTINEL);
        keeps!(settings_window_title());

        keeps!(fatal_caption());
        keeps!(fatal_no_data_dir(SENTINEL), SENTINEL);
        keeps!(fatal_open_database(SENTINEL, "§PATH§"), SENTINEL, "§PATH§");
        keeps!(fatal_load_pool(SENTINEL, "§PATH§"), SENTINEL, "§PATH§");

        keeps!(db_journal_mode_failed(SENTINEL), SENTINEL);
        keeps!(db_read_version_failed(SENTINEL), SENTINEL);
        keeps!(db_newer_version(4242, 7), "4242", "7");
        keeps!(db_create_tables_failed(SENTINEL), SENTINEL);
        keeps!(db_write_version_failed(SENTINEL), SENTINEL);

        keeps!(entry_not_found(4242), "4242");
        keeps!(invalid_json(SENTINEL), SENTINEL);
        keeps!(shared_file_newer(4242), "4242");
        keeps!(invalid_backup(SENTINEL), SENTINEL);
        keeps!(backup_newer(4242), "4242");
        keeps!(boost_not_finite());
        keeps!(boost_negative(-4.25), "-4.25");
        keeps!(template_has_control_chars(SENTINEL), SENTINEL);

        keeps!(invalid_regex(SENTINEL), SENTINEL);
        keeps!(opacity_out_of_range(11, 22, 33), "11", "22", "33");
        keeps!(unsupported_language(SENTINEL), SENTINEL);

        keeps!(write_failed("§PATH§", SENTINEL), "§PATH§", SENTINEL);
        keeps!(read_failed("§PATH§", SENTINEL), "§PATH§", SENTINEL);
        keeps!(no_log_dir(SENTINEL), SENTINEL);
        keeps!(create_log_dir_failed(SENTINEL), SENTINEL);
        keeps!(open_log_dir_failed(SENTINEL), SENTINEL);
        keeps!(link_not_allowed(SENTINEL), SENTINEL);
        keeps!(open_link_failed(SENTINEL), SENTINEL);

        keeps!(no_target_window());
        keeps!(restore_focus_failed());
        keeps!(input_partially_sent(41, 99), "41", "99");

        keeps!(shortcut_parse_failed(SENTINEL, "§E§"), SENTINEL, "§E§");
        keeps!(shortcut_register_failed(SENTINEL, "§E§"), SENTINEL, "§E§");
    }

    #[test]
    fn a_missing_or_corrupted_setting_falls_back_to_following_the_system() {
        // 手改壞資料庫，或還原了較新版本 QQKey 寫的備份
        assert_eq!(resolve(Some("klingon")), system_language());
        assert_eq!(resolve(None), system_language());
        assert_eq!(resolve(Some("")), system_language());
        assert_eq!(resolve(Some(super::AUTO)), system_language());
        assert_eq!(resolve(Some("ja")), Lang::Ja);
    }
}
