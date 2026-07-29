import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { splitTemplate, type Candidate } from "../shared/types";

export default function Launcher() {
  const [query, setQuery] = useState("");
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [selected, setSelected] = useState(0);
  /** 每次叫出候選框都要重查一次，常用度可能在上次之後變了 */
  const [refreshToken, setRefreshToken] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    const unlisten = listen("launcher:shown", () => {
      setQuery("");
      setRefreshToken((token) => token + 1);
      inputRef.current?.focus();
    });
    return () => {
      unlisten.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    invoke<Candidate[]>("search_candidates", { query })
      .then((result) => {
        if (!cancelled) {
          setCandidates(result);
          setSelected(0);
        }
      })
      .catch((error) => {
        console.error("查詢候選命令失敗", error);
        if (!cancelled) {
          setCandidates([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [query, refreshToken]);

  const dismiss = () => {
    void invoke("hide_launcher");
  };

  const accept = (index: number) => {
    const candidate = candidates[index];
    if (!candidate) {
      return;
    }
    // 後端負責截斷佔位符、還原焦點、送出文字，並記下這次使用
    invoke("accept_candidate", { id: candidate.id }).catch((error) => {
      console.error("填入命令失敗", error);
    });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      dismiss();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelected((index) => Math.min(index + 1, candidates.length - 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelected((index) => Math.max(index - 1, 0));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      accept(selected);
      return;
    }
    // 參數一律留到終端機裡再補，所以查詢字串不需要數字，1–9 可直接當選取鍵
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
          placeholder="輸入命令關鍵字，例如 usbipd 或「掛載」"
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
                {candidate.score >= 1 && (
                  <span className="launcher__score">
                    ★{Math.round(candidate.score)}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      ) : (
        <div className="launcher__empty">
          {query.trim() ? "沒有相符的命令" : "輸入關鍵字開始搜尋"}
        </div>
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
