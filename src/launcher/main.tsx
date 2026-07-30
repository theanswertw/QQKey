import i18next from "i18next";
import React from "react";
import ReactDOM from "react-dom/client";
import Launcher from "./Launcher";
import ErrorBoundary from "../shared/ErrorBoundary";
import { initI18n, watchLanguage } from "../i18n";
import "./launcher.css";

async function bootstrap() {
  // render 之前就把語系定下來。用 Suspense 的話首幀會是空的，而候選框的首幀
  // 就是使用者按下快捷鍵當下看到的東西。initI18n() 不會 reject。
  await initI18n();
  watchLanguage();

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <ErrorBoundary
        title={i18next.t("common.crash.title")}
        retryLabel={i18next.t("common.crash.retry")}
      >
        <Launcher />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}

// 具名 async 函式而不是 top-level await：不依賴 Vite 的 TLA 輸出行為，
// 而且錯誤處理的位置是明確的。
void bootstrap();
