"""Compute MoM policy outcomes from mom_bench_results.jsonl."""
import json
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).parent
CHEAP = "deepseek/deepseek-v4-flash"
STRONG = "anthropic/claude-opus-5"

by = defaultdict(dict)
for line in (HERE / "mom_bench_results.jsonl").read_text().splitlines():
    r = json.loads(line)
    by[r["task_id"]][r["model"]] = r

tasks = sorted(t for t, d in by.items() if CHEAP in d and STRONG in d)
n = len(tasks)
print(f"{n} problems with both runs\n")


def tot(sel):
    passes = cost = ptok = ctok = 0
    for t in tasks:
        p, c, pt, ct = sel(by[t])
        passes += p
        cost += c
        ptok += pt
        ctok += ct
    return passes, cost, ptok, ctok


def row(name, passes, cost, ptok, ctok, esc=None):
    extra = f"  escalated {esc}/{n}" if esc is not None else ""
    print(f"{name:28s} {passes:3d}/{n} = {100*passes/n:5.1f}%   "
          f"${cost:7.4f}   {ptok+ctok:>9,} tok{extra}")


always_cheap = tot(lambda d: (d[CHEAP]["pass"],
                              d[CHEAP]["cost"],
                              d[CHEAP]["prompt_tokens"],
                              d[CHEAP]["completion_tokens"]))
always_strong = tot(lambda d: (d[STRONG]["pass"],
                               d[STRONG]["cost"],
                               d[STRONG]["prompt_tokens"],
                               d[STRONG]["completion_tokens"]))


def cascade(d):
    c, s = d[CHEAP], d[STRONG]
    if c["pass"]:
        return (1, c["cost"], c["prompt_tokens"], c["completion_tokens"])
    return (s["pass"], c["cost"] + s["cost"],
            c["prompt_tokens"] + s["prompt_tokens"],
            c["completion_tokens"] + s["completion_tokens"])


mom = tot(cascade)
escalations = sum(1 for t in tasks if not by[t][CHEAP]["pass"])

print(f"{'policy':28s} {'pass rate':>16s}   {'cost':>8s}   {'tokens':>12s}")
row(f"always cheap ({CHEAP.split('/')[1]})", *always_cheap)
row(f"always max ({STRONG.split('/')[1]})", *always_strong)
row("mom cascade (stuck->max)", *mom, esc=escalations)

sc, cc, mc = always_strong[1], always_cheap[1], mom[1]
sp, cp, mp = always_strong[0], always_cheap[0], mom[0]
print()
print(f"cascade vs always-max:   {100*(sc-mc)/sc:5.1f}% cheaper, "
      f"{mp-sp:+d} problems vs max's {sp}")
print(f"cascade vs always-cheap: {mp-cp:+d} problems recovered, "
      f"{(mc-cc)/cc*100:5.1f}% cost increase over cheap")

both_fail = [t for t in tasks
             if not by[t][CHEAP]["pass"] and not by[t][STRONG]["pass"]]
strong_only = [t for t in tasks
               if not by[t][CHEAP]["pass"] and by[t][STRONG]["pass"]]
cheap_only = [t for t in tasks
              if by[t][CHEAP]["pass"] and not by[t][STRONG]["pass"]]
print(f"\ncheap fails strong saves: {len(strong_only)}  "
      f"both fail: {len(both_fail)}  cheap passes strong fails: {len(cheap_only)}")
errs = [(t, m) for t in tasks for m in (CHEAP, STRONG) if by[t][m].get("error")]
if errs:
    print(f"api errors: {len(errs)} -> {errs[:5]}")
