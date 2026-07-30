import { useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ImportReport, Settings } from "../shared/types";

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
    } catch (error) {
      onError(String(error));
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  if (!settings) {
    return <div className="panel">載入中…</div>;
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

  const importEntries = async () => {
    try {
      const json = await navigator.clipboard.readText();
      const written = await invoke<number>("import_entries", { json });
      onNotice(`已從剪貼簿匯入 ${written} 筆命令`);
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
    </div>
  );
}
