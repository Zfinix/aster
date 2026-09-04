import { createRoot } from "react-dom/client";
import { App } from "./App";
import { PlanPage } from "./components/PlanPage";
import { inEditor } from "./lib/host";
import { inPlanTab } from "./lib/plan-tab";
import "./index.css";

const isPlanTab = !inEditor && inPlanTab();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(isPlanTab ? <PlanPage /> : <App />);
}
