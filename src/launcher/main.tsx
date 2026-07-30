import React from "react";
import ReactDOM from "react-dom/client";
import Launcher from "./Launcher";
import ErrorBoundary from "../shared/ErrorBoundary";
import "./launcher.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <Launcher />
    </ErrorBoundary>
  </React.StrictMode>,
);
