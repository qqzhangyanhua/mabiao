import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles.css";

function currentLabel(): string {
  try {
    return getCurrentWebviewWindow().label;
  } catch {
    return "main";
  }
}

function requireRoot(): HTMLElement {
  const root = document.getElementById("root");
  if (!root) {
    throw new Error("root element is missing");
  }
  return root;
}

async function boot() {
  const App =
    currentLabel() === "tray-quota"
      ? (await import("./TrayQuotaApp")).default
      : (await import("./App")).default;
  ReactDOM.createRoot(requireRoot()).render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>,
  );
}

void boot();
