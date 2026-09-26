import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppErrorBoundary } from "./components/AppErrorBoundary";
import { I18nProvider } from "./i18n";
import "./styles.css";

// Apply the WebView editing policy to dynamically mounted fields and dialog portals.
document.addEventListener("focus", (event) => {
  const field = event.target;
  if (!(field instanceof HTMLElement) || !field.matches("input, textarea, [contenteditable]")) return;

  field.setAttribute("spellcheck", "false");
  field.setAttribute("autocorrect", "off");
  field.setAttribute("autocapitalize", "off");
  field.setAttribute("writingsuggestions", "false");
}, true);

ReactDOM.createRoot(document.getElementById("app") as HTMLElement).render(
  <AppErrorBoundary>
    <I18nProvider>
      <App />
    </I18nProvider>
  </AppErrorBoundary>
);
