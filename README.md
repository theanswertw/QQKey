# QQKey

在任何視窗按下 **Alt+Q** 叫出候選框，鍵入關鍵字即列出對應命令，以方向鍵或 `Alt+數字` 選取後，
命令會**填入**原本的命令列（不執行），游標停在第一個待填參數處。常用命令依 frecency 自動上浮。

為的是不必再為了 `usbipd`、`git`、`netsh` 這些工具的子命令與旗標去翻 `--help`。

## 現況

| 里程碑 | 狀態 | 內容 |
|---|---|---|
| M0 | ✅ | caret 定位驗證（Windows Terminal 可精準定位） |
| M1 | ✅ | Tauri 骨架、雙視窗、全域快捷鍵 Alt+Q |
| M2 | ✅ | 焦點還原與 `SendInput` 文字注入 |
| M3 | ✅ | 候選框貼齊輸入游標 |
| M4 | ✅ | 命令目錄、模糊搜尋、frecency 排序 |
| M5 | ✅ | PSReadLine 歷史學習與機密過濾 |
| M6 | ✅ | 字詞設定畫面 |
| M7 | ✅ | 系統匣、開機自啟、封裝 |

內建目錄涵蓋 **usbipd、git、wsl、netsh、docker、winget、npm、cargo** 共 104 筆命令，
全附繁體中文說明。搜尋支援中文關鍵字——輸入「掛載」也能找到 `usbipd attach --wsl`。

啟動時會從 PSReadLine 歷史學習你實際用過的命令，出現次數越多初始排序越前面。
匯入採增量方式，只讀上次之後新增的部分。

## 技術架構

- **後端**：Rust + Tauri v2。全域快捷鍵走 `tauri-plugin-global-shortcut`；
  caret 定位與文字注入直接呼叫 Win32（`windows` crate）。
- **前端**：React + TypeScript + Vite，兩個入口對應兩個 Tauri 視窗
  （`launcher` 無邊框透明置頂候選框、`settings` 字詞設定畫面）。
- **資料**：SQLite，存放於 `%APPDATA%\com.jeremywen.qqkey\qqkey.db`，不對外傳送。
  候選池啟動時全載入記憶體，每次敲鍵的搜尋都不必再碰資料庫。

```
src/
├─ launcher/   候選框 UI
├─ settings/   字詞設定畫面
└─ shared/     共用型別
src-tauri/
├─ resources/catalog/*.json   內建命令目錄（隨執行檔內嵌）
└─ src/
   ├─ hotkey.rs    全域快捷鍵與候選框顯示／隱藏
   ├─ caret.rs     游標定位（三層 fallback）與螢幕邊界夾制
   ├─ inject.rs    前景追蹤、焦點還原 + SendInput 注入
   ├─ template.rs  `{佔位符}` 截斷
   ├─ catalog/     候選命令型別、內建目錄與歷史學習
   ├─ store.rs     SQLite 儲存與 frecency 持久化
   ├─ ranking.rs   模糊比對 × frecency 排序
   ├─ state.rs     資料庫與記憶體候選池
   └─ commands.rs  前端可呼叫的 IPC
spike/
├─ caret-probe/  caret 定位驗證（M0）
└─ input-probe/  SendInput 與快捷鍵驗證
```

## 安裝

從 `src-tauri/target/release/bundle/` 取安裝檔：

- `msi/QQKey_0.1.0_x64_en-US.msi`
- `nsis/QQKey_0.1.0_x64-setup.exe`

安裝後 QQKey 常駐在系統匣，平常沒有可見視窗。左鍵單擊系統匣圖示可叫出候選框，
右鍵開選單（叫出候選框／設定／結束）。開機自動啟動可在設定畫面開啟。

## 開發

```powershell
npm install
npm run tauri dev      # 開發模式，支援熱重載
npm run tauri build    # 產出 MSI / NSIS 安裝檔
```

```powershell
cd src-tauri
cargo test             # 後端單元測試
```

