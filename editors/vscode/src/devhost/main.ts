import * as path from "node:path";
import { setRoot, stub } from "./vscodeStub";

/**
 * Runs the panel in a browser against the real CLI. The webview never knew it
 * was in an editor — it posts JSON to a host that spawns `aster` — so serving
 * the same bundle with a host that answers the same protocol is enough.
 *
 *   bun run dev:web -- --cwd ../.. --port 4123
 */
function arg(name: string, fallback?: string): string | undefined {
  const at = process.argv.indexOf(`--${name}`);
  return at !== -1 && process.argv[at + 1] ? process.argv[at + 1] : fallback;
}

const root = path.resolve(arg("cwd", process.cwd()) as string);
const port = Number(arg("port", "4123"));

setRoot(root);

type Loader = {
  _load(request: string, parent: unknown, isMain: boolean): unknown;
};
const loader = require("node:module") as Loader;
const load = loader._load;
loader._load = function (request: string, parent: unknown, isMain: boolean): unknown {
  return request === "vscode" ? stub : load.call(this, request, parent, isMain);
};

const { start } = require("./server") as typeof import("./server");
start(root, port);
