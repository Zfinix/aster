# MoM bench

The live benchmark behind the MoM 1.0 announcement numbers. Runs HumanEval and
MBPP over OpenRouter, grades answers by executing the canonical tests, and
simulates switching policies offline from the recorded per-problem results.

Reads `OPEN_ROUTER_API_KEY` from `~/.aster/.env`. Costs real money to re-run;
the recorded results in `results/` are free to re-analyze.

| Script | What it does |
| --- | --- |
| `mom_bench.py` | HumanEval: run cheap and strong models on every problem, grade, simulate the cascade. |
| `mom_bench2.py` | Same harness generalized: any benchmark (humaneval, mbpp), any model list. |
| `mom_analyze.py`, `mom_analyze2.py` | Score a results file: per-model pass rate, cost, and cascade simulation. |
| `live_dash.py` | The auto-refreshing dashboard over `results/`. |

`results/` holds one JSON line per (model, problem): task id, pass, cost, and
token counts. The headline numbers reproduce from `results/` alone:

- Cascade qwen3-coder-next → sonnet-5 → opus-5 on HumanEval: 164/164 for $0.09.
  Eight problems escalated to Sonnet, one of those to Opus.
- Claude Opus 5 alone: 163/164 for $1.34.

The cascade is simulated from recorded runs: every problem takes the cheap
model's recorded result, and a failure substitutes the stronger model's
recorded result, which is exactly what the policy would have executed.
