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
   → 睡 40ms → `SendInput` 逐個 UTF-16 送出。
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
`meta` 存字串設定（快捷鍵、歷史匯入位移與開關、機密過濾樣式、候選框背景不透明度），
key 常數定義在 `state.rs` 頂端。

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
比對目標是 `Entry::haystack()`（template + keywords + description 併起來），
所以中文關鍵字搜尋（輸入「掛載」找到 `usbipd attach --wsl`）才成立。

空查詢時只列出用過或手動加權過的條目，不塞一串沒用過的當噪音。

## 修改時要注意的接點

新增內建命令目錄
: 在 `src-tauri/resources/catalog/` 加 JSON，**同時**在 `catalog/builtin.rs` 的
  `CATALOGS` 加一行 `include_str!`（目錄是內嵌進執行檔的，不是執行時讀檔）。
  `builtin.rs` 的測試會擋下重複 template 與缺少中文說明的條目。

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
- 前端沒有 lint 設定；型別檢查靠 `npm run build`。無前端測試。

## 已知限制與依賴約束

- `rusqlite` **固定在 0.37**：0.40 會帶進 `libsqlite3-sys` 0.38，其 build script
  用了 unstable 的 `cfg_select`，在 Rust 1.92 stable 編不過。
- `tauri.conf.json` 不可加 `"center": true`——會在顯示時覆寫算好的座標。
  定位失敗才主動 `center()`。
- 候選框在中文輸入法模式下會進入注音組字狀態。搜尋需支援中文關鍵字，
  所以不宜直接停用輸入法。
- `spike/` 下兩個獨立 crate（`caret-probe`、`input-probe`）是 M0/M1 的驗證工具，
  不參與主專案建置，但 `caret-probe/README.md` 記著 caret 定位的實測結論，改
  `caret.rs` 前值得一讀。
