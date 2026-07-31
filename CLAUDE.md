# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 專案概觀

QQKey 是 Windows 桌面常駐工具：按 `Alt+Q` 在任何視窗叫出候選框，搜尋命令後**填入**
（而非執行）原本的命令列，游標停在第一個待填參數處。Rust + Tauri v2 後端、
React + TypeScript 前端。純 Windows，後端大量直接呼叫 Win32。

## 常用指令

```powershell
npm install
npm run tauri dev      # 開發模式（會自動起 vite dev server on :1420）
npm run tauri build    # 產出 MSI / NSIS 安裝檔至 src-tauri/target/release/bundle/
npm run build          # 只做前端建置；同時是唯一的 TypeScript 型別檢查（tsc && vite build）
```

```powershell
cd src-tauri
cargo test                    # 後端單元測試
cargo test flips_above        # 單一測試（測試名稱皆為描述句，可直接當篩選字串）
cargo test --lib caret::      # 單一模組
```

換應用程式圖示：替換 `src-tauri/icons/source.png`（1024×1024）後執行
`npm run tauri icon src-tauri/icons/source.png`。

> 重啟 QQKey 時，前一個程序要完全結束後全域快捷鍵才會釋放。太快啟動新的會註冊失敗，
> 且 release 版沒有 console 看不到錯誤訊息。

## 架構

### 主要流程（跨多個檔案，先讀這段）

按下快捷鍵到文字落進終端機，牽涉一連串有嚴格順序的 Win32 呼叫：

1. `inject.rs::watch_foreground()` 在啟動時裝 `SetWinEventHook`，**持續**記錄前景視窗。
   不是按下快捷鍵當下才查 `GetForegroundWindow`——QQKey 自己的隱藏視窗在剛啟動時
   仍可能占著前景。
2. `hotkey.rs::show_launcher()` 依序做 `remember_foreground()` → `position_at_caret()`
   → `window.show()`。**前兩步必須在 `show()` 之前**：候選框一顯示就成了前景視窗，
   屆時就取不到目標視窗的 caret 了。
3. `caret.rs::locate()` 三層 fallback 取 caret 座標（`GetGUIThreadInfo` → UI Automation
   `TextPattern` → 視窗左下角），`caret.rs::place()` 再做螢幕邊界夾制。
   實測結論見 `spike/caret-probe/README.md`。
4. 前端選定後呼叫 `accept_candidate` → `template.rs::injectable_prefix()` 截斷佔位符
   → `template.rs::sanitize()` 剝除控制字元（`\r\n` 送到終端機就等同 Enter，
   命令會直接執行——這是「填入而不執行」的最後一道）→ `inject.rs::inject_text()`
   還原焦點（`SetForegroundWindow`，失敗則以 `AttachThreadInput` 繞過前景鎖定）
   → **輪詢 `GetForegroundWindow()` 確認前景真的換過去**（`wait_until()`，每 5ms 一次、
   上限 400ms，逾時回 Err 而不硬送）→ 睡 10ms 讓視窗內部焦點就緒
   → `SendInput` 逐個 UTF-16 送出。
5. 注入成功才 `record_use()`，失敗的嘗試不拉高排序。**失敗時候選框會重新顯示
   並把錯誤寫在框裡**——收了框又沒有字，使用者只會以為工具壞了。

### 雙視窗

`launcher`（無邊框、透明、置頂、失焦自動隱藏）在 `tauri.conf.json` 宣告；
`settings` **刻意不宣告**，由 `commands.rs::show_settings_window()` 動態建立——
即使設 `visible: false`，它在啟動時仍會被 Windows 當成前景視窗，害候選框把它
誤認為注入目標。兩個視窗各對應一個 Vite 入口（`index.html` / `settings.html`，
見 `vite.config.ts` 的 `rollupOptions.input`）。

關閉設定視窗是 `hide()` 而非銷毀（`prevent_close`）——QQKey 是常駐工具。

### 資料層

