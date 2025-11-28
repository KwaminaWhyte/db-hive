import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { ThemeProvider } from "./components/theme-provider";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { PluginProvider } from "./contexts/PluginContext";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <ThemeProvider defaultTheme="dark" storageKey="db-hive-theme">
        <PluginProvider>
          <App />
        </PluginProvider>
      </ThemeProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);
