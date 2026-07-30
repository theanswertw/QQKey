import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

/**
 * 作者與聯絡方式。
 *
 * 這是整個 UI 唯一寫著這些字串的地方——它們不會因為使用者的操作而改變，
 * 沒有進資料庫的理由，後端也不需要認識它們。
 */
const AUTHOR = {
  name: "Jeremy Wen",
  email: "jeremy@jeremywen.com",
  repo: "https://github.com/theanswertw/QQKey",
};

/** 顯示用的專案頁位址。去掉 `https://` 只是為了那一行不要太長。 */
const REPO_LABEL = AUTHOR.repo.replace(/^https:\/\//, "");

export default function AboutPanel({
  onError,
  onNotice,
}: {
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const [version, setVersion] = useState("");

  useEffect(() => {
    // 取不到版本不必擋住整頁——這頁其餘內容都是寫死的，照樣讀得到。
    // 版本那一格會留空，比整頁顯示錯誤有用。
    void getVersion().then(setVersion, () => setVersion(""));
  }, []);

  const openExternal = async (target: string) => {
    try {
      await invoke("open_external", { target });
    } catch (error) {
      onError(String(error));
    }
  };

  /*
   * Email 除了「寄信」還給一個「複製」。mailto: 要有註冊過的郵件軟體才開得起來，
   * 沒裝的機器上按下去不是沒反應就是跳出系統的關聯程式對話框——那時候至少
   * 要有辦法把位址帶走。
   */
  const copyEmail = async () => {
    try {
      await navigator.clipboard.writeText(AUTHOR.email);
      onNotice("已複製 Email 到剪貼簿");
    } catch (error) {
      onError(String(error));
    }
  };

  return (
    <div className="panel panel--form">
      <section className="section">
        <h2 className="section__title">QQKey</h2>
        <p className="section__note">
          在任何視窗按下快捷鍵叫出候選框，鍵入關鍵字找到命令後
          <strong>填入</strong>命令列而不執行，游標停在第一個待填參數處。
          按不按 Enter 由你決定。常用的命令依 frecency 自動上浮。
        </p>
        <dl className="about">
          <div className="about__row">
            <dt className="about__label">版本</dt>
            <dd className="about__value">
              <span className="about__mono">{version || "—"}</span>
            </dd>
          </div>
          <div className="about__row">
            <dt className="about__label">授權</dt>
            <dd className="about__value">MIT License</dd>
          </div>
          <div className="about__row">
            <dt className="about__label">資料</dt>
            <dd className="about__value">全部存於本機，不對外傳送</dd>
          </div>
        </dl>
      </section>

      <section className="section">
        <h2 className="section__title">作者</h2>
        <p className="section__note">
          QQKey 由 {AUTHOR.name} 一人開發與維護。起因很單純：usbipd、git、netsh
          這些工具的子命令與旗標記不住，每次都得回頭翻 <code>--help</code>，
          所以做了一個能用中文關鍵字找命令的工具，找到之後填進命令列讓自己
          再看一眼，而不是替你按下去。
        </p>
        <p className="section__note">
          使用上的問題、想補進內建目錄的命令，或是回報 bug，都歡迎從下面任一條路找我。
        </p>
        <dl className="about">
          <div className="about__row">
            <dt className="about__label">Email</dt>
            <dd className="about__value">
              <span className="about__mono">{AUTHOR.email}</span>
              <button
                className="button button--ghost"
                onClick={() => void openExternal(`mailto:${AUTHOR.email}`)}
              >
                寄信
              </button>
              <button className="button button--ghost" onClick={copyEmail}>
                複製
              </button>
            </dd>
          </div>
          <div className="about__row">
            <dt className="about__label">專案頁</dt>
            <dd className="about__value">
              <span className="about__mono">{REPO_LABEL}</span>
              <button
                className="button button--ghost"
                onClick={() => void openExternal(AUTHOR.repo)}
              >
                開啟
              </button>
            </dd>
          </div>
        </dl>
        <p className="section__note section__note--warn">
          回報問題時請避免貼上含有憑證或公司內部路徑的命令內容。
        </p>
      </section>
    </div>
  );
}