`store.rs`（SQLite，`%APPDATA%\com.jeremywen.qqkey\qqkey.db`）+ `state.rs`（`AppState`，
啟用中的條目全載入 `RwLock<Vec<Entry>>` 記憶體候選池，每次敲鍵的搜尋不必碰資料庫）。

**任何改動條目的操作都必須呼叫 `AppState::reload_pool()`**，否則排序與可見性
要等到下次啟動才反映。`record_use()` 例外——它就地更新單筆，不必重載整池。

schema 只有 `entry` 與 `meta` 兩張表。版本記在 SQLite 內建的 `user_version`，
`store.rs::migrate()` 依 `SCHEMA_VERSION` 逐段套用；**改動表結構時要 `SCHEMA_VERSION` +1
並在階梯尾端補一段**，只加 `CREATE TABLE IF NOT EXISTS` 對既有資料庫等於沒做。
版本比程式新時 `Store::open()` 直接回 Err（不試著降級），訊息會進啟動失敗對話框。
`meta` 存字串設定（快捷鍵、歷史匯入位移與開關、機密過濾樣式、候選框背景不透明度、
介面語言），key 常數定義在 `state.rs` 頂端。

`entry` 有兩個關鍵字欄位，語意不同：`keywords` 是**給人看的**（設定畫面的「搜尋
關鍵字」輸入框讀它，只放當前介面語言那一份），`keywords_all` 是**給模糊比對用的**
（內建目錄七個語言的聯集，`Entry::haystack()` 優先用它）。分成兩欄是因為把七語言塞進
`keywords` 的話，使用者一按存檔就把那團字變成自己的關鍵字，而條目同時轉成 user 來源、
從此不再被內建目錄更新。**`update_entry()` 與 `upsert_user()` 必須把 `keywords_all`
清成 NULL**——使用者接手之後就只該吃他填的字，不清的話他改成 `foo` 之後德文的舊關鍵字
照樣命中，而他從畫面上看不出為什麼。`keywords_all` 是衍生資料，刻意不進備份檔
（`BackupEntry` 沒有這一欄），還原後由 `AppState::resync_builtin()` 補回。

### 條目來源與優先序

`Source` 三種：`user` > `builtin` > `history`（`catalog/mod.rs::priority()`）。
幾條不變條件：

- 使用者在設定畫面**編輯過的 builtin 條目會轉為 user**，`sync_builtin()` 之後
  才不會把修改蓋回去（`store.rs::update_entry()` 的 `CASE source WHEN 'builtin'`）。
- `import_history()` 的 upsert 帶 `WHERE entry.source = 'history'`，不會蓋掉整理過的
  內建條目，也不會把實際使用累積的分數回沖。
- `export_entries()` **只匯出 user 來源**：內建目錄對方也有，歷史學來的可能夾帶工作內容。
- `upsert_user()`（匯入）刻意**不**帶 source 保護：使用者選了對方那一版，效果等同他
  自己編輯過。但它不覆寫 `enabled`——刻意停用的條目不該因為匯入就自己打開。

**匯出與備份是兩件事**，不要合併：`export_entries()` 是分享（只含 user，`SharedFile`），
`backup()` 是換機器（全來源 + 使用統計 + 整張 `meta`，`BackupFile`）。備份存的是
**未衰減**的原始 `score`——衰減是相對於「現在」算的，存快照等於把時間也凍進去。
`restore()` 是**取代**不是合併，整段在同一個交易裡。

### 排序

`ranking.rs`：`nucleo-matcher` 模糊比對分數 × frecency 加權。frecency 只存一個分數
加最後使用時間，每次使用先衰減（三十天半衰期）再加一，等同對歷次使用做指數加權。
**衰減在 Rust 端算**——SQLite 的數學函式要編譯時另外開啟，不能假設有。
比對目標是 `Entry::haystack()`（template + 關鍵字 + description 併起來），
所以中文關鍵字搜尋（輸入「掛載」找到 `usbipd attach --wsl`）才成立。內建條目用的是
`keywords_all`（七語言聯集），所以介面切成英文之後中文關鍵字仍然找得到——**這個承諾
由 `ranking.rs` 的 `chinese_keywords_still_match_when_the_ui_is_english` 守著**。
代價是 haystack 從約 39 字元長到約 115，短查詢會誤命中其他條目的歐語關鍵字；
`a_cross_language_keyword_does_not_outrank_a_real_match` 是那件事的回歸測試。

