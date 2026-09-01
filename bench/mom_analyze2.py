"""Cross-bench MoM policy analysis with bootstrap CIs."""
import json, random
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).parent
FLASH = "deepseek/deepseek-v4-flash"
SONNET = "anthropic/claude-sonnet-5"
OPUS = "anthropic/claude-opus-5"
random.seed(7)

def load(*files):
    by = defaultdict(dict)
    for fn in files:
        p = HERE / fn
        if not p.exists(): continue
        for line in p.read_text().splitlines():
            r = json.loads(line)
            by[r["task_id"]][r["model"]] = r
    return by

def policy_outcomes(by, tasks, chain):
    """chain = ordered models; escalate on observed test failure."""
    out = []
    for t in tasks:
        cost = tok = 0; ok = False
        for m in chain:
            r = by[t][m]
            cost += r["cost"]; tok += r["prompt_tokens"] + r["completion_tokens"]
            if r["pass"]:
                ok = True; break
        out.append((ok, cost, tok))
    return out

def summarize(name, out, n_esc=None):
    n = len(out)
    p = sum(o[0] for o in out); c = sum(o[1] for o in out); tk = sum(o[2] for o in out)
    # bootstrap CI on pass rate and cost
    prs, cs = [], []
    for _ in range(2000):
        s = [out[random.randrange(n)] for _ in range(n)]
        prs.append(sum(x[0] for x in s)/n); cs.append(sum(x[1] for x in s))
    prs.sort(); cs.sort()
    lo, hi = prs[50]*100, prs[1949]*100
    esc = f"  esc {n_esc}/{n}" if n_esc is not None else ""
    print(f"{name:34s} {p:4d}/{n} = {100*p/n:5.1f}%  [{lo:.1f},{hi:.1f}]  "
          f"${c:8.4f} [${cs[50]:.2f},${cs[1949]:.2f}]  {tk:>10,}t{esc}")
    return p, c

def bench(title, by, chains):
    tasks = sorted(t for t, d in by.items()
                   if all(m in d for ch in chains for m in ch[1]))
    n = len(tasks)
    print(f"\n=== {title} ({n} problems) ===")
    print(f"{'policy':34s} {'pass rate  [95% CI]':>26s}  {'cost  [95% CI]':>22s}")
    for name, chain in chains:
        esc = sum(1 for t in tasks if not by[t][chain[0]]["pass"]) if len(chain) > 1 else None
        summarize(name, policy_outcomes(by, tasks, chain), esc)
    errs = sum(1 for t in tasks for m in set(m for _, ch in chains for m in ch)
               if by[t].get(m, {}).get("error"))
    if errs: print(f"  (api errors: {errs})")

he = load("mom_bench_results.jsonl", "humaneval_extra_results.jsonl")
mb = load("mbpp_results.jsonl")

bench("HumanEval, 2-tier + 3-tier", he, [
    ("always flash", [FLASH]),
    ("always sonnet-5", [SONNET]),
    ("always opus-5", [OPUS]),
    ("mom: flash->opus", [FLASH, OPUS]),
    ("mom: flash->sonnet", [FLASH, SONNET]),
    ("mom: flash->sonnet->opus", [FLASH, SONNET, OPUS]),
])

alts = ["openai/gpt-5-nano", "qwen/qwen3.7-flash", "z-ai/glm-5.3-flash"]
bench("HumanEval, alternate cheap models", he,
      [(f"always {a.split('/')[1]}", [a]) for a in alts] +
      [(f"mom: {a.split('/')[1]}->opus", [a, OPUS]) for a in alts])

bench("MBPP test split, 2-tier + 3-tier", mb, [
    ("always flash", [FLASH]),
    ("always sonnet-5", [SONNET]),
    ("always opus-5", [OPUS]),
    ("mom: flash->opus", [FLASH, OPUS]),
    ("mom: flash->sonnet", [FLASH, SONNET]),
    ("mom: flash->sonnet->opus", [FLASH, SONNET, OPUS]),
])
