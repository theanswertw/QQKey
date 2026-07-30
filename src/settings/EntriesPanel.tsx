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

const PAGE_SIZE = 40;

type SourceFilter = CandidateSource | "all";

export default function EntriesPanel({ onError }: { onError: (message: string) => void }) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<SourceFilter>("all");
  const [page, setPage] = useState(0);
  const [data, setData] = useState<EntryPage>({ total: 0, entries: [] });
  const [checked, setChecked] = useState<Set<number>>(new Set());
  /** undefined 表示沒開對話框；null 表示新增 */
  const [editing, setEditing] = useState<EntryView | null | undefined>(undefined);

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
    setChecked(new Set());
  }, [query, source]);

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

  const save = (patch: EntryPatch) => {
    const target = editing;
    setEditing(undefined);
    void run(() =>
      target
        ? invoke("update_entry", { id: target.id, patch })
        : invoke("create_entry", { patch }),
    );
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
              <span className="row__score" title="frecency 分數">
                {entry.score >= 0.05 ? entry.score.toFixed(1) : "—"}
              </span>
              <div className="row__actions">
                <button
                  className="button button--ghost"
                  onClick={() =>
                    void run(() =>
                      invoke("update_entry", {
                        id: entry.id,
                        patch: {
                          template: entry.template,
                          description: entry.description,
                          keywords: entry.keywords,
                          enabled: !entry.enabled,
                        },
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
                  onClick={() => void run(() => invoke("delete_entry", { id: entry.id }))}
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
    </div>
  );
}