空查詢時只列出用過或手動加權過的條目，不塞一串沒用過的當噪音。

## 修改時要注意的接點

新增內建命令目錄
: 在 `src-tauri/resources/catalog/` 加 JSON，**同時**在 `catalog/builtin.rs` 的
  `CATALOGS` 加一行 `include_str!`（目錄是內嵌進執行檔的，不是執行時讀檔）。
  `description` 與 `keywords` 是**七個語言的 map**（`zh-Hant`/`zh-Hans`/`ja`/`en`/`fr`/
  `de`/`ko`），`template` 只寫一次——它是資料庫的 UNIQUE key，複製七份的失敗模式是靜默的。
  `builtin.rs` 的測試會擋下重複 template、解析不過的檔案，以及**任何語言缺譯**
  （`every_entry_is_translated_into_every_language` 檢查的是 `LangMap::get()`
  而不是 `load_builtin()` 的產物——後者有 fallback，缺譯會靜靜地變成英文或繁中）。

新增使用者可見的後端字串
: 在 `i18n.rs` 的 `messages!` 巨集裡加一條，七個語言一次寫齊。少一個語言巨集就
  不匹配、插值名字打錯 `format!` 就報錯——這是選手寫方案而不是 `rust-i18n` 的全部
  理由，所以**不要**改成執行期查表。「每個語言都有」不需要測試（編譯器擋著），
  但「每個語言都真的印出了插值」需要：加進
  `every_language_keeps_the_interpolated_values`，因為把
  `"Open launcher ({shortcut})"` 寫成 `"Open launcher"` 是合法的 `format!`。
  **`trace()` 與 log 字串不多語化**——讀者是開發者，固定語言才 grep 得到，
  而且同一份日誌不該前後兩種語言。

新增使用者可見的前端字串
: 七個 `src/i18n/locales/*.json` 都要加。漏一個會被 `resources.ts` 的型別標註擋下
  （`tsc` 會指名缺哪個鍵）；但它擋不到「多」出來的鍵，所以每個帶 `{{count}}` 的鍵
  一律七檔同時提供 `_one` 與 `_other`，中日韓兩者填相同文字。內嵌 `<code>`／`<strong>`
  的句子用 `<Trans components={{ code: <code /> }} />` 的**對照表**形式，不要用
  `<0>`／`<1>` 索引——譯者重排標籤的那一刻索引就錯了。

新增 IPC 指令
: `commands.rs` 加函式後，**必須**在 `lib.rs` 的 `invoke_handler!` 註冊。
  用到新的 Tauri plugin 能力時另需在 `src-tauri/capabilities/default.json` 加權限。

前後端型別
: `src/shared/types.ts` 手動對應後端 serde 結構。傳給前端的結構一律加
  `#[serde(rename_all = "camelCase")]`（含 `Candidate`、`EntryPage`、`EntryPatch`
  這幾個目前欄位都是單字、加不加看起來一樣的），`Source` 序列化為小寫字串。
  漏了的話，日後加一個 `last_used` 欄位時 Rust 送 snake_case、TS 讀 camelCase，
  `cargo build` 與 `tsc` 都不會有意見，執行期才變成 undefined。

分數顯示
: `Candidate::from_entry()` 與 `EntryView::from_entry()` 給的 `score` 是**衰減到
  當下**的值，跟排序用的是同一個。傳原始累計值會讓三個月沒碰的命令標著 ★10
  卻排在 ★3 後面——使用者看到的數字必須解釋得了他看到的順序。

