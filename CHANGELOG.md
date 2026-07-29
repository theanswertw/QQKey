# Changelog

本檔案記錄 QQKey 的重要變更，格式依循 [Keep a Changelog](https://keepachangelog.com/zh-TW/1.1.0/)，
版本號依循 [Semantic Versioning](https://semver.org/lang/zh-TW/)。

## [Unreleased]

### Added

- **M0**：`spike/caret-probe` caret 定位驗證工具，三層 fallback
  （`GetGUIThreadInfo` → UI Automation `TextPattern` → 視窗矩形）。
  實測確認 Windows Terminal 可經由 `TermControl` 元素精準取得游標座標。
- **M1**：Tauri v2 骨架，`launcher`（無邊框、透明、置頂）與 `settings` 雙視窗；
  全域快捷鍵 **Alt+Q** 叫出／收起候選框；失焦自動隱藏。
- **M1**：候選框 UI，支援 `↑↓` 移動、`1`–`9` 直選、`Enter` 填入、`Esc` 取消。
- **M1**：`spike/input-probe`，以 `SendInput` 自動驗證全域快捷鍵與文字注入可用性。
- **M2**：`inject.rs` 焦點還原（`SetForegroundWindow`，前景鎖定時以
  `AttachThreadInput` 繞過）與 `SendInput` Unicode 文字注入。
- **M2**：`template.rs` 佔位符截斷——送出的內容止於第一個 `{參數}` 之前，
  游標停在使用者該接手輸入的位置。附單元測試。
- **M3**：`caret.rs` 三層 fallback 取得游標座標，候選框貼齊輸入游標；
  含螢幕邊界夾制（下方放不下就翻到游標上方）與多螢幕支援，附 6 個單元測試。
- **M3**：以 `SetWinEventHook` 持續追蹤前景視窗，取代按下快捷鍵當下才查詢的作法。
- **M3**：收起候選框時把焦點還給原本的視窗，Esc 之後可直接接著打字。

- **M4**：SQLite 儲存（`store.rs`）與記憶體候選池（`state.rs`），
  候選池啟動時全載入，敲鍵搜尋不必碰資料庫。
- **M4**：內建命令目錄，涵蓋 usbipd、git、wsl、netsh、docker、winget、npm、cargo
  共 104 筆命令，全附繁體中文說明與中文搜尋關鍵字。
- **M4**：`ranking.rs` 以 `nucleo-matcher` 模糊比對，乘上 frecency 加權排序；
  frecency 採三十天半衰期的增量衰減，只需存分數與最後使用時間。
  分數相同時依 來源優先序 → 命令長度 決勝。
- **M4**：候選框改為向後端查詢真實資料；空查詢時只列出用過的命令，
  而不是塞一串沒用過的當噪音。

### Fixed

- **M3**：`settings` 視窗即使設為 `visible: false`，在啟動時仍會被 Windows
  當成前景視窗，害候選框把它誤認為注入目標。改為需要時才動態建立。
- **M3**：`tauri.conf.json` 的 `"center": true` 會在顯示時覆寫算好的座標，已移除；
  改為定位失敗時才主動置中。

### Notes

- `rusqlite` 固定在 0.37：0.40 會帶進 `libsqlite3-sys` 0.38，其 build script
  用了 unstable 的 `cfg_select`，在 Rust 1.92 stable 上編不過。
