import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Trans, useTranslation } from "react-i18next";
import { AUTO_LANGUAGE, LANGUAGES, LANGUAGE_LABELS } from "../i18n/languages";
import type { ImportPreview, ImportReport, Settings } from "../shared/types";
import ConfirmDialog from "./ConfirmDialog";

/** 不透明度可調的範圍。後端才是權威，這裡只是不讓 UI 送出後端會拒絕的值。 */
const MIN_OPACITY = 20;
const MAX_OPACITY = 100;

/**
 * 預覽底下墊的假終端機輸出。
 *
 * 只是襯底——候選框在設定視窗開著時是隱藏的，沒有東西墊在後面就看不出
 * 不透明度的差別。內容刻意用中性的建置訊息，不帶任何真實路徑。
 */
const PREVIEW_DESK = `PS C:\\dev\\qqkey> npm run tauri dev

  VITE v5.4.10  ready in 412 ms

  ➜  Local:   http://localhost:1420/
  ➜  press h + enter to show help

   Compiling qqkey v0.1.0
    Finished dev profile in 18.42s

PS C:\\dev\\qqkey> _`;

export default function GeneralPanel({
  onError,
  onNotice,
}: {
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [shortcut, setShortcut] = useState("");
  const [pattern, setPattern] = useState("");
  const [report, setReport] = useState<ImportReport | null>(null);
  const [autostart, setAutostart] = useState(false);
  /** 首次載入失敗的原因。有值時畫面要給得出重試，而不是卡在「載入中…」。 */
  const [loadError, setLoadError] = useState<string | null>(null);
  /** 已試算、等待確認的匯入。 */
  const [pending, setPending] = useState<{ json: string; preview: ImportPreview } | undefined>(
    undefined,
  );
  /** 已選定、等待確認的還原來源。還原會蓋掉全部資料，不能不問就做。 */
  const [restorePath, setRestorePath] = useState<string | undefined>(undefined);
  /** 不透明度草稿。拖動時只驅動預覽，放手才寫入。 */
  const [opacity, setOpacity] = useState(0);

  const reload = async () => {
    try {
      const result = await invoke<Settings>("get_settings");
      setSettings(result);
      setShortcut(result.shortcut);
      setPattern(result.secretPattern);
      setOpacity(result.launcherOpacity);
      setAutostart(await invoke<boolean>("autostart_enabled"));
      setLoadError(null);
    } catch (error) {
      setLoadError(String(error));
      onError(String(error));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  /*
   * 設定視窗關閉時只是 hide()，React state 全數保留，所以重新開啟看到的會是
   * 上次的舊資料——候選池筆數、使用統計都可能已經變了。重新取得焦點時再讀一次。
   */
  useEffect(() => {
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  if (!settings) {
    // autostart_enabled 走的是登錄檔查詢，失敗是有可能的。沒有這個分支的話
    // 畫面會永遠停在「載入中…」，而唯一的錯誤 toast 四秒後就消失了。
    return (
      <div className="panel">
        {loadError === null ? (
          t("settings.general.loading")
        ) : (
          <div className="crash">
            <p className="crash__title">{t("settings.general.loadFailed")}</p>
            <p className="crash__message">{loadError}</p>
            <button type="button" className="crash__button" onClick={() => void reload()}>
              {t("settings.general.retry")}
            </button>
          </div>
        )}
      </div>
    );
  }

  const run = async (action: () => Promise<unknown>, notice: string) => {
    try {
      await action();
      await reload();
      onNotice(notice);
    } catch (error) {
      onError(String(error));
    }
  };

  /**
   * 滑桿與預覽用的值。輸入框可能暫時是空的或超出範圍（打「25」會先經過「2」），
   * 這裡夾制過再用，空的就退回已存的值。
   */
  const shown = Number.isFinite(opacity)
    ? Math.min(MAX_OPACITY, Math.max(MIN_OPACITY, Math.round(opacity)))
    : settings.launcherOpacity;

  /**
   * 放手才寫入。刻意不走 `run()`——那會跳 toast，而拖一次滑桿彈一次提示太吵，
   * 預覽本身就是回饋。
   */
  const applyOpacity = async (percent: number) => {
    setOpacity(percent);
    if (percent === settings.launcherOpacity) {
      return;
    }
    try {
      await invoke("set_launcher_opacity", { percent });
      await reload();
    } catch (error) {
      onError(String(error));
    }
  };

  /**
   * 換介面語言。
   *
   * 刻意不走 `run()`——那會跳 toast，而 notice 字串是在語系改變**之前**求值的，
   * 會用舊語言顯示。而且整個介面當場換掉本身就是最清楚的回饋，
   * 理由同不透明度滑桿（那裡是「預覽本身就是回饋」）。
   */
  const changeLanguage = async (language: string) => {
    try {
      await invoke("set_language", { language });
      await reload();
    } catch (error) {
      onError(String(error));
    }
  };

  const exportEntries = async () => {
    try {
      const json = await invoke<string>("export_entries");
      await navigator.clipboard.writeText(json);
      const count = (JSON.parse(json) as { entries: unknown[] }).entries.length;
      onNotice(t("settings.share.exported", { count }));
    } catch (error) {
      onError(String(error));
    }
  };

  const openLogDir = async () => {
    try {
      // 資料夾開起來使用者自己就看到了，不必再彈提示
      await invoke("open_log_dir");
    } catch (error) {
      onError(String(error));
    }
  };

  /*
   * 先試算再問。從前是讀了剪貼簿就直接寫進去、事後才回報筆數——
   * 使用者沒有機會知道自己即將覆蓋掉本機多少東西，而覆寫沒有 undo。
   */
  const importEntries = async () => {
    try {
      const json = await navigator.clipboard.readText();
      const result = await invoke<ImportPreview>("preview_import", { json });
      if (result.total === 0) {
        onNotice(t("settings.share.clipboardEmpty"));
        return;
      }
      setPending({ json, preview: result });
    } catch (error) {
      onError(String(error));
    }
  };

  const backup = async () => {
    try {
      const path = await save({
        // 原生對話框的標題也是使用者看得到的字
        title: t("settings.backup.saveTitle"),
        defaultPath: "qqkey-backup.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) {
        return;
      }
      const count = await invoke<number>("backup_to_file", { path });
      onNotice(t("settings.backup.saved", { count }));
    } catch (error) {
      onError(String(error));
    }
  };

  const chooseRestore = async () => {
    try {
      const path = await open({
        title: t("settings.backup.openTitle"),
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof path !== "string") {
        return;
      }
      setRestorePath(path);
    } catch (error) {
      onError(String(error));
    }
  };

  return (
    <div className="panel panel--form">
      {/* 排在第一個：讀不懂介面的使用者最先要找的就是這一項。 */}
      <section className="section">
        <h2 className="section__title">{t("settings.language.title")}</h2>
        <p className="section__note">{t("settings.language.note")}</p>
        <div className="section__row">
          <select
            className="toolbar__select"
            aria-label={t("settings.language.title")}
            value={settings.language}
            onChange={(event) => void changeLanguage(event.target.value)}
          >
            <option value={AUTO_LANGUAGE}>
              {t("settings.language.auto", {
                language: LANGUAGE_LABELS[settings.systemLanguage] ?? settings.systemLanguage,
              })}
            </option>
            {/* 語言名稱用該語言自己的寫法，不隨介面語言翻譯 */}
            {LANGUAGES.map((tag) => (
              <option key={tag} value={tag}>
                {LANGUAGE_LABELS[tag]}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.general.shortcutTitle")}</h2>
        <p className="section__note">
          <Trans i18nKey="settings.general.shortcutNote" components={{ code: <code /> }} />
        </p>
        <div className="section__row">
          <input
            className="field__input field__input--mono"
            value={shortcut}
            onChange={(event) => setShortcut(event.target.value)}
            spellCheck={false}
          />
          <button
            className="button button--primary"
            disabled={shortcut === settings.shortcut}
            onClick={() =>
              void run(
                () => invoke("set_shortcut", { shortcut }),
                t("settings.general.shortcutUpdated"),
              )
            }
          >
            {t("settings.general.apply")}
          </button>
        </div>
        {settings.activeShortcut !== settings.shortcut && (
          <p className="section__note section__note--warn">
            {settings.activeShortcut
              ? t("settings.general.shortcutFellBack", {
                  wanted: settings.shortcut,
                  active: settings.activeShortcut,
                })
              : t("settings.general.shortcutNoneActive", { wanted: settings.shortcut })}
          </p>
        )}
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.opacity.title")}</h2>
        <p className="section__note">{t("settings.opacity.note")}</p>

        <div className="preview" aria-hidden="true">
          <pre className="preview__desk">{PREVIEW_DESK}</pre>
          <div
            className="preview__launcher"
            style={{ "--preview-alpha": shown / 100 } as CSSProperties}
          >
            <div className="preview__input-row">
              <span className="preview__glyph">⌕</span>
              <span className="preview__query">git</span>
            </div>
            <div className="preview__list">
              <div className="preview__item preview__item--selected">
                <span className="preview__index">1</span>
                <span className="preview__command">
                  git switch <span className="preview__hint">&lt;branch&gt;</span>
                </span>
              </div>
              <div className="preview__item">
                <span className="preview__index">2</span>
                <span className="preview__command">
                  git rebase -i <span className="preview__hint">&lt;base&gt;</span>
                </span>
              </div>
            </div>
          </div>
          <span className="preview__label">{t("settings.opacity.preview")}</span>
        </div>

        <div className="section__row">
          <input
            className="slider"
            type="range"
            aria-label={t("settings.opacity.sliderLabel")}
            min={MIN_OPACITY}
            max={MAX_OPACITY}
            step={1}
            value={shown}
            style={
              {
                "--slider-fill": `${
                  ((shown - MIN_OPACITY) / (MAX_OPACITY - MIN_OPACITY)) * 100
                }%`,
              } as CSSProperties
            }
            onChange={(event) => setOpacity(Number(event.target.value))}
            onPointerUp={() => void applyOpacity(shown)}
            onKeyUp={() => void applyOpacity(shown)}
            /* 快速拖曳有可能在元素外放手而漏掉 pointerup，失焦時再補一次。
               值沒變時 applyOpacity 會直接返回，多叫幾次不會多寫入 */
            onBlur={() => void applyOpacity(shown)}
          />
          <input
            className="field__input field__input--mono field__input--tiny"
            type="number"
            aria-label={t("settings.opacity.percentLabel")}
            min={MIN_OPACITY}
            max={MAX_OPACITY}
            value={Number.isFinite(opacity) ? opacity : ""}
            onChange={(event) =>
              setOpacity(event.target.value === "" ? NaN : Number(event.target.value))
            }
            onBlur={() => void applyOpacity(shown)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                void applyOpacity(shown);
              }
            }}
          />
          {/* 緊貼數字輸入框的單位標示，不是文句，所以留字面的 % */}
          <span className="field__unit">%</span>
          <button
            className="button"
            disabled={settings.launcherOpacity === settings.defaultLauncherOpacity}
            onClick={() => void applyOpacity(settings.defaultLauncherOpacity)}
          >
            {t("settings.general.restoreDefault")}
          </button>
        </div>
        {/* 這一句是文句，% 寫在譯文裡（法文的 % 前面有空格，那屬於翻譯）。
            刻意不用 Intl 的 style: "percent"——它期待 0–1，會把 20 印成 2000%。 */}
        <span className="field__hint">
          {t("settings.opacity.minimumHint", { percent: MIN_OPACITY })}
        </span>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.startup.title")}</h2>
        <p className="section__note">{t("settings.startup.note")}</p>
        <label className="switch">
          <input
            type="checkbox"
            checked={autostart}
            onChange={(event) =>
              void run(
                () => invoke("set_autostart", { enabled: event.target.checked }),
                event.target.checked
                  ? t("settings.startup.enabled")
                  : t("settings.startup.disabled"),
              )
            }
          />
          <span>{t("settings.startup.label")}</span>
        </label>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.history.title")}</h2>
        <p className="section__note">
          {t("settings.history.note", { count: settings.poolSize })}
        </p>
        <div className="section__row">
          <label className="switch">
            <input
              type="checkbox"
              checked={settings.historyImport}
              onChange={(event) =>
                void run(
                  () =>
                    invoke("set_history_import_enabled", {
                      enabled: event.target.checked,
                    }),
                  event.target.checked
                    ? t("settings.history.enabled")
                    : t("settings.history.disabled"),
                )
              }
            />
            <span>{t("settings.history.autoImport")}</span>
          </label>
          <button
            className="button"
            onClick={async () => {
              try {
                setReport(await invoke<ImportReport>("import_history"));
                await reload();
              } catch (error) {
                onError(String(error));
              }
            }}
          >
            {t("settings.history.importNow")}
          </button>
        </div>

        {report && (
          <div className="report">
            {/*
             * scanned === 0 時另外三個數字必然是 0、毫無資訊，本來就該是另一則
             * 訊息。從前是把「（沒有新增的歷史紀錄）」附在後面，於是印出
             * 「掃描 0 行，匯入 0 筆，略過 0 筆…（沒有新增的歷史紀錄）」，
             * 同一件事講了兩遍。
             */}
            {report.scanned === 0
              ? t("settings.history.reportEmpty")
              : t("settings.history.report", {
                  count: report.scanned,
                  imported: report.imported,
                  skippedSecret: report.skippedSecret,
                  skippedNoise: report.skippedNoise,
                })}
          </div>
        )}
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.secret.title")}</h2>
        <p className="section__note">
          <Trans i18nKey="settings.secret.note" components={{ code: <code /> }} />
        </p>
        <textarea
          className="field__textarea"
          value={pattern}
          onChange={(event) => setPattern(event.target.value)}
          spellCheck={false}
          rows={3}
        />
        <div className="section__row">
          <button
            className="button button--primary"
            disabled={pattern === settings.secretPattern}
            onClick={() =>
              void run(
                () => invoke("set_secret_pattern", { pattern }),
                t("settings.secret.updated"),
              )
            }
          >
            {t("settings.general.apply")}
          </button>
          <button
            className="button"
            disabled={pattern === settings.defaultSecretPattern}
            onClick={() => setPattern(settings.defaultSecretPattern)}
          >
            {t("settings.general.restoreDefault")}
          </button>
        </div>
        <p className="section__note section__note--warn">
          {t("settings.secret.warning", { tab: t("settings.tab.entries") })}
        </p>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.share.title")}</h2>
        <p className="section__note">{t("settings.share.note")}</p>
        <div className="section__row">
          <button className="button" onClick={exportEntries}>
            {t("settings.share.export")}
          </button>
          <button className="button" onClick={importEntries}>
            {t("settings.share.import")}
          </button>
        </div>
      </section>

      <section className="section">
        <h2 className="section__title">{t("settings.backup.title")}</h2>
        <p className="section__note">
          <Trans
            i18nKey="settings.backup.note"
            values={{ share: t("settings.share.title") }}
            components={{ strong: <strong /> }}
          />
        </p>
        <div className="section__row">
          <button className="button" onClick={backup}>
            {t("settings.backup.save")}
          </button>
          <button className="button" onClick={chooseRestore}>
            {t("settings.backup.restore")}
          </button>
        </div>
        <p className="section__note section__note--warn">
          <Trans i18nKey="settings.backup.warning" components={{ strong: <strong /> }} />
        </p>
      </section>

      {pending && (
        <ConfirmDialog
          title={t("settings.import.confirmTitle")}
          /*
           * 兩條完整的句子，而不是用 + 串接再讓 else 分支只補一個句號。
           * ⚠ 這是本應用最難翻的一條：一句話裡有三個數字，而 i18next 的
           * {{count}} 每個 key 只能驅動一個複數選擇器。added 與 overwritten
           * 的句式必須在任何數字下都讀得通——英文 "adds 1" 可以，
           * "1 commands" 不行。
           */
          message={
            pending.preview.overwritten > 0
              ? t("settings.import.confirmOverwrite", {
                  count: pending.preview.total,
                  added: pending.preview.added,
                  overwritten: pending.preview.overwritten,
                })
              : t("settings.import.confirm", {
                  count: pending.preview.total,
                  added: pending.preview.added,
                })
          }
          confirmLabel={t("settings.import.confirmLabel")}
          danger={pending.preview.overwritten > 0}
          onConfirm={() => {
            const { json } = pending;
            setPending(undefined);
            void run(
              () => invoke("import_entries", { json }),
              t("settings.import.done", { count: pending.preview.total }),
            );
          }}
          onCancel={() => setPending(undefined)}
        />
      )}

      {restorePath && (
        <ConfirmDialog
          title={t("settings.restore.confirmTitle")}
          message={t("settings.restore.confirmMessage", { path: restorePath })}
          confirmLabel={t("settings.restore.confirmLabel")}
          danger
          onConfirm={() => {
            const path = restorePath;
            setRestorePath(undefined);
            void run(
              () => invoke("restore_from_file", { path }),
              t("settings.restore.done"),
            );
          }}
          onCancel={() => setRestorePath(undefined)}
        />
      )}

      <section className="section">
        <h2 className="section__title">{t("settings.logs.title")}</h2>
        <p className="section__note">
          <Trans i18nKey="settings.logs.note" components={{ strong: <strong /> }} />
        </p>
        <div className="section__row">
          <button className="button" onClick={openLogDir}>
            {t("settings.logs.open")}
          </button>
        </div>
      </section>
    </div>
  );
}
