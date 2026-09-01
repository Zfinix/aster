"""Live benchmark dashboard: aggregates result jsonl files, serves one page."""
import json
import glob
from http.server import BaseHTTPRequestHandler, HTTPServer
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).parent
EXPECT = {"humaneval": 164, "mbpp": 500}

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>MoM bench live</title><style>
* { margin:0; padding:0; box-sizing:border-box; }
body { background:#0a0a0a; color:#ededed; font-family:'Inter',-apple-system,sans-serif; padding:44px 56px; }
h1 { font-size:28px; font-weight:620; } h1 span { color:#c98500; }
.meta { color:#8b8a84; font-size:14px; margin-top:6px; }
.grid { display:grid; grid-template-columns:1fr 1fr; gap:36px; margin-top:30px; }
h2 { font-size:16px; font-weight:560; color:#c3c2b7; margin-bottom:14px; }
.row { display:grid; grid-template-columns:230px 1fr 150px; gap:14px; align-items:center; padding:7px 0; border-bottom:1px solid #1c1c1a; font-size:14px; }
.name { color:#ededed; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
.bar { height:14px; background:#1c1c1a; border-radius:7px; overflow:hidden; position:relative; }
.fill { height:100%; background:#3987e5; border-radius:7px; }
.fill.done { background:#c98500; }
.stat { color:#c3c2b7; text-align:right; font-variant-numeric:tabular-nums; }
.total { margin-top:26px; font-size:15px; color:#c3c2b7; }
.total b { color:#c98500; font-weight:620; }
</style></head><body>
<h1>MoM <span>live bench</span></h1>
<div class="meta" id="meta"></div>
<div class="grid"><div><h2>HumanEval · 164 problems</h2><div id="humaneval"></div></div>
<div><h2>MBPP · 500 problems</h2><div id="mbpp"></div></div></div>
<div class="total" id="total"></div>
<script>
async function tick(){
  const d = await (await fetch('/data')).json();
  for (const bench of ['humaneval','mbpp']){
    const el = document.getElementById(bench); el.innerHTML='';
    for (const m of d[bench]){
      const pct = Math.min(100, 100*m.n/m.expect);
      const done = m.n >= m.expect;
      el.innerHTML += `<div class="row"><div class="name">${m.model}</div>
        <div class="bar"><div class="fill ${done?'done':''}" style="width:${pct}%"></div></div>
        <div class="stat">${done ? (100*m.pass/m.n).toFixed(1)+'% · $'+m.cost.toFixed(2) : m.n+'/'+m.expect}</div></div>`;
    }
  }
  document.getElementById('total').innerHTML =
    `total calls <b>${d.calls.toLocaleString()}</b> · total spend <b>$${d.spend.toFixed(2)}</b> · models done <b>${d.done}/${d.models}</b>`;
  document.getElementById('meta').textContent = 'auto-refreshes · ' + new Date().toLocaleTimeString();
}
tick(); setInterval(tick, 3000);
</script></body></html>"""


def aggregate():
    agg = {"humaneval": defaultdict(lambda: [0, 0, 0.0]),
           "mbpp": defaultdict(lambda: [0, 0, 0.0])}
    for f in glob.glob(str(HERE / "*.jsonl")):
        name = Path(f).name
        bench = "mbpp" if name.startswith("mbpp") else \
                "humaneval" if name.startswith("humaneval") or name.startswith("mom_bench") else None
        if not bench:
            continue
        seen = {}
        for line in open(f):
            try:
                r = json.loads(line)
            except ValueError:
                continue
            if "model" not in r:
                continue
            k = (r["task_id"], r["model"])
            if k not in seen or r["pass"]:
                seen[k] = r
        for r in seen.values():
            a = agg[bench][r["model"]]
            a[0] += 1
            a[1] += 1 if r["pass"] else 0
            a[2] += r.get("cost") or 0
    out = {"calls": 0, "spend": 0.0, "models": 0, "done": 0}
    for bench, models in agg.items():
        rows = []
        for m, (n, p, c) in sorted(models.items(), key=lambda kv: -kv[1][1]/max(1,kv[1][0])):
            rows.append({"model": m, "n": n, "pass": p, "cost": c, "expect": EXPECT[bench]})
            out["calls"] += n
            out["spend"] += c
            out["models"] += 1
            out["done"] += n >= EXPECT[bench]
        out[bench] = rows
    return out


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        body = json.dumps(aggregate()).encode() if self.path == "/data" else PAGE.encode()
        ctype = "application/json" if self.path == "/data" else "text/html"
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("127.0.0.1", 8791), H).serve_forever()
