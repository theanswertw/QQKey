import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { Trans, useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
      onNotice(t("settings.about.emailCopied"));
    } catch (error) {
      onError(String(error));
    }
  };

  return (
    <div className="panel panel--form">
      <section className="section">
        <h2 className="section__title">QQKey</h2>
        <p className="section__note">
          <Trans i18nKey="settings.about.pitch" components={{ strong: <strong /> }} />
        </p>
        <dl className="about">
          <div className="about__row">
            <dt className="about__label">{t("settings.about.version")}</dt>
            <dd className="about__value">
              <span className="about__mono">{version || "—"}</span>
            </dd>
          </div>
          <div className="about__row">
            <dt className="about__label">{t("settings.about.license")}</dt>
            <dd className="about__value">MIT License</dd>
          </div>
          <div className="about__row">
            <dt className="about__label">{t("settings.about.data")}</dt>
            <dd className="about__value">{t("settings.about.dataValue")}</dd>
          </div>
        </dl>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.about.authorTitle")}</h2>
        <p className="section__note">
          <Trans
            i18nKey="settings.about.origin"
            values={{ author: AUTHOR.name }}
            components={{ code: <code /> }}
          />
        </p>
        <p className="section__note">{t("settings.about.contact")}</p>
        <dl className="about">
          <div className="about__row">
            <dt className="about__label">Email</dt>
            <dd className="about__value">
              <span className="about__mono">{AUTHOR.email}</span>
              <button
                className="button button--ghost"
                onClick={() => void openExternal(`mailto:${AUTHOR.email}`)}
              >
                {t("settings.about.sendMail")}
              </button>
              <button className="button button--ghost" onClick={copyEmail}>
                {t("settings.about.copy")}
              </button>
            </dd>
          </div>
          <div className="about__row">
            <dt className="about__label">{t("settings.about.repo")}</dt>
            <dd className="about__value">
              <span className="about__mono">{REPO_LABEL}</span>
              <button
                className="button button--ghost"
                onClick={() => void openExternal(AUTHOR.repo)}
              >
                {t("settings.about.open")}
              </button>
            </dd>
          </div>
        </dl>
        <p className="section__note section__note--warn">
          {t("settings.about.secretWarning")}
        </p>
      </section>
    </div>
  );
}
