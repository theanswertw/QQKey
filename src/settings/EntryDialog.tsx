import { useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import type { EntryPatch, EntryView } from "../shared/types";

interface Props {
  /** null 表示新增 */
  entry: EntryView | null;
  onSave: (patch: EntryPatch) => void;
  onCancel: () => void;
}

export default function EntryDialog({ entry, onSave, onCancel }: Props) {
  const { t } = useTranslation();
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
          {entry ? t("settings.entry.editTitle") : t("settings.entry.createTitle")}
        </h2>

        <label className="field">
          <span className="field__label">{t("settings.entry.template")}</span>
          <input
            className="field__input field__input--mono"
            value={template}
            onChange={(event) => setTemplate(event.target.value)}
            placeholder="usbipd attach --wsl --busid {busid}"
            spellCheck={false}
            autoFocus
          />
          <span className="field__hint">
            {/*
             * 譯文裡的 <code>{名稱}</code> 是**佔位符語法的示例**，不是可翻譯的
             * 文句，也不是插值——單層大括號不會被 i18next 的 {{ }} 碰到。
             * 不要把它「修正」成 {{name}}，那會讓這整段說明失去意義。
             */}
            <Trans i18nKey="settings.entry.templateHint" components={{ code: <code /> }} />
          </span>
        </label>

        <label className="field">
          <span className="field__label">{t("settings.entry.description")}</span>
          <input
            className="field__input"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder={t("settings.entry.descriptionPlaceholder")}
          />
        </label>

        <label className="field">
          <span className="field__label">{t("settings.entry.keywords")}</span>
          <input
            className="field__input"
            value={keywords}
            onChange={(event) => setKeywords(event.target.value)}
            placeholder={t("settings.entry.keywordsPlaceholder")}
          />
          <span className="field__hint">
            {t("settings.entry.keywordsHint")}
            {/*
             * 內建條目的關鍵字欄位顯示的是「目前介面語言」那一份。存檔之後這筆
             * 會轉成自訂來源、六語言的搜尋聯集被清掉，從此只吃這裡填的字。
             * 那是刻意的（編輯過就不該再被內建目錄蓋回），但得先講出來。
             */}
            {entry?.source === "builtin" && ` ${t("settings.entry.keywordsBuiltinHint")}`}
          </span>
        </label>

        <label className="field">
          <span className="field__label">{t("settings.entry.boost")}</span>
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
          <span className="field__hint">{t("settings.entry.boostHint")}</span>
        </label>

        <div className="dialog__actions">
          <button type="button" className="button" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            className="button button--primary"
            disabled={!template.trim()}
          >
            {t("settings.entry.save")}
          </button>
        </div>
      </form>
    </div>
  );
}