佔位符邏輯有兩份
: Rust `template.rs::injectable_prefix()`（決定真正送出什麼）與 TS
  `types.ts::splitTemplate()`（決定 UI 灰字提示切在哪）。規則要一致。

控制字元有兩道
: 入口 `state.rs::check_template()` 擋下含控制字元的新增與匯入（講出問題），
  注入前 `template.rs::sanitize()` 再濾一次（資料庫裡已有髒資料也擋得住）。
  匯入是**整批拒絕**而非跳過壞的那幾筆——那是信任邊界，默默改掉別人給的東西更難追。

快捷鍵字串格式
: 是 `keyboard-types` 的 code 名稱——`Alt+KeyQ`、`Alt+Shift+KeyQ`。顯示給人看時
  用 `tray.rs::pretty()` 去掉 `Key` 前綴。`hotkey.rs::rebind()` 在新綁定失敗時會
  把舊的補回去。

失敗策略
: 啟動時快捷鍵註冊失敗、系統匣建立失敗、歷史匯入失敗都**只記錄不中止**——
  設定畫面是唯一能改綁的地方，那扇門得打得開。`AppState::secret_filter()`
  在自訂樣式無效時退回預設，不讓匯入停擺。**例外是資料庫**：開不起來就沒有
  候選池，`lib.rs::fatal_dialog()` 跳系統對話框說明原因與路徑後才退出，
  不留一個叫不出東西的空殼。

快捷鍵有兩個值
: `meta.shortcut` 是設定值，`AppState::active_shortcut()` 是**實際註冊成功**的那一個。
  設定的組合被佔用時會退回 `DEFAULT_SHORTCUT`，兩者就此分岔。系統匣顯示與
  `rebind()` 的解除目標都必須用後者——拿設定值去解除會解到一個從沒註冊成功的
  組合，退回註冊的那個從此賴在系統裡，使用者反而設不回預設值。
  空字串代表一個都沒註冊上，`tray.rs` 會顯示「快捷鍵未生效」。

改動條目的兩條路語意不同
: `update_entry()` 會把 builtin 轉成 user（使用者編輯過就不該再被內建目錄蓋回），
  `set_enabled()` 不轉。所以**單純開關啟用一律走 `set_enabled()`**，
  前端單筆與批次都是。走錯的話，按個「停用」就會讓內建條目被算進匯出檔。

## 多語言（`i18n.rs` + `src/i18n/`）

支援七個語言：`zh-Hant` `zh-Hans` `ja` `en` `fr` `de` `ko`。**這七個標籤在前後端逐字相同**
（Rust 的 `Lang` serde 名稱 = TS 的 `LANGUAGES`），不一致的話兩邊會各自 fallback
而且都不報錯，畫面上只看到一半換了語言。用正規 BCP 47 標籤是刻意的——前端可以
直接餵給 `Intl.*` 與 `<html lang>`，兩邊都不需要轉換表。

### 語系有三層

| 層 | 位置 | 角色 |
|---|---|---|
| 持久真值 | `meta.language` | `"auto"`（跟隨系統，預設）或某個標籤 |
| 熱快取 | `i18n::CURRENT`（`static RwLock<Lang>`） | 給拿不到 `AppState` 的地方讀 |
| 唯一寫入口 | `AppState::set_language()` | 同時寫 meta 與快取，兩者不可能分岔 |

之所以需要全域而不是各處傳參，是三個硬約束：`lib.rs` 的 `app.manage(state)` 發生在
`tray::setup()` **之後**；`store.rs::migrate()` 是自由函式，跑的時候 `Store` 還在建構；
而 `fatal_dialog()` 的定義就是 `AppState` 建不起來。這條路上語系**只能是系統語系**——
別把它「修正」成讀資料庫，那時 `meta` 讀不到。

