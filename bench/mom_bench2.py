"""Generalized MoM policy bench: HumanEval + MBPP, any model list."""
import gzip, json, os, re, subprocess, sys, tempfile, time, urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).parent
API = "https://openrouter.ai/api/v1/chat/completions"
PRELUDE = "from typing import List, Dict, Tuple, Optional, Any\nimport math\nimport re\n"

def api_key():
    for line in (Path.home() / ".aster/.env").read_text().splitlines():
        if line.startswith("OPEN_ROUTER_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"')
    raise SystemExit("no key")
KEY = api_key()

def ds_key():
    for line in (Path.home() / ".aster/.env").read_text().splitlines():
        if line.startswith("DEEPSEEK_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"')
    raise SystemExit("no DEEPSEEK_API_KEY")

DS_PRICES = {"deepseek-v4-flash": (0.079e-6, 0.159e-6),
             "deepseek-v4-pro": (1.031e-6, 2.062e-6)}

def load_humaneval():
    p = HERE / "HumanEval.jsonl.gz"
    if not p.exists():
        urllib.request.urlretrieve("https://github.com/openai/human-eval/raw/master/data/HumanEval.jsonl.gz", p)
    out = []
    with gzip.open(p, "rt") as f:
        for line in f:
            d = json.loads(line)
            prompt = ("Complete this Python function. Output ONLY the full function "
                      "definition (including the given signature and any needed imports) "
                      "inside one ```python code block. No explanation.\n\n```python\n"
                      + d["prompt"] + "\n```")
            check = d["test"] + f"\ncheck({d['entry_point']})\n"
            out.append({"task_id": d["task_id"], "prompt": prompt, "check": check})
    return out

def load_mbpp():
    p = HERE / "mbpp.jsonl"
    if not p.exists():
        urllib.request.urlretrieve("https://raw.githubusercontent.com/google-research/google-research/master/mbpp/mbpp.jsonl", p)
    out = []
    for line in p.read_text().splitlines():
        d = json.loads(line)
        if not (11 <= d["task_id"] <= 510):   # standard test split
            continue
        tests = "\n".join(d["test_list"])
        prompt = ("Write a Python function for this task. Output ONLY the code "
                  "inside one ```python code block. No explanation.\n\nTask: "
                  + d["text"] + "\n\nYour code must pass these tests:\n```python\n"
                  + tests + "\n```")
        setup = d.get("test_setup_code") or ""
        out.append({"task_id": f"mbpp/{d['task_id']}", "prompt": prompt,
                    "check": setup + "\n" + tests + "\n"})
    return out

def ask(model, prompt, tries=4):
    direct = model.startswith("ds:")
    url = "https://api.deepseek.com/chat/completions" if direct else API
    key = ds_key() if direct else KEY
    mid = model[3:] if direct else model
    payload = {"model": mid,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0, "max_tokens": 1600,
        "usage": {"include": True}}
    if not direct and mid.startswith("anthropic/"):
        payload["provider"] = {"order": ["anthropic"], "allow_fallbacks": False}
    body = json.dumps(payload).encode()
    for attempt in range(tries):
        req = urllib.request.Request(url, data=body, headers={
            "Authorization": f"Bearer {key}", "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                d = json.load(r)
            if not d.get("choices"):
                raise ValueError(str(d)[:200])
            u = d.get("usage", {})
            if direct and "cost" not in u:
                pi, po = DS_PRICES.get(mid, (0, 0))
                u["cost"] = u.get("prompt_tokens", 0)*pi + u.get("completion_tokens", 0)*po
            return {"text": d["choices"][0]["message"]["content"] or "",
                    "prompt_tokens": u.get("prompt_tokens", 0),
                    "completion_tokens": u.get("completion_tokens", 0),
                    "cost": u.get("cost", 0.0)}
        except Exception as e:
            if attempt == tries - 1:
                return {"text": "", "prompt_tokens": 0, "completion_tokens": 0,
                        "cost": 0.0, "error": str(e)[:200]}
            time.sleep(3 * (attempt + 1))

def grade(problem, text):
    m = re.findall(r"```(?:python)?\n(.*?)```", text, re.S)
    code = m[-1] if m else text
    program = PRELUDE + code + "\n\n" + problem["check"]
    with tempfile.NamedTemporaryFile("w", suffix=".py", dir=HERE, delete=False) as f:
        f.write(program); path = f.name
    try:
        r = subprocess.run([sys.executable, path], capture_output=True, timeout=15, cwd=HERE)
        return r.returncode == 0
    except subprocess.TimeoutExpired:
        return False
    finally:
        os.unlink(path)

def looks_complete(text):
    import ast as _ast
    m = re.findall(r"```(?:python)?\n(.*?)```", text, re.S)
    if not m:
        return False
    try:
        _ast.parse(m[-1])
        return True
    except SyntaxError:
        return False

def run_one(problem, model):
    resp = ask(model, problem["prompt"])
    for _ in range(2):
        if resp["text"] and not looks_complete(resp["text"]):
            retry = ask(model, problem["prompt"])
            for k in ("prompt_tokens", "completion_tokens", "cost"):
                retry[k] += resp[k]
            resp = retry
        else:
            break
    ok = grade(problem, resp["text"]) if resp["text"] else False
    return {"task_id": problem["task_id"], "model": model, "pass": ok,
            "snippet": None if ok else resp["text"][:200],
            "prompt_tokens": resp["prompt_tokens"],
            "completion_tokens": resp["completion_tokens"],
            "cost": resp["cost"], "error": resp.get("error")}

def main():
    dataset, results_file = sys.argv[1], Path(sys.argv[2])
    models = sys.argv[3:]
    problems = load_humaneval() if dataset == "humaneval" else load_mbpp()
    done = set()
    if results_file.exists():
        for line in results_file.read_text().splitlines():
            r = json.loads(line); done.add((r["task_id"], r["model"]))
    jobs = [(p, m) for p in problems for m in models if (p["task_id"], m) not in done]
    print(f"{dataset}: {len(problems)} problems, {len(jobs)} calls ({len(done)} cached)", flush=True)
    with results_file.open("a") as out, ThreadPoolExecutor(max_workers=int(os.environ.get("MOMW", "8"))) as ex:
        futs = {ex.submit(run_one, p, m): 0 for p, m in jobs}
        for i, fut in enumerate(as_completed(futs)):
            out.write(json.dumps(fut.result()) + "\n"); out.flush()
            if (i + 1) % 50 == 0:
                print(f"  {i+1}/{len(jobs)}", flush=True)
    print("done", flush=True)

if __name__ == "__main__":
    main()
