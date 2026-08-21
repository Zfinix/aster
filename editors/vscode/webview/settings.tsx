import { createRoot } from "react-dom/client";
import { SettingsApp } from "./settings/SettingsApp";
import "./settings.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<SettingsApp />);
}
