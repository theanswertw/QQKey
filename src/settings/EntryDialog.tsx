import { useEffect, useState } from "react";
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

  useEffect(() => {
    setTemplate(entry?.template ?? "");
    setDescription(entry?.description ?? "");
    setKeywords(entry?.keywords ?? "");
    setBoost(entry?.boost ?? 0);
  }, [entry]);

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
    <div className="overlay" onClick={onCancel}>
      <form
        className="dialog"
        onClick={(event) => event.stopPropagation()}
        onSubmit={submit}
      >
        <h2 className="dialog__title">{entry ? "編輯命令" : "新增命令"}</h2>

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
            step="0.5"
            value={boost}
            onChange={(event) => setBoost(Number(event.target.value))}
          />
          <span className="field__hint">
            加到 frecency 分數上，讓這筆命令固定往前排。
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
