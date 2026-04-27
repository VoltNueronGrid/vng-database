import React from "react";
import ReactDOM from "react-dom/client";
import { loader } from "@monaco-editor/react";
import { App } from "./App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { initThemeWatcher } from "@/store/theme";
import "./styles/globals.css";

// Pin Monaco to the installed version served from CDN.
// This is the most reliable cross-env setup for Vite + @monaco-editor/react.
loader.config({
  paths: {
    vs: "https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs",
  },
});

// Initialise OS theme change listener (also syncs DOM on first paint).
initThemeWatcher();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary label="App">
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
