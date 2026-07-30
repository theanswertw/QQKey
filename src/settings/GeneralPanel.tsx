import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ImportReport, Settings } from "../shared/types";

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

  const reload = async () => {
    try {
      const result = await invoke<Settings>("get_settings");
      setSettings(result);
      setShortcut(result.shortcut);
      setPattern(result.secretPattern);
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
