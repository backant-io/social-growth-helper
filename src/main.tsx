import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { inTauri } from "./lib/api";
import "./index.css";

if (inTauri() && /Mac/i.test(navigator.userAgent)) document.documentElement.dataset.vibrancy = "sidebar";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
