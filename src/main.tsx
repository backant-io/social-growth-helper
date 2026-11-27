import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { inTauri, routeExternalLinks } from "./lib/api";
import "@fontsource-variable/bricolage-grotesque/index.css";
import "@fontsource-variable/instrument-sans/index.css";
import "@fontsource-variable/jetbrains-mono/index.css";
import "./index.css";

routeExternalLinks();
if (inTauri() && /Mac/i.test(navigator.userAgent)) {
  document.documentElement.dataset.vibrancy = "sidebar";
  document.documentElement.dataset.titlebar = "overlay";
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
