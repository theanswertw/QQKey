import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { splitTemplate, type Candidate } from "../shared/types";

/** 一次最多顯示九筆，對應數字鍵 1–9。 */
const MAX_VISIBLE = 9;

/**
 * M1 暫用的固定資料，僅為了驗證鍵盤操作與版面。
 * M4 會改為向後端查詢，由 SQLite 目錄 + frecency 排序供給。
 */
const PLACEHOLDER_CANDIDATES: Candidate[] = [
  {
    id: 1,
    template: "usbipd list",
    title: "usbipd list",
    description: "列出所有 USB 裝置與其 BUSID",
    source: "builtin",
    score: 37,
  },
  {
    id: 2,
    template: "usbipd attach --wsl --busid {busid}",
    title: "usbipd attach --wsl --busid",
    description: "將裝置掛載到 WSL",
    source: "builtin",
    score: 22,
  },
  {
    id: 3,
    template: "usbipd bind --busid {busid}",
    title: "usbipd bind --busid",
    description: "綁定裝置以供分享",
    source: "builtin",
    score: 8,
  },
  {
    id: 4,
    template: "usbipd detach --busid {busid}",
    title: "usbipd detach --busid",
    description: "卸離裝置",
    source: "builtin",
    score: 0,
  },
  {
    id: 5,
    template: "git switch -c {branch}",
    title: "git switch -c",
    description: "建立並切換到新分支",
    source: "builtin",
    score: 15,
  },
];

export default function Launcher() {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const candidates = useMemo(() => {
    const keyword = query.trim().toLowerCase();
    const matched = keyword
      ? PLACEHOLDER_CANDIDATES.filter((c) =>
          c.template.toLowerCase().includes(keyword),
        )
      : PLACEHOLDER_CANDIDATES;
    return matched.slice(0, MAX_VISIBLE);
  }, [query]);

  // 每次叫出候選框都應該是乾淨的狀態，且輸入焦點要落在輸入框上。
  useEffect(() => {
    inputRef.current?.focus();
    const unlisten = listen("launcher:shown", () => {
      setQuery("");
      setSelected(0);
      inputRef.current?.focus();
    });
    return () => {
      unlisten.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    setSelected(0);
  }, [query]);

  const dismiss = () => {
    void invoke("hide_launcher");
  };

  const accept = (index: number) => {
    const candidate = candidates[index];
    if (!candidate) {
      return;
    }
    // 後端負責截斷佔位符、還原焦點並送出文字
    void invoke("accept_candidate", { template: candidate.template });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      dismiss();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelected((i) => Math.min(i + 1, candidates.length - 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      accept(selected);
      return;
    }
    // 參數一律留到終端機裡再補，所以查詢字串不需要數字，1–9 可直接當選取鍵。
    if (/^[1-9]$/.test(event.key)) {
      const index = Number(event.key) - 1;
      if (index < candidates.length) {
        event.preventDefault();
        accept(index);
      }
    }
  };

  return (
    <div className="launcher">
      <div className="launcher__input-row">
        <span className="launcher__glyph" aria-hidden="true">
          ⌕
        </span>
        <input
          ref={inputRef}
          className="launcher__input"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="輸入命令關鍵字，例如 usbipd"
          spellCheck={false}
          autoComplete="off"
        />
      </div>

      {candidates.length > 0 ? (
        <ul className="launcher__list">
          {candidates.map((candidate, index) => {
            const { prefix, hint } = splitTemplate(candidate.template);
            return (
              <li
                key={candidate.id}
                className={
                  index === selected
                    ? "launcher__item launcher__item--selected"
                    : "launcher__item"
                }
                onMouseEnter={() => setSelected(index)}
                onClick={() => accept(index)}
              >
                <span className="launcher__index">{index + 1}</span>
                <span className="launcher__command">
                  {prefix}
                  {hint && <span className="launcher__hint">{hint}</span>}
                </span>
                {candidate.description && (
                  <span className="launcher__description">
                    {candidate.description}
                  </span>
                )}
                {candidate.score > 0 && (
                  <span className="launcher__score">★{candidate.score}</span>
                )}
              </li>
            );
          })}
        </ul>
      ) : (
        <div className="launcher__empty">沒有相符的命令</div>
      )}

      <div className="launcher__footer">
        <span>↑↓ 移動</span>
        <span>1–9 直選</span>
        <span>Enter 填入</span>
        <span>Esc 取消</span>
      </div>
    </div>
  );
}
