# QQKey

在任何視窗按下 **Alt+Q** 叫出候選框，鍵入關鍵字即列出對應命令，以數字鍵或方向鍵選取後，
命令會**填入**原本的命令列（不執行），游標停在第一個待填參數處。常用命令依 frecency 自動上浮。

為的是不必再為了 `usbipd`、`git`、`netsh` 這些工具的子命令與旗標去翻 `--help`。

## 現況

| 里程碑 | 狀態 | 內容 |
|---|---|---|
| M0 | ✅ | caret 定位驗證（Windows Terminal 可精準定位） |
| M1 | ✅ | Tauri 骨架、雙視窗、全域快捷鍵 Alt+Q |
| M2 | ✅ | 焦點還原與 `SendInput` 文字注入 |
| M3 | ⬜ | 候選框貼齊輸入游標 |
| M4 | ⬜ | 命令目錄、模糊搜尋、frecency 排序 |
| M5 | ⬜ | PSReadLine 歷史學習與機密過濾 |
| M6 | ⬜ | 字詞設定畫面 |
| M7 | ⬜ | 系統匣、開機自啟、封裝 |

M4 之前，候選框顯示的是一組固定的示範資料，且固定出現在螢幕中央。

## 技術架構

- **後端**：Rust + Tauri v2。全域快捷鍵走 `tauri-plugin-global-shortcut`；
  caret 定位與文字注入直接呼叫 Win32（`windows` crate）。
- **前端**：React + TypeScript + Vite，兩個入口對應兩個 Tauri 視窗
  （`launcher` 無邊框透明置頂候選框、`settings` 字詞設定畫面）。
- **資料**：SQLite，存放於 `%APPDATA%\QQKey\`，不對外傳送。

```
src/
├─ launcher/   候選框 UI
├─ settings/   字詞設定畫面
└─ shared/     共用型別
src-tauri/src/
├─ hotkey.rs    全域快捷鍵與候選框顯示／隱藏
├─ inject.rs    焦點還原 + SendInput 注入
├─ template.rs  `{佔位符}` 截斷
└─ commands.rs  前端可呼叫的 IPC
spike/
├─ caret-probe/  caret 定位驗證（M0）
└─ input-probe/  SendInput 與快捷鍵驗證
```

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

啟動後不會有任何可見視窗（系統匣為 M7 項目），按 **Alt+Q** 叫出候選框。

## 快捷鍵

| 按鍵 | 動作 |
|---|---|
| `Alt+Q` | 叫出／收起候選框 |
| `↑` `↓` | 移動選取 |
| `1`–`9` | 直接選取對應項目 |
| `Enter` | 填入命令列 |
| `Esc` | 取消 |

選擇 Alt+Q 而非 Alt+Space，是因為後者是 Windows 系統視窗選單的保留鍵，
在 Windows Terminal 中需要另外改設定才能傳遞給應用程式。

## 注意事項

- QQKey 使用 `SendInput` 與 UI Automation，行為近似自動化工具。
  部署前請自行審查程式碼，並確認符合公司資安政策；如遭 EDR 攔截，請洽 IT 部門。
- M5 會讀取 PowerShell 歷史紀錄以學習常用命令。歷史中可能夾帶憑證，
  匯入前會以規則過濾，且設定畫面提供逐筆檢視與刪除。所有資料僅存於本機。