> 重新啟動 QQKey 時，前一個程序要完全結束後全域快捷鍵才會釋放。
> 太快啟動新的會註冊失敗，中間留幾秒。

## 快捷鍵

| 按鍵 | 動作 |
|---|---|
| `Alt+Q` | 叫出／收起候選框（可在設定畫面改綁） |
| `Alt+Shift+Q` | 開啟設定畫面 |
| `↑` `↓` | 移動選取 |
| `Alt+1`–`Alt+9` | 直接選取對應項目 |
| `Enter` | 填入命令列 |
| `Esc` | 取消 |

直選掛在 `Alt` 上而不是裸數字鍵，因為命令名稱本身就常帶數字
（`7z`、`base64`、`md5sum`、`python3`），數字鍵要留給查詢字串。

選擇 Alt+Q 而非 Alt+Space，是因為後者是 Windows 系統視窗選單的保留鍵，
在 Windows Terminal 中需要另外改設定才能傳遞給應用程式。

設定入口做成全域快捷鍵而非候選框裡的按鍵，是因為中文輸入法會攔截
`Ctrl+,` 這類組合，把修飾鍵吃掉只留下一個全形逗號。

## 設定畫面

`Alt+Shift+Q` 開啟，分兩頁：

- **命令字詞**：搜尋、依來源篩選、新增／編輯／刪除、啟用停用（可批次）、
  清除單筆使用統計。編輯過的內建條目會轉為自訂，之後更新版本不會被蓋回去。
- **一般設定**：開機自啟、快捷鍵改綁、歷史學習開關與手動匯入、機密過濾規則、
  透過剪貼簿以 JSON 分享自訂命令（只含自己新增或編輯過的，
  內建目錄對方也有，歷史學來的可能夾帶工作內容，都不會被匯出）、
  完整備份與還原、開啟日誌資料夾。

**分享**與**備份**是兩件事：分享只含自訂命令，備份則帶走全部條目、使用統計與
所有設定——歷史學來的那上千筆只有備份帶得走。還原會取代目前的全部資料。

## 圖示

`src-tauri/icons/source.png` 是 1024×1024 的來源檔，其餘尺寸與 `.ico`／`.icns`
由它產生。要換圖示時替換來源檔後執行：

```powershell
npm run tauri icon src-tauri/icons/source.png
```

圖案取自產品本身的隱喻——發光的輸入游標，底下貼著候選框。

## 已知限制

- 候選框在中文輸入法模式下打字會進入注音組字狀態。這是 Windows 應用的
  共同行為，目前需自行切換中英。搜尋支援中文關鍵字，所以不宜直接停用輸入法。

## 注意事項

- QQKey 使用 `SendInput` 與 UI Automation，行為近似自動化工具。
  部署前請自行審查程式碼，並確認符合公司資安政策；如遭 EDR 攔截，請洽 IT 部門。
- QQKey 會讀取 PowerShell 歷史紀錄以學習常用命令。歷史中可能夾帶憑證，
  匯入前一律以規則過濾——命中 `password`、`token`、`secret`、`credential`、
  `ConvertTo-SecureString` 等樣式，或 `=`／`:` 後接 20 字元以上長字串的行，
  整行略過，不會進資料庫也不會出現在候選框。略過的**筆數**會回報，內容不會被記錄。
  過濾採寧可誤殺的策略：誤殺的命令可以自己補回來，漏放的憑證會一直留在資料庫裡。
- 所有資料僅存於本機 `%APPDATA%\com.jeremywen.qqkey\`，不對外傳送。
  歷史匯入可在設定畫面關閉（M6）。
- 診斷日誌寫在 `%LOCALAPPDATA%\com.jeremywen.qqkey\logs\`（與資料庫不同層），
  可從設定畫面開啟。日誌不記錄視窗標題，也不記錄填入的內容。

## 授權

MIT License，見 [LICENSE](LICENSE)。
