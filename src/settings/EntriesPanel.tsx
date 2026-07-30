import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  SOURCE_LABELS,
  splitTemplate,
  type CandidateSource,
  type EntryPage,
  type EntryPatch,
  type EntryView,
} from "../shared/types";
import EntryDialog from "./EntryDialog";
import ConfirmDialog from "./ConfirmDialog";

const PAGE_SIZE = 40;

type SourceFilter = CandidateSource | "all";

/** 把最後使用時間寫成「3 天前」。後端一直有傳這個值，只是從來沒被畫出來過。 */
function relativeTime(seconds: number | null): string {
  if (!seconds) {
    return "未用過";
  }
  const days = Math.floor(Date.now() / 1000 / 86400 - seconds / 86400);
  if (days <= 0) {
    return "今天";
  }
  if (days === 1) {
    return "昨天";
  }
  if (days < 30) {
    return `${days} 天前`;
  }
  const months = Math.floor(days / 30);
  return months < 12 ? `${months} 個月前` : `${Math.floor(days / 365)} 年前`;
}

/** 待確認的刪除。單筆與批次共用一個對話框。 */
interface PendingDelete {
  ids: number[];
  /** 單筆時附上命令內容，讓使用者確認自己點的是哪一筆 */
  template?: string;
}

export default function EntriesPanel({ onError }: { onError: (message: string) => void }) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<SourceFilter>("all");
  const [page, setPage] = useState(0);
  const [data, setData] = useState<EntryPage>({ total: 0, entries: [] });
  const [checked, setChecked] = useState<Set<number>>(new Set());
  /** undefined 表示沒開對話框；null 表示新增 */
  const [editing, setEditing] = useState<EntryView | null | undefined>(undefined);
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | undefined>(undefined);

  const reload = useCallback(async () => {
    try {
      const result = await invoke<EntryPage>("list_entries", {
        query,
        source: source === "all" ? null : source,
        offset: page * PAGE_SIZE,
        limit: PAGE_SIZE,
      });
      setData(result);
    } catch (error) {
      onError(String(error));
    }
  }, [query, source, page, onError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // 換條件時回到第一頁，免得停在一個已經不存在的頁碼上
  useEffect(() => {
    setPage(0);
  }, [query, source]);

  /*
   * 翻頁與換條件都清空選取。從前這裡只看 [query, source]，於是在第 1 頁勾了
   * 十筆、翻到第 2 頁之後，批次列仍寫著「已選取 10 筆」——按下停用會作用在
   * 螢幕上看不到的條目。跨頁保留選取要能講清楚選了哪些才不危險，
   * 而這個畫面沒有那個位置。
   */
  useEffect(() => {
    setChecked(new Set());
  }, [query, source, page]);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await reload();
    } catch (error) {
      onError(String(error));
    }
  };

  const toggleChecked = (id: number) => {
    setChecked((current) => {
      const next = new Set(current);
      if (!next.delete(id)) {
        next.add(id);
      }
      return next;
    });
  };

  /*
   * 成功才關對話框。從前是先關再送出，於是撞上 UNIQUE 約束（同一個 template
   * 已經存在）或控制字元檢查時，使用者剛打完的整筆內容就沒了，而看到的
   * 只有一行原始的 SQL 錯誤。
   */
  const save = async (patch: EntryPatch) => {
    const target = editing;
    try {
      if (target) {
        await invoke("update_entry", { id: target.id, patch });
      } else {
        await invoke("create_entry", { patch });
      }
      setEditing(undefined);
      await reload();
    } catch (error) {
      onError(String(error));
    }
  };

  const lastPage = Math.max(0, Math.ceil(data.total / PAGE_SIZE) - 1);

  return (
    <div className="panel">
      <div className="toolbar">
        <input
          className="toolbar__search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="搜尋命令、說明或關鍵字"
          spellCheck={false}
        />
        <select
          className="toolbar__select"
          value={source}
          onChange={(event) => setSource(event.target.value as SourceFilter)}
        >
          <option value="all">全部來源</option>
          <option value="user">自訂</option>
          <option value="builtin">內建</option>
          <option value="history">歷史</option>
        </select>
        <button
          className="button"
          disabled={data.entries.length === 0}
          onClick={() => setChecked(new Set(data.entries.map((entry) => entry.id)))}
        >
          選取本頁
        </button>
        <button className="button button--primary" onClick={() => setEditing(null)}>
          新增命令
        </button>
      </div>

      {checked.size > 0 && (
        <div className="bulk">
          <span>已選取 {checked.size} 筆</span>
          <button
            className="button"
            onClick={() =>
              void run(() =>
                invoke("set_entries_enabled", { ids: [...checked], enabled: true }),
              ).then(() => setChecked(new Set()))
            }
          >
            啟用
          </button>
          <button
            className="button"
            onClick={() =>
              void run(() =>
                invoke("set_entries_enabled", { ids: [...checked], enabled: false }),
              ).then(() => setChecked(new Set()))
            }
          >
            停用
          </button>
          <button
            className="button button--danger"
            onClick={() => setPendingDelete({ ids: [...checked] })}
          >
            刪除
          </button>
          <button className="button button--ghost" onClick={() => setChecked(new Set())}>
            取消選取
          </button>
        </div>
      )}

      <div className="table">
        {data.entries.map((entry) => {
          const { prefix, hint } = splitTemplate(entry.template);
          return (
            <div
              key={entry.id}
              className={entry.enabled ? "row" : "row row--disabled"}
            >
              <input
                type="checkbox"
                className="row__check"
                checked={checked.has(entry.id)}
                onChange={() => toggleChecked(entry.id)}
              />
              <div className="row__main">
                <div className="row__command">
                  {prefix}
                  {hint && <span className="row__hint">{hint}</span>}
                </div>
                {entry.description && (
                  <div className="row__description">{entry.description}</div>
                )}
              </div>
              <span className={`tag tag--${entry.source}`}>
                {SOURCE_LABELS[entry.source]}
              </span>
              <span
                className="row__score"
                title="衰減後的使用分數——就是候選框拿來排序的那一個"
              >
                {entry.score >= 0.05 ? entry.score.toFixed(1) : "—"}
              </span>
              <span className="row__boost" title="手動加權">
                {entry.boost > 0 ? `+${entry.boost}` : ""}
              </span>
              <span className="row__used" title="最後使用時間">
                {relativeTime(entry.lastUsed)}
              </span>
              <div className="row__actions">
                <button
                  className="button button--ghost"
                  onClick={() =>
                    // 走跟批次停用同一條路。從前這裡送 update_entry，而那支
                    // 會把內建條目轉成自訂——按個「停用」就讓它從此收不到
                    // 內建目錄更新、還被算進匯出檔，兩顆看起來一樣的按鈕
                    // 後果卻不同。單純開關啟用不該動到來源。
                    void run(() =>
                      invoke("set_entries_enabled", {
                        ids: [entry.id],
                        enabled: !entry.enabled,
                      }),
                    )
                  }
                >
                  {entry.enabled ? "停用" : "啟用"}
                </button>
                <button className="button button--ghost" onClick={() => setEditing(entry)}>
                  編輯
                </button>
                <button
                  className="button button--ghost"
                  title="清除使用統計"
                  onClick={() => void run(() => invoke("reset_entry_score", { id: entry.id }))}
                >
                  歸零
                </button>
                <button
                  className="button button--ghost button--danger"
                  onClick={() =>
                    setPendingDelete({ ids: [entry.id], template: entry.template })
                  }
                >
                  刪除
                </button>
              </div>
            </div>
          );
        })}

        {data.entries.length === 0 && (
          <div className="table__empty">沒有符合條件的命令</div>
        )}
      </div>

      <div className="pager">
        <span>
          共 {data.total} 筆
          {data.total > PAGE_SIZE && `，第 ${page + 1} / ${lastPage + 1} 頁`}
        </span>
        <div className="pager__buttons">
          <button
            className="button"
            disabled={page === 0}
            onClick={() => setPage((current) => current - 1)}
          >
            上一頁
          </button>
          <button
            className="button"
            disabled={page >= lastPage}
            onClick={() => setPage((current) => current + 1)}
          >
            下一頁
          </button>
        </div>
      </div>

      {editing !== undefined && (
        <EntryDialog
          entry={editing}
          onSave={save}
          onCancel={() => setEditing(undefined)}
        />
      )}

      {pendingDelete && (
        <ConfirmDialog
          title={pendingDelete.ids.length > 1 ? "刪除選取的命令" : "刪除這筆命令"}
          message={
            pendingDelete.template
              ? `${pendingDelete.template}\n\n刪除後無法復原。若只是想讓它不出現在候選框，用「停用」就好。`
              : `將刪除 ${pendingDelete.ids.length} 筆命令，無法復原。\n\n若只是想讓它們不出現在候選框，用「停用」就好。`
          }
          confirmLabel="刪除"
          danger
          onConfirm={() => {
            const { ids } = pendingDelete;
            setPendingDelete(undefined);
            void run(() => invoke("delete_entries", { ids })).then(() =>
              setChecked(new Set()),
            );
          }}
          onCancel={() => setPendingDelete(undefined)}
        />
      )}
    </div>
  );
}
