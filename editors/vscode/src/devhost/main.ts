import * as path from "node:path";
import { spawn } from "node:child_process";
import { setRoot, stub } from "./vscodeStub";

function arg(name: string, fallback?: string): string | undefined {
  const at = process.argv.indexOf(`--${name}`);
  return at !== -1 && process.argv[at + 1] ? process.argv[at + 1] : fallback;
}

/** Re-exec in a detached child so the foreground call returns immediately. */
if (process.argv.includes("--background")) {
  const args = process.argv.filter((a) => a !== "--background");
  const child = spawn(process.execPath, args, {
    cwd: process.cwd(),
    stdio: "ignore",
    detached: true,
  });
  child.unref();
  console.log(child.pid);
  process.exit(0);
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
