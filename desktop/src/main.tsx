import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App.tsx";
import { followLinksExternally } from "./lib/links";
import "./index.css";

followLinksExternally();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
