"""Mine aster sessions for harness pathologies. Read-only."""

import collections
import glob
import json
import os
import sys
import time

DAYS = float(sys.argv[1]) if len(sys.argv) > 1 else 30
ROOT = os.environ.get("ASTER_SESSIONS", os.path.expanduser("~/.local/share/aster/sessions"))
cutoff = time.time() - DAYS * 86400

dupes = collections.Counter()
pointers = 0
errors = collections.Counter()
error_samples = []
barren_args = collections.defaultdict(list)
big_results = []
retry_streaks = collections.Counter()
tool_totals = collections.Counter()

for path in glob.glob(os.path.join(ROOT, "*", "*.jsonl")):
    if os.path.getmtime(path) < cutoff:
        continue
    calls, seen, last_tool, streak = {}, set(), None, 0
    for line in open(path):
        try:
            ev = json.loads(line)
        except Exception:
            continue
        if ev.get("role") == "assistant":
            for c in ev.get("tool_calls") or []:
                fn = c.get("function") or {}
                calls[c["id"]] = (fn.get("name"), fn.get("arguments") or "")
                key = (fn.get("name"), fn.get("arguments"))
                if key in seen:
                    dupes[fn.get("name")] += 1
                seen.add(key)
        if ev.get("role") != "tool":
            continue
        name, args = calls.get(ev.get("tool_call_id"), (None, ""))
        if not name:
            continue
        tool_totals[name] += 1
        out = (ev.get("content") or "").strip()

        if "identical" in out and "earlier in this turn" in out:
            pointers += 1
        if out.startswith("error: ") or "exit code: 1" in out:
            errors[name] += 1
            if len(error_samples) < 25:
                error_samples.append((name, args[:90], out[:110]))
        if out in ("no matches", "no files matched") and len(barren_args[name]) < 10:
            barren_args[name].append(args[:110])
        if len(out) > 20000:
            big_results.append((name, len(out)))
        # Same tool fired back to back across rounds: the flailing signature.
        if name == last_tool:
            streak += 1
            if streak >= 3:
                retry_streaks[name] += 1
        else:
            streak = 0
        last_tool = name

print(f"window: last {DAYS} days\n")
print("tool calls:", dict(tool_totals.most_common()))
print("\nduplicate (tool,args) pairs:", dupes.most_common(8))
print("dedupe pointers actually returned:", pointers)
print("\nerror/exit-1 results by tool:", errors.most_common(8))
print("consecutive same-tool streaks (>=3 in a row):", retry_streaks.most_common(8))
print("oversized results (>20k chars):", big_results[:10])
for tool, samples in barren_args.items():
    print(f"\nbarren {tool}:")
    for s in samples:
        print("   ", s)
print("\nerror samples:")
for name, args, out in error_samples:
    print(f"  {name}({args}) -> {out}")
