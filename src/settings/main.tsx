import i18next from "i18next";
import React from "react";
import ReactDOM from "react-dom/client";
import Settings from "./Settings";
import ErrorBoundary from "../shared/ErrorBoundary";
import { initI18n, watchLanguage } from "../i18n";
import "./settings.css";

async function bootstrap() {
  // 兩個入口各有一份 i18next 實例，所以這一組要呼叫兩次。少掛一邊的症狀是
  // 「換語言後設定視窗變了、候選框沒變」，看起來像事件派送壞了。
  await initI18n();
  watchLanguage();

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <ErrorBoundary
        title={i18next.t("common.crash.title")}
        retryLabel={i18next.t("common.crash.retry")}
      >
        <Settings />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}

void bootstrap();