系統語系用 Win32 `GetUserPreferredUILanguages`（**顯示語言**）而不是
`GetUserDefaultLocaleName`（那是日期數字的**地區格式**）。顯示語言設英文、地區設台灣
的機器在台灣企業環境很常見，用後者會把它們全判成中文，而測試抓不到這個錯
（只覆蓋 `match_tag`）。實測：繁中版 Windows 11 回報的是舊式的 `zh-TW`，不是
`zh-Hant-TW`，所以「只看主要語言子標籤」那一段是主路徑而非備用路徑。

`zh` 是**唯一**要再看 script 與 region 子標籤的語言（簡繁共用同一個主要子標籤），
其餘語言仍然只看第一段。判別採白名單：明確指向簡體的（`Hans`／`CN`／`SG`／`MY`）
才對到 `zh-Hans`，其餘的 `zh` 一律 `zh-Hant`——含 `zh-TW`／`zh-HK`／`zh-MO` 與裸 `zh`。
裸 `zh` 在 CLDR 的預設是簡體，這裡刻意不跟：Windows 的顯示語言從不回報這種形式，
而把一個判不出來的標籤丟給繁中使用者看簡體，比反過來更像故障。
**前端 `src/i18n/index.ts` 的 `matchTag()` 有同一份規則，兩邊要一起改。**

`i18n::set_current()` 必須是 `.setup()` 的第一行：更早（`run()` 開頭）plugin 還沒
初始化，偵測結果那行 `trace` 會被靜默丟掉；更晚就趕不上 `migrate()` 與 `fatal_dialog()`。

### 切換語言時要動的四個地方

`commands::set_language()` → `AppState::set_language()`（落地）→ `resync_builtin()`
（換內建目錄說明並重載候選池）→ `tray::refresh()`（**整個選單重建**，`TrayIcon` 沒有
`menu()` getter）→ 設定視窗 `set_title()` → `app.emit("app:language")`。
用全域 `emit` 而不是 `emit_to`：讓發起改動的設定視窗自己也走事件，套用語系就只有
一條路徑。`restore_from_file()` 走**同一組副作用**（`apply_language()`）——備份會覆寫
整張 meta 含 `language`，不推副作用的話畫面要到重啟才對得上。

### 刻意不做的事

- **使用者編輯過的條目切語言後停在舊語言。** 覆寫會破壞「編輯過就不再被蓋回」這條
  不變條件，等於切一次語言就靜默毀掉他自己寫的說明。想拿回內建版本就刪掉那一筆。
- **錯誤訊息在後端就翻好**，回傳 `Result<T, String>`。改成 `{code, params}` 交前端翻
  要動約 48 處 Rust 簽章與 15 處前端，而產物逐字相同（翻好的前綴 + 未翻的 rusqlite
  英文 detail）。日後真要拿 code 時，作法是 serialize-as-string 的 `AppError`，
  前端那 15 處不必動就能先繼續運作。
- **`hotkey.rs` 註冊設定快捷鍵失敗那條訊息不多語化**——它只進日誌，從不給使用者看。
  這類「看起來像使用者訊息、其實只進日誌」的要逐條確認，別多翻。

### 測試怎麼處理語系

`i18n::pin_for_tests()` 把 `CURRENT` 釘在繁中，呼叫點是 `state.rs::temp_state()`、
`store.rs::temp_store()` 與兩個自行 `Store::open()` 的測試。兩個理由：不釘的話
`active_language()` 會跟著開發機的 Windows 顯示語言跑；而 `CURRENT` 是 process 級的，
`cargo test` 的執行緒共用它——**所有測試釘同一個值**，競爭才無害。要驗證別的語言請用
顯式收 `Lang` 的純函式（`tray::show_label`、`messages!` 產出的每一支都是），不要動全域。

於是 `state.rs`／`store.rs`／`tray.rs` 那些以繁中字眼斷言錯誤內容的測試字面完全不動。
改繁中措辭時它們會壞——那是好事，改文案的人本來就該看一眼測試。

## 機密過濾（`catalog/history.rs`）

