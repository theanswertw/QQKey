/**
 * 字詞設定畫面。M6 才會填入條目 CRUD、匯入匯出與一般設定，
 * 目前僅提供骨架以確認雙視窗載入正常。
 */
export default function Settings() {
  return (
    <div className="settings">
      <h1 className="settings__title">QQKey 設定</h1>
      <p className="settings__note">
        字詞管理、快捷鍵與歷史匯入設定將於 M6 實作。
      </p>
    </div>
  );
}
