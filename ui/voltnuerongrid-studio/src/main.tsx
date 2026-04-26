import React from "react";
import ReactDOM from "react-dom/client";
import { loader } from "@monaco-editor/react";
import { App } from "./App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import "./styles/globals.css";

// Pin Monaco to the installed version served from CDN.
// This is the most reliable cross-env setup for Vite + @monaco-editor/react.
loader.config({
  paths: {
    vs: "https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs",
  },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary label="App">
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
