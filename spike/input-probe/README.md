# input-probe（M1/M2 spike）

驗證 `SendInput` 在本機環境可用（未被 EDR 攔截），並自動測試 QQKey 的全域快捷鍵是否真的被攔截到。

## 執行

```powershell
cd spike\input-probe
cargo run -- --alt-q            # 送出 Alt+Q，回報候選框是否因此顯示或收起
cargo run -- --settings         # 送出 Alt+Shift+Q，回報設定視窗是否開啟
cargo run -- --find QQKey       # 列出標題含關鍵字的可見視窗
cargo run -- --focus PowerShell # 把符合的視窗設為前景
cargo run -- --type "usbipd "   # 3 秒後把文字送到當時的前景視窗
```

需先以 `npm run tauri dev` 或安裝版啟動 QQKey 才有對象可測。

## 實測結果

- `SendInput` 可正常送出，未被 EDR 攔截。
- Alt+Q 全域快捷鍵註冊成功，連按兩次可正確在顯示／隱藏之間切換。
- Alt+Shift+Q 可開啟設定視窗。

## 踩過的坑

- 判斷候選框是否顯示要用**完全比對**視窗標題 `QQKey`，不能用包含比對——
  開著專案資料夾的檔案總管與執行中的終端機，標題都會含有 "QQKey"。
- `--focus` 從 console 程式呼叫 `SetForegroundWindow` 會被前景鎖定擋下，
  即使搭配 `AttachThreadInput` 也一樣。它只在已有前景的程序中可靠。
- 經過 webview 的快捷鍵（如 `Ctrl+,`）在中文輸入法下不可靠：輸入法會吃掉
  修飾鍵，只把一個全形逗號送進輸入框。設定入口因此改用全域快捷鍵。
