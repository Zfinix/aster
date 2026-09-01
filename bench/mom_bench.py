"""Live MoM policy test on HumanEval via OpenRouter.

Runs cheap + strong models on all problems, grades by executing canonical
tests, then simulates policies offline: always-cheap, always-strong, and
the MoM cascade (cheap, escalate to strong on observed test failure).
"""
import gzip
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).parent
CHEAP = "deepseek/deepseek-v4-flash"
STRONG = "anthropic/claude-opus-5"
DATA_URL = "https://github.com/openai/human-eval/raw/master/data/HumanEval.jsonl.gz"
API = "https://openrouter.ai/api/v1/chat/completions"
RESULTS = HERE / "mom_bench_results.jsonl"
PRELUDE = "from typing import List, Dict, Tuple, Optional, Any\nimport math\nimport re\n"


def api_key():
    for line in (Path.home() / ".aster/.env").read_text().splitlines():
        if line.startswith("OPEN_ROUTER_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"')
    raise SystemExit("no OPEN_ROUTER_API_KEY in ~/.aster/.env")


KEY = api_key()


def load_problems():
    p = HERE / "HumanEval.jsonl.gz"
    if not p.exists():
        urllib.request.urlretrieve(DATA_URL, p)
    out = []
    with gzip.open(p, "rt") as f:
        for line in f:
            out.append(json.loads(line))
    return out


def ask(model, prompt, tries=4):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": (
            "Complete this Python function. Output ONLY the full function "
            "definition (including the given signature, docstring handling, "
            "and any needed imports) inside one ```python code block. "
            "No explanation.\n\n```python\n" + prompt + "\n```"
        )}],
        "temperature": 0,
        "max_tokens": 1600,
        "usage": {"include": True},
    }).encode()
    for attempt in range(tries):
        req = urllib.request.Request(API, data=body, headers={
            "Authorization": f"Bearer {KEY}",
            "Content-Type": "application/json",
        })
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                d = json.load(r)
            if "choices" not in d or not d["choices"]:
                raise ValueError(str(d)[:200])
            u = d.get("usage", {})
            return {
                "text": d["choices"][0]["message"]["content"] or "",
                "prompt_tokens": u.get("prompt_tokens", 0),
                "completion_tokens": u.get("completion_tokens", 0),
                "cost": u.get("cost", 0.0),
            }
        except Exception as e:
            if attempt == tries - 1:
                return {"text": "", "prompt_tokens": 0, "completion_tokens": 0,
                        "cost": 0.0, "error": str(e)[:200]}
            time.sleep(3 * (attempt + 1))


def extract(text):
    blocks = re.findall(r"```(?:python)?\n(.*?)```", text, re.S)
    return blocks[-1] if blocks else text


def grade(problem, completion_text):
    code = extract(completion_text)
    program = PRELUDE + code + "\n\n" + problem["test"] + \
        f"\ncheck({problem['entry_point']})\n"
    with tempfile.NamedTemporaryFile("w", suffix=".py", dir=HERE, delete=False) as f:
        f.write(program)
        path = f.name
    try:
        r = subprocess.run([sys.executable, path], capture_output=True,
                           timeout=15, cwd=HERE)
        return r.returncode == 0
    except subprocess.TimeoutExpired:
        return False
    finally:
        os.unlink(path)


def run_one(problem, model):
    resp = ask(model, problem["prompt"])
    ok = grade(problem, resp["text"]) if resp["text"] else False
    return {
        "task_id": problem["task_id"], "model": model, "pass": ok,
        "prompt_tokens": resp["prompt_tokens"],
        "completion_tokens": resp["completion_tokens"],
        "cost": resp["cost"], "error": resp.get("error"),
    }


def main():
    problems = load_problems()
    print(f"{len(problems)} problems; cheap={CHEAP} strong={STRONG}", flush=True)
    done = set()
    if RESULTS.exists():
        for line in RESULTS.read_text().splitlines():
            r = json.loads(line)
            done.add((r["task_id"], r["model"]))
    jobs = [(p, m) for p in problems for m in (CHEAP, STRONG)
            if (p["task_id"], m) not in done]
    print(f"{len(jobs)} calls to make ({len(done)} cached)", flush=True)
    with RESULTS.open("a") as out, ThreadPoolExecutor(max_workers=8) as ex:
        futs = {ex.submit(run_one, p, m): (p["task_id"], m) for p, m in jobs}
        for i, fut in enumerate(as_completed(futs)):
            r = fut.result()
            out.write(json.dumps(r) + "\n")
            out.flush()
            if (i + 1) % 20 == 0:
                print(f"  {i+1}/{len(jobs)}", flush=True)
    print("done", flush=True)


if __name__ == "__main__":
    main()
