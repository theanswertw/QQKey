import React from "react";
import ReactDOM from "react-dom/client";
import Launcher from "./Launcher";
import "./launcher.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Launcher />
  </React.StrictMode>,
);