從 PSReadLine 歷史學習命令前，一律過濾疑似含憑證的行。兩條規則：
使用者可改的 `DEFAULT_SECRET_PATTERN` 關鍵字樣式，以及**不開放修改**的
`LONG_VALUE_PATTERN`（`=`／`:` 後接 20 字元以上長字串）。

策略是**寧可誤殺**：誤殺的命令可以自己補回來，漏放的憑證會一直留在資料庫裡。
關鍵字樣式刻意不加 `\b`，因為 `GITHUB_TOKEN`、`DB_PASSWORD` 這種底線命名會漏網。

只回報略過的**筆數**，內容不記錄。`inject.rs` 的前景追蹤 callback 也刻意不做診斷
輸出——每次切換視窗都會觸發，記錄標題等於把使用者一整天開過什麼全寫進 log。
同樣理由，`remember_foreground()` 只記 HWND 不記標題。

## 程式碼慣例

- 註解、文件、UI 文案、commit message 皆繁體中文台灣用語。註解寫「為什麼」，
  尤其是 Win32 呼叫順序、fallback 分支這類看不出理由的地方。
- 測試名稱是英文描述句（`flips_above_when_it_would_overflow_the_bottom`），
  assert 訊息用繁中說明期望。
- `crate::trace(scope, message)` 是後端診斷輸出，一律寫進日誌檔（`tauri-plugin-log`，
  Windows 位置是 `%LOCALAPPDATA%\com.jeremywen.qqkey\logs\`，注意不是資料庫所在的
  Roaming）。**日誌會留在磁碟上，所以不記錄視窗標題、不記錄注入內容**。
- 前端沒有 lint 設定；型別檢查靠 `npm run build`。無前端測試——所以
  `src/i18n/resources.ts` 的型別標註是七個語系檔一致性的唯一防線。
- UI 文案一律進 `src/i18n/locales/*.json` 或 `i18n.rs` 的 `messages!`，元件裡不留
  硬編碼字串（`console.error` 例外，那是開發者面向的）。`aria-label`、`title`、
  `placeholder` 與原生檔案對話框的 `title` 也算 UI 文案，最容易漏。

## 已知限制與依賴約束

- `rusqlite` **固定在 0.37**：0.40 會帶進 `libsqlite3-sys` 0.38，其 build script
  用了 unstable 的 `cfg_select`，在 Rust 1.92 stable 編不過。
- `tauri.conf.json` 不可加 `"center": true`——會在顯示時覆寫算好的座標。
  定位失敗才主動 `center()`。
- 候選框在中文輸入法模式下會進入注音組字狀態。搜尋需支援中文關鍵字，
  所以不宜直接停用輸入法。
- CSS 的字型堆疊**刻意不指名 CJK 字型**，改讓 Chromium 依 `<html lang>` 做語言感知
  fallback。拿掉 `Microsoft JhengHei UI` 看起來像退步，實際上留著會讓日文使用者拿到
  中文字形。代價是**要在裝有對應語言套件的機器上才驗得出來**——開發機沒裝日文字型
  會給出假的通過。
- `Cargo.toml` 的 `description` 會進 MSI/NSIS 與 exe 檔案屬性，是打包期決定、不隨
  執行期語言變，所以是語言中性的英文。安裝程式語言與應用程式語言是兩件事，
  不要試圖同步（真要多語安裝程式是 `tauri.conf.json` 的 `bundle.windows.nsis.languages`）。
- `restore()` 會覆寫 `meta.shortcut` 與 `meta.launcher_opacity` 卻不重新註冊快捷鍵、
  不推送不透明度（語系那一份已經在 `restore_from_file()` 補上了，這兩個還沒）。
  既有缺陷，未修。
- `spike/` 下兩個獨立 crate（`caret-probe`、`input-probe`）是 M0/M1 的驗證工具，
  不參與主專案建置，但 `caret-probe/README.md` 記著 caret 定位的實測結論，改
  `caret.rs` 前值得一讀。
