import { useEffect, useRef, useState } from "react";
import type { EntryPatch, EntryView } from "../shared/types";

interface Props {
  /** null 表示新增 */
  entry: EntryView | null;
  onSave: (patch: EntryPatch) => void;
  onCancel: () => void;
}

export default function EntryDialog({ entry, onSave, onCancel }: Props) {
  const [template, setTemplate] = useState("");
  const [description, setDescription] = useState("");
  const [keywords, setKeywords] = useState("");
  const [boost, setBoost] = useState(0);

  const formRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    setTemplate(entry?.template ?? "");
    setDescription(entry?.description ?? "");
    setKeywords(entry?.keywords ?? "");
    setBoost(entry?.boost ?? 0);
  }, [entry]);

  /** 有沒有改過東西。用來決定誤點遮罩時該不該把內容丟掉。 */
  const dirty =
    template !== (entry?.template ?? "") ||
    description !== (entry?.description ?? "") ||
    keywords !== (entry?.keywords ?? "") ||
    boost !== (entry?.boost ?? 0);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      /*
       * 焦點留在對話框裡。沒有這段的話 Tab 會走到底下那張表格的按鈕上——
       * 看不出焦點在哪，按下去卻真的會動到別的條目。
       */
      const focusable = formRef.current?.querySelectorAll<HTMLElement>(
        "input:not([disabled]), button:not([disabled])",
      );
      if (!focusable?.length) {
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = template.trim();
    if (!trimmed) {
      return;
    }
    onSave({
      template: trimmed,
      description: description.trim() || null,
      keywords: keywords.trim() || null,
      boost,
    });
  };

  return (
    <div
      className="overlay"
      /*
       * 改過東西就不讓點遮罩關掉——那一下多半是誤觸，而代價是整筆重打。
       * 「取消」鈕與 Esc 是明確的意思表示，那兩條路照常放行。
       */
      onClick={() => {
        if (!dirty) {
          onCancel();
        }
      }}
    >
      <form
        ref={formRef}
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="entry-dialog-title"
        onClick={(event) => event.stopPropagation()}
        onSubmit={submit}
      >
        <h2 className="dialog__title" id="entry-dialog-title">
          {entry ? "編輯命令" : "新增命令"}
        </h2>

        <label className="field">
          <span className="field__label">命令</span>
          <input
            className="field__input field__input--mono"
            value={template}
            onChange={(event) => setTemplate(event.target.value)}
            placeholder="usbipd attach --wsl --busid {busid}"
            spellCheck={false}
            autoFocus
          />
          <span className="field__hint">
            以 <code>{"{名稱}"}</code> 標記待填參數。填入命令列時會截斷在第一個
            佔位符之前，游標剛好停在你要接手輸入的位置。
          </span>
        </label>

        <label className="field">
          <span className="field__label">說明</span>
          <input
            className="field__input"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="將裝置掛載到 WSL"
          />
        </label>

        <label className="field">
          <span className="field__label">搜尋關鍵字</span>
          <input
            className="field__input"
            value={keywords}
            onChange={(event) => setKeywords(event.target.value)}
            placeholder="掛載 wsl 連接"
          />
          <span className="field__hint">
            以空白分隔。加了中文詞就能用中文搜到這筆命令。
          </span>
        </label>

        <label className="field">
          <span className="field__label">手動加權</span>
          <input
            className="field__input field__input--narrow"
            type="number"
            min="0"
            step="0.5"
            value={boost}
            onChange={(event) => {
              // 負值會讓排序權重的 ln() 變成 NaN，那筆就卡在原位且不受查詢
              // 影響；1e999 之類的則直接是 Infinity。兩者後端都會擋，
              // 但擋在這裡使用者才不會白打一輪。
              const value = Number(event.target.value);
              setBoost(Number.isFinite(value) ? Math.max(0, value) : 0);
            }}
          />
          <span className="field__hint">
            加到 frecency 分數上，讓這筆命令固定往前排。不收負數——
            想讓某筆不要出現請用「停用」。
          </span>
        </label>

        <div className="dialog__actions">
          <button type="button" className="button" onClick={onCancel}>
            取消
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={!template.trim()}
          >
            儲存
          </button>
        </div>
      </form>
    </div>
  );
}
