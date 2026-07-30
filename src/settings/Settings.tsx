import { useEffect, useState } from "react";
import AboutPanel from "./AboutPanel";
import EntriesPanel from "./EntriesPanel";
import GeneralPanel from "./GeneralPanel";

type Tab = "entries" | "general" | "about";

interface Toast {
  kind: "error" | "notice";
  message: string;
}

export default function Settings() {
  const [tab, setTab] = useState<Tab>("entries");
  const [toast, setToast] = useState<Toast | null>(null);

  useEffect(() => {
    if (!toast) {
      return;
    }
    const timer = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(timer);
  }, [toast]);

  return (
    <div className="settings">
      <header className="settings__header">
        <h1 className="settings__title">QQKey 設定</h1>
        <nav className="tabs" role="tablist">
          <button
            role="tab"
            aria-selected={tab === "entries"}
            className={tab === "entries" ? "tab tab--active" : "tab"}
            onClick={() => setTab("entries")}
          >
            命令字詞
          </button>
          <button
            role="tab"
            aria-selected={tab === "general"}
            className={tab === "general" ? "tab tab--active" : "tab"}
            onClick={() => setTab("general")}
          >
            一般設定
          </button>
          <button
            role="tab"
            aria-selected={tab === "about"}
            className={tab === "about" ? "tab tab--active" : "tab"}
            onClick={() => setTab("about")}
          >
            關於
          </button>
        </nav>
      </header>

      {tab === "entries" && (
        <EntriesPanel onError={(message) => setToast({ kind: "error", message })} />
      )}
      {tab === "general" && (
        <GeneralPanel
          onError={(message) => setToast({ kind: "error", message })}
          onNotice={(message) => setToast({ kind: "notice", message })}
        />
      )}
      {tab === "about" && (
        <AboutPanel
          onError={(message) => setToast({ kind: "error", message })}
          onNotice={(message) => setToast({ kind: "notice", message })}
        />
      )}

      {toast && (
        <div
          className={`toast toast--${toast.kind}`}
          role="status"
          aria-live="polite"
          onClick={() => setToast(null)}
        >
          {toast.message}
        </div>
      )}
    </div>
  );
}
