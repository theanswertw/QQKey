import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import AboutPanel from "./AboutPanel";
import EntriesPanel from "./EntriesPanel";
import GeneralPanel from "./GeneralPanel";

type Tab = "entries" | "general" | "about";

interface Toast {
  kind: "error" | "notice";
  message: string;
}

export default function Settings() {
  const { t } = useTranslation();
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
        <h1 className="settings__title">{t("settings.title")}</h1>
        <nav className="tabs" role="tablist">
          <button
            role="tab"
            aria-selected={tab === "entries"}
            className={tab === "entries" ? "tab tab--active" : "tab"}
            onClick={() => setTab("entries")}
          >
            {t("settings.tab.entries")}
          </button>
          <button
            role="tab"
            aria-selected={tab === "general"}
            className={tab === "general" ? "tab tab--active" : "tab"}
            onClick={() => setTab("general")}
          >
            {t("settings.tab.general")}
          </button>
          <button
            role="tab"
            aria-selected={tab === "about"}
            className={tab === "about" ? "tab tab--active" : "tab"}
            onClick={() => setTab("about")}
          >
            {t("settings.tab.about")}
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
