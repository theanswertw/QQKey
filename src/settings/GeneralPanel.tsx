import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
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
          "載入中…"
        ) : (
          <div className="crash">
            <p className="crash__title">讀不到設定</p>
            <p className="crash__message">{loadError}</p>
            <button type="button" className="crash__button" onClick={() => void reload()}>
              重試
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

  const exportEntries = async () => {
    try {
      const json = await invoke<string>("export_entries");
      await navigator.clipboard.writeText(json);
      const count = (JSON.parse(json) as { entries: unknown[] }).entries.length;
      onNotice(`已複製 ${count} 筆自訂命令到剪貼簿`);
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
        onNotice("剪貼簿裡的檔案沒有任何命令");
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
        title: "備份 QQKey 資料",
        defaultPath: "qqkey-backup.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) {
        return;
      }
      const count = await invoke<number>("backup_to_file", { path });
      onNotice(`已備份 ${count} 筆命令與目前的設定`);
    } catch (error) {
      onError(String(error));
    }
  };

  const chooseRestore = async () => {
    try {
      const path = await open({
        title: "選擇要還原的備份",
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
      <section className="section">
        <h2 className="section__title">全域快捷鍵</h2>
        <p className="section__note">
          格式為修飾鍵加按鍵代碼，例如 <code>Alt+KeyQ</code>、
          <code>Control+Shift+Space</code>。字母鍵要寫成 <code>KeyQ</code> 這種形式。
          避開 <code>Alt+Space</code>——那是 Windows 系統視窗選單的保留鍵。
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
            onClick={() => void run(() => invoke("set_shortcut", { shortcut }), "快捷鍵已更新")}
          >
            套用
          </button>
        </div>
        {settings.activeShortcut !== settings.shortcut && (
          <p className="section__note section__note--warn">
            {settings.activeShortcut
              ? `${settings.shortcut} 沒有註冊成功，多半是被其他程式佔用了。
                 目前實際可用的是 ${settings.activeShortcut}，換一組再套用就會生效。`
              : `${settings.shortcut} 註冊失敗，現在沒有任何快捷鍵可以叫出候選框——
                 請從系統匣圖示操作，或在這裡換一組。`}
          </p>
        )}
      </section>

      <section className="section">
        <h2 className="section__title">候選框背景</h2>
        <p className="section__note">
          候選框浮在你正在用的視窗上。不透明度越低，透出的底下內容越多；
          背景模糊會維持命令文字的可讀性。設定視窗開著時候選框是隱藏的，
          下面的預覽就是它實際的樣子。
        </p>

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
          <span className="preview__label">預覽</span>
        </div>

        <div className="section__row">
          <input
            className="slider"
            type="range"
            aria-label="候選框背景不透明度"
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
            aria-label="候選框背景不透明度百分比"
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
          <span className="field__unit">%</span>
          <button
            className="button"
            disabled={settings.launcherOpacity === settings.defaultLauncherOpacity}
            onClick={() => void applyOpacity(settings.defaultLauncherOpacity)}
          >
            還原預設
          </button>
        </div>
        <span className="field__hint">
          下限 {MIN_OPACITY}%——再低，命令會被底下的內容干擾到讀不出來。
        </span>
      </section>

      <section className="section">
        <h2 className="section__title">啟動</h2>
        <p className="section__note">
          QQKey 平常沒有可見視窗，靠系統匣圖示常駐。左鍵單擊圖示可叫出候選框，
          右鍵開選單。
        </p>
        <label className="switch">
          <input
            type="checkbox"
            checked={autostart}
            onChange={(event) =>
              void run(
                () => invoke("set_autostart", { enabled: event.target.checked }),
                event.target.checked ? "已設為開機自動啟動" : "已取消開機自動啟動",
              )
            }
          />
          <span>開機時自動啟動</span>
        </label>
      </section>

      <section className="section">
        <h2 className="section__title">歷史紀錄學習</h2>
        <p className="section__note">
          從 PSReadLine 歷史學習你實際用過的命令，出現次數越多初始排序越前面。
          採增量匯入，只讀上次之後新增的部分。目前候選池共 {settings.poolSize} 筆。
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
                  event.target.checked ? "已啟用歷史學習" : "已停用歷史學習",
                )
              }
            />
            <span>啟動時自動匯入</span>
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
            立即匯入
          </button>
        </div>

        {report && (
          <div className="report">
            掃描 {report.scanned} 行，匯入 {report.imported} 筆，
            略過 {report.skippedSecret} 筆疑似含憑證、{report.skippedNoise} 筆雜訊。
            {report.scanned === 0 && "（沒有新增的歷史紀錄）"}
          </div>
        )}
      </section>

      <section className="section">
        <h2 className="section__title">機密過濾規則</h2>
        <p className="section__note">
          命中這個正規表示式的歷史行會整行略過，不會進資料庫也不會出現在候選框。
          只會回報略過的筆數，內容不會被記錄。另有一條固定規則會擋下
          <code>=</code> 或 <code>:</code> 後接 20 字元以上的長字串，不受此處影響。
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
              void run(() => invoke("set_secret_pattern", { pattern }), "過濾規則已更新")
            }
          >
            套用
          </button>
          <button
            className="button"
            disabled={pattern === settings.defaultSecretPattern}
            onClick={() => setPattern(settings.defaultSecretPattern)}
          >
            還原預設
          </button>
        </div>
        <p className="section__note section__note--warn">
          放寬規則前請先確認：漏放的憑證會一直留在本機資料庫裡。
          被誤殺的命令可以在「命令字詞」頁自己新增回來。
        </p>
      </section>

      <section className="section">
        <h2 className="section__title">分享自訂命令</h2>
        <p className="section__note">
          透過剪貼簿以 JSON 交換。只包含你自己新增或編輯過的命令——
          內建目錄對方也有，歷史學來的可能夾帶工作內容，都不會被匯出。
        </p>
        <div className="section__row">
          <button className="button" onClick={exportEntries}>
            匯出到剪貼簿
          </button>
          <button className="button" onClick={importEntries}>
            從剪貼簿匯入
          </button>
        </div>
      </section>

      <section className="section">
        <h2 className="section__title">備份與還原</h2>
        <p className="section__note">
          跟上面的「分享」是兩件事。備份帶走<strong>全部</strong>——內建、
          歷史學來的、以及累積的使用統計與這一頁的所有設定，換一台機器能回到原狀。
          歷史學來的那上千筆只有這條路帶得走。
        </p>
        <div className="section__row">
          <button className="button" onClick={backup}>
            備份到檔案
          </button>
          <button className="button" onClick={chooseRestore}>
            從備份還原
          </button>
        </div>
        <p className="section__note section__note--warn">
          還原會<strong>取代</strong>目前的全部資料，包含你之後新增或編輯過的命令。
        </p>
      </section>

      {pending && (
        <ConfirmDialog
          title="確認匯入"
          message={
            `這個檔案有 ${pending.preview.total} 筆命令：新增 ${pending.preview.added} 筆` +
            (pending.preview.overwritten > 0
              ? `，覆寫 ${pending.preview.overwritten} 筆本機已有的。\n\n被覆寫的說明與關鍵字會換成檔案裡的版本，無法復原。`
              : "。")
          }
          confirmLabel="匯入"
          danger={pending.preview.overwritten > 0}
          onConfirm={() => {
            const { json } = pending;
            setPending(undefined);
            void run(
              () => invoke("import_entries", { json }),
              `已匯入 ${pending.preview.total} 筆命令`,
            );
          }}
          onCancel={() => setPending(undefined)}
        />
      )}

      {restorePath && (
        <ConfirmDialog
          title="確認還原"
          message={`${restorePath}\n\n將以這份備份取代目前的全部資料——現有的命令、使用統計與設定都會被蓋掉，無法復原。`}
          confirmLabel="還原"
          danger
          onConfirm={() => {
            const path = restorePath;
            setRestorePath(undefined);
            void run(
              () => invoke("restore_from_file", { path }),
              "已從備份還原",
            );
          }}
          onCancel={() => setRestorePath(undefined)}
        />
      )}

      <section className="section">
        <h2 className="section__title">診斷紀錄</h2>
        <p className="section__note">
          QQKey 平常沒有可見視窗，出問題時只能靠日誌回推是哪一步失敗的。
          日誌記錄叫出候選框、定位與注入各走到哪裡，
          <strong>不記錄視窗標題，也不記錄填入的內容</strong>。
        </p>
        <div className="section__row">
          <button className="button" onClick={openLogDir}>
            開啟日誌資料夾
          </button>
        </div>
      </section>
    </div>
  );
}
