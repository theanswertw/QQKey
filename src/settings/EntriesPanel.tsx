import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
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

/** 待確認的刪除。單筆與批次共用一個對話框。 */
interface PendingDelete {
  ids: number[];
  /** 單筆時附上命令內容，讓使用者確認自己點的是哪一筆 */
  template?: string;
}

export default function EntriesPanel({ onError }: { onError: (message: string) => void }) {
  const { t, i18n } = useTranslation();
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

  /*
   * 兩個 Intl formatter 提到 useMemo：一頁 40 列，逐列 new Intl.* 是可測量的
   * 成本，而原本的 toFixed 與手寫的相對時間完全不建構物件——這是新增的開銷，
   * 得刻意處理掉。
   */
  const relative = useMemo(
    // numeric: "auto" 才會給出「今天」／「昨天」而不是「0 天前」／「1 天前」，
    // 正好等於原本手寫的那兩個特例，現在六種語言免費得到。
    () => new Intl.RelativeTimeFormat(i18n.language, { numeric: "auto" }),
    [i18n.language],
  );
  const scoreFormat = useMemo(
    // 德文與法文的小數點是逗號，toFixed 永遠給句點
    () =>
      new Intl.NumberFormat(i18n.language, {
        minimumFractionDigits: 1,
        maximumFractionDigits: 1,
      }),
    [i18n.language],
  );

  /** 把最後使用時間寫成「3 天前」。後端一直有傳這個值，只是從來沒被畫出來過。 */
  const relativeTime = (seconds: number | null): string => {
    if (!seconds) {
      return t("settings.entries.neverUsed");
    }
    const days = Math.floor(Date.now() / 1000 / 86400 - seconds / 86400);
    if (days <= 0) {
      return relative.format(0, "day");
    }
    if (days < 30) {
      return relative.format(-days, "day");
    }
    const months = Math.floor(days / 30);
    return months < 12
      ? relative.format(-months, "month")
      : relative.format(-Math.floor(days / 365), "year");
  };

  return (
    <div className="panel">
      <div className="toolbar">
        <input
          className="toolbar__search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("settings.entries.searchPlaceholder")}
          spellCheck={false}
        />
        <select
          className="toolbar__select"
          aria-label={t("settings.entries.sourceFilter")}
          value={source}
          onChange={(event) => setSource(event.target.value as SourceFilter)}
        >
          {/* 三個來源標籤跟表格裡的標籤共用同一組 key，兩處不可能再分岔 */}
          <option value="all">{t("settings.entries.allSources")}</option>
          <option value="user">{t("common.source.user")}</option>
          <option value="builtin">{t("common.source.builtin")}</option>
          <option value="history">{t("common.source.history")}</option>
        </select>
        <button
          className="button"
          disabled={data.entries.length === 0}
          onClick={() => setChecked(new Set(data.entries.map((entry) => entry.id)))}
        >
          {t("settings.entries.selectPage")}
        </button>
        <button className="button button--primary" onClick={() => setEditing(null)}>
          {t("settings.entries.create")}
        </button>
      </div>

      {checked.size > 0 && (
        <div className="bulk">
          <span>{t("settings.entries.selectedCount", { count: checked.size })}</span>
          <button
            className="button"
            onClick={() =>
              void run(() =>
                invoke("set_entries_enabled", { ids: [...checked], enabled: true }),
              ).then(() => setChecked(new Set()))
            }
          >
            {t("settings.entries.enable")}
          </button>
          <button
            className="button"
            onClick={() =>
              void run(() =>
                invoke("set_entries_enabled", { ids: [...checked], enabled: false }),
              ).then(() => setChecked(new Set()))
            }
          >
            {t("settings.entries.disable")}
          </button>
          <button
            className="button button--danger"
            onClick={() => setPendingDelete({ ids: [...checked] })}
          >
            {t("settings.entries.delete")}
          </button>
          <button className="button button--ghost" onClick={() => setChecked(new Set())}>
            {t("settings.entries.clearSelection")}
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
                {t(`common.source.${entry.source}`)}
              </span>
              <span className="row__score" title={t("settings.entries.scoreHint")}>
                {entry.score >= 0.05 ? scoreFormat.format(entry.score) : "—"}
              </span>
              <span className="row__boost" title={t("settings.entries.boostHint")}>
                {entry.boost > 0 ? `+${entry.boost}` : ""}
              </span>
              <span className="row__used" title={t("settings.entries.lastUsedHint")}>
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
                  {entry.enabled
                    ? t("settings.entries.disable")
                    : t("settings.entries.enable")}
                </button>
                <button className="button button--ghost" onClick={() => setEditing(entry)}>
                  {t("settings.entries.edit")}
                </button>
                <button
                  className="button button--ghost"
                  title={t("settings.entries.resetScoreHint")}
                  onClick={() => void run(() => invoke("reset_entry_score", { id: entry.id }))}
                >
                  {t("settings.entries.resetScore")}
                </button>
                <button
                  className="button button--ghost button--danger"
                  onClick={() =>
                    setPendingDelete({ ids: [entry.id], template: entry.template })
                  }
                >
                  {t("settings.entries.delete")}
                </button>
              </div>
            </div>
          );
        })}

        {data.entries.length === 0 && (
          <div className="table__empty">{t("settings.entries.empty")}</div>
        )}
      </div>

      <div className="pager">
        {/*
         * 兩條完整的句子，而不是「共 N 筆」再接一個以逗號開頭的片段。
         * 續段那種寫法翻不了——它假設了語序，也不讓譯者改標點。
         */}
        <span>
          {data.total > PAGE_SIZE
            ? t("settings.entries.countPaged", {
                count: data.total,
                page: page + 1,
                pages: lastPage + 1,
              })
            : t("settings.entries.count", { count: data.total })}
        </span>
        <div className="pager__buttons">
          <button
            className="button"
            disabled={page === 0}
            onClick={() => setPage((current) => current - 1)}
          >
            {t("settings.entries.previousPage")}
          </button>
          <button
            className="button"
            disabled={page >= lastPage}
            onClick={() => setPage((current) => current + 1)}
          >
            {t("settings.entries.nextPage")}
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
          title={
            pendingDelete.ids.length > 1
              ? t("settings.entries.deleteManyTitle")
              : t("settings.entries.deleteOneTitle")
          }
          message={
            pendingDelete.template
              ? t("settings.entries.deleteOneMessage", { template: pendingDelete.template })
              : t("settings.entries.deleteManyMessage", { count: pendingDelete.ids.length })
          }
          confirmLabel={t("settings.entries.delete")}
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
