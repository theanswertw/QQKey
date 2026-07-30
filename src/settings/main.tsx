import React from "react";
import ReactDOM from "react-dom/client";
import Settings from "./Settings";
import ErrorBoundary from "../shared/ErrorBoundary";
import "./settings.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <Settings />
    </ErrorBoundary>
  </React.StrictMode>,
);
