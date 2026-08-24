import { createRoot } from "react-dom/client";
import { App } from "./App";
import { PlanPage } from "./components/PlanPage";
import { inEditor } from "./lib/host";
import { inPlanTab } from "./lib/plan-tab";
import "./index.css";

/** `aster serve` gives a plan a tab of its own; an editor opens it as a real
 *  editor tab instead, so this route only ever matches in a browser. */
const isPlanTab = !inEditor && inPlanTab();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(isPlanTab ? <PlanPage /> : <App />);
}
