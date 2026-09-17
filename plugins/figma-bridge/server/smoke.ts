/**
 * Smoke test: spawn the server, speak MCP to it over stdio, print the answers.
 *
 * Run with: bun server/smoke.ts
 */

const proc = Bun.spawn(["bun", new URL("./index.ts", import.meta.url).pathname], {
  stdin: "pipe",
  stdout: "pipe",
  stderr: "inherit",
});

const send = (msg: unknown) => proc.stdin.write(JSON.stringify(msg) + "\n");
send({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} });
send({ jsonrpc: "2.0", method: "notifications/initialized" });
send({ jsonrpc: "2.0", id: 2, method: "tools/list" });
send({
  jsonrpc: "2.0",
  id: 3,
  method: "tools/call",
  params: { name: "execute", arguments: { code: "1 + 1" } },
});
proc.stdin.end();

const lines: string[] = [];
const done = new Promise<void>((resolve) => {
  (async () => {
    for await (const chunk of proc.stdout) {
      const text = new TextDecoder().decode(chunk);
      for (const line of text.split("\n")) {
        if (line.trim()) lines.push(line);
      }
      if (lines.length >= 3) resolve();
    }
  })();
});

const timer = setTimeout(() => {
  console.error("smoke: timed out waiting for responses");
  proc.kill();
  process.exit(1);
}, 15000);
await done;
clearTimeout(timer);

for (const line of lines) {
  const msg = JSON.parse(line);
  if (msg.id === 2) {
    console.log("tools:", msg.result.tools.map((t: any) => t.name).join(" "));
  } else if (msg.id === 3) {
    console.log("execute 1+1:", msg.result.content[0].text);
  } else {
    console.log("initialize:", JSON.stringify(msg).slice(0, 300));
  }
}
proc.kill();
process.exit(0);