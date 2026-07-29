# caret-probe（M0 spike）

驗證 QQKey 能否在各種宿主視窗中取得輸入游標（caret）的螢幕座標，決定候選框要貼齊游標、還是降級為視窗左下角定位。

## 三層 fallback

| 層 | 手段 | 預期適用 |
|---|---|---|
| 1 | `GetGUIThreadInfo` → `rcCaret` → `ClientToScreen` | conhost、Win32 原生輸入控制項 |
| 2 | UI Automation `TextPattern2::GetCaretRange`，退回 `TextPattern::GetSelection` + `ExpandToEnclosingUnit` | Windows Terminal、VS Code 等自繪文字的應用 |
| 3 | `GetWindowRect` 左下角內縮 / 滑鼠座標 | 前兩層皆失敗時的保底 |

## 執行

```powershell
cd spike\caret-probe
cargo run -- 5      # 參數為倒數秒數，預設 5
```

倒數期間切換到要測試的視窗，時間到會印出各層結果。
要測 Windows Terminal 本身，就在 Windows Terminal 中執行並保持不切換視窗。

## 實測結果

| 宿主 | 命中層 | 備註 |
|---|---|---|
| **Windows Terminal (PowerShell)** | **Layer 2 — `TextPattern::GetSelection`** | 主要目標，**可精準定位**。caret rect 寬 9 高 19，即一個等寬字元 |
| LINE (Qt) | Layer 1 — `GetGUIThreadInfo` | 輸入框有系統 caret |
| 檔案總管（已最小化） | 無 → Layer 3 | 視窗座標為 -32000，需在實作中以 `IsIconic` 排除 |

## 給 M3 實作的結論

1. **Windows Terminal 不需降級**。`CASCADIA_HOSTING_WINDOW_CLASS` 底下的 `TermControl`
   （`class="TermControl"`、controlType 50020 = Text）實作了 TextPattern。
2. WT 上 `TextPattern2::GetCaretRange` **未命中**，實際生效的是
   `TextPattern::GetSelection` 取得零寬度範圍後 `ExpandToEnclosingUnit(Character)` 再量邊界。
   兩條路徑都要保留。
3. 遍歷後代找 TextPattern 時，**第一個命中的是分頁標題的 `TextBlock`**（會回報標題列座標）。
   必須逐一嘗試取 caret 而非取第一個，或直接以 `class == "TermControl"` 過濾。
4. 實際執行時應優先用 `GetFocusedElement()`；取不到才退回 `ElementFromHandle` + 遍歷後代。
5. 最小化視窗會回報 -32000 座標，定位前需過濾。
