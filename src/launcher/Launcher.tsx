import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { splitTemplate, type Candidate } from "../shared/types";

export default function Launcher() {
  const [query, setQuery] = useState("");
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [selected, setSelected] = useState(0);
  /** 注入失敗的原因。候選框沒有別的地方能講話，訊息只能留在框裡。 */
  const [error, setError] = useState<string | null>(null);
  /** 每次叫出候選框都要重查一次，常用度可能在上次之後變了 */
  const [refreshToken, setRefreshToken] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const itemRefs = useRef<(HTMLLIElement | null)[]>([]);

  useEffect(() => {
    focusInput();
    const unlisten = listen("launcher:shown", () => {
      setQuery("");
      setError(null);
      setRefreshToken((token) => token + 1);
      focusInput();
    });
    return () => {
      unlisten.then((dispose) => dispose());
    };
  }, []);

  /**
   * 背景不透明度。CSS 裡寫的那個值只在取到設定之前撐著，所以掛載時一定要
   * 主動問一次——不然重新啟動後使用者設定的不透明度就不會生效。
   * 事件只負責讓設定畫面改完立即套用，不必等重新啟動。
   */
  useEffect(() => {
    const apply = (percent: number) => {
      document.documentElement.style.setProperty(
        "--qq-surface-alpha",
        String(percent / 100),
      );
    };
    invoke<number>("launcher_opacity")
      .then(apply)
      .catch((error) => {
        console.error("取得背景不透明度失敗", error);
      });
    const unlisten = listen<number>("launcher:opacity", (event) => {
      apply(event.payload);
    });
    return () => {
      unlisten.then((dispose) => dispose());
    };
  }, []);

  /**
   * 視窗拿到焦點不代表 webview 拿到焦點，webview 拿到也不代表輸入框拿到。
   * 三層都要點名，而且要等這一幀畫完——`show()` 之後立刻 focus 會落空。
   */
  function focusInput() {
    requestAnimationFrame(() => {
      window.focus();
      document.body.focus();
      inputRef.current?.focus();
    });
  }

  useEffect(() => {
    let cancelled = false;
    invoke<Candidate[]>("search_candidates", { query })
      .then((result) => {
        if (!cancelled) {
          setCandidates(result);
          setSelected(0);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setCandidates([]);
          // 不要讓查詢失敗長得跟「查無結果」一樣——那會讓人一直改關鍵字重試
          setError(`搜尋失敗：${reason}`);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [query, refreshToken]);

  /*
   * 讓選取項目留在可視範圍內。目前九筆剛好塞得下純屬巧合——把
   * MAX_CANDIDATES 調大、或系統字型放大一級，鍵盤選取就會移出視野
   * 而畫面完全沒有反應。
   */
  useEffect(() => {
    itemRefs.current[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const dismiss = () => {
    void invoke("hide_launcher");
  };

  const accept = (index: number) => {
    const candidate = candidates[index];
    if (!candidate) {
      return;
    }
    // 後端負責截斷佔位符、還原焦點、送出文字，並記下這次使用
    invoke("accept_candidate", { id: candidate.id }).catch((reason) => {
      // 注入失敗時後端會把候選框重新顯示，訊息就落在這裡——
      // 從前這裡只寫 console，而 release 版根本沒有 console 可看
      setError(String(reason));
      focusInput();
    });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    /*
     * 組字期間的 Enter 與 ↑↓ 屬於輸入法，不是在選命令。不讓開的話，
     * 用注音打「掛載」時確認選字就會把命令注入出去——而搜尋本來就是
     * 鼓勵打中文關鍵字的，這條路踩到的機會不低。
     *
     * 只看 isComposing 不夠：compositionend 與 keydown 的先後順序沒有保證，
     * 確認鍵有時會落在組字結束之後。keyCode 229 是輸入法吃掉按鍵時的標記，
     * 補的就是這個空隙。
     */
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
      return;
    }
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
    /*
     * 直選掛在 Alt 上，數字鍵留給查詢字串——命令名稱本身就常帶數字
     * （7z、base64、md5sum、python3），裸數字當選取鍵的話這些命令永遠打不出來。
     * 掛 Alt 也讓行為固定：原本 preventDefault 只在候選數夠多時才執行，
     * 同一顆鍵會因為當下有幾筆候選而時而選取、時而輸入。
     */
    if (event.altKey && /^[1-9]$/.test(event.key)) {
      event.preventDefault();
      accept(Number(event.key) - 1);
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
          role="combobox"
          aria-label="搜尋命令"
          aria-expanded={candidates.length > 0}
          aria-controls="launcher-list"
          aria-activedescendant={
            candidates[selected] ? `launcher-item-${candidates[selected].id}` : undefined
          }
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setError(null);
          }}
          onKeyDown={handleKeyDown}
          placeholder="輸入命令關鍵字，例如 usbipd 或「掛載」"
          spellCheck={false}
          autoComplete="off"
        />
      </div>

      {candidates.length > 0 ? (
        <ul className="launcher__list" id="launcher-list" role="listbox">
          {candidates.map((candidate, index) => {
            const { prefix, hint } = splitTemplate(candidate.template);
            return (
              <li
                key={candidate.id}
                id={`launcher-item-${candidate.id}`}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                role="option"
                aria-selected={index === selected}
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
                {/* 分數是衰減到當下的，跟排序用的是同一個值——顯示原始累計值
                    會讓三個月沒碰的命令標著 ★10 卻排在 ★3 後面 */}
                {candidate.score >= 0.5 && (
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

      {error && (
        <div className="launcher__error" role="alert">
          填不進去：{error}
        </div>
      )}

      <div className="launcher__footer">
        <span>↑↓ 移動</span>
        <span>Alt+1–9 直選</span>
        <span>Enter 填入</span>
        <span>Esc 取消</span>
        {/* 從前這裡只是靜態文字，滑鼠沒有辦法從候選框進到設定畫面——
            而 open_settings 這支 IPC 早就註冊好了卻沒有人呼叫 */}
        <button
          type="button"
          className="launcher__footer-end launcher__link"
          onClick={() => void invoke("open_settings")}
        >
          Alt+Shift+Q 設定
        </button>
      </div>
    </div>
  );
}
