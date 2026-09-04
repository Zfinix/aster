import * as path from "node:path";
import { setRoot, stub } from "./vscodeStub";

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
