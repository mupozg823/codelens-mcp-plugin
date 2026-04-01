#!/usr/bin/env python3
"""Token efficiency benchmark: CodeLens vs typical agent workflows.

Usage:
    python3 benchmarks/token-efficiency.py [project_path] [--check]

Measures actual token counts using tiktoken (cl100k_base) for both
CodeLens tool output and equivalent agent workflows (search + file reads).
No arbitrary multipliers — only real data.
"""

import argparse, subprocess, json, os, sys, glob, time, socket
from urllib import request as urllib_request
from urllib import error as urllib_error

# --- tiktoken setup ---
try:
    import tiktoken

    enc = tiktoken.get_encoding("cl100k_base")

    def count_tokens(text: str) -> int:
        return len(enc.encode(text)) if text else 0

except ImportError:
    print("WARNING: tiktoken not installed. Falling back to bytes/4 estimate.")
    print("Install with: pip3 install tiktoken\n")

def count_tokens(text: str) -> int:
        return len(text.encode("utf-8")) // 4 if text else 0


parser = argparse.ArgumentParser()
parser.add_argument("project_path", nargs="?", default=".")
parser.add_argument("--check", action="store_true")
parser.add_argument("--min-workflow-savings", type=float, default=35.0)
parser.add_argument("--min-chain-reduction", type=float, default=40.0)
parser.add_argument("--min-queue-depth", type=int, default=1)
parser.add_argument("--min-peak-workers", type=int, default=2)
parser.add_argument("--min-queue-success-rate", type=float, default=1.0)
parser.add_argument("--max-queue-failures", type=int, default=0)
parser.add_argument("--require-handle-win", action="store_true")
ARGS = parser.parse_args()

PROJECT = os.path.abspath(ARGS.project_path)
BIN = os.environ.get(
    "CODELENS_BIN",
    os.path.join(os.path.dirname(__file__), "..", "target", "release", "codelens-mcp"),
)
BIN = os.path.abspath(BIN)
LOW_LEVEL_TOOLS = {
    "find_symbol",
    "get_symbols_overview",
    "get_ranked_context",
    "get_changed_files",
    "get_impact_analysis",
    "find_referencing_symbols",
    "find_tests",
    "get_type_hierarchy",
    "plan_symbol_rename",
}


def percentile_95(values):
    if not values:
        return 0
    ordered = sorted(values)
    index = max(0, int(round(0.95 * (len(ordered) - 1))))
    return ordered[index]


def parse_output_json(output: str):
    text = (output or "").strip()
    if not text:
        return None
    try:
        return json.loads(text.splitlines()[-1])
    except Exception:
        return None


def codelens(cmd, args, timeout=15, preset=None, profile=None):
    """Run a CodeLens command and return output, token count, elapsed, parsed payload."""
    argv = [BIN, PROJECT]
    if profile:
        argv += ["--profile", profile]
    elif preset:
        argv += ["--preset", preset]
    argv += ["--cmd", cmd, "--args", json.dumps(args)]
    t0 = time.monotonic()
    try:
        r = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        elapsed = int((time.monotonic() - t0) * 1000)
        output = r.stdout or ""
        return output, count_tokens(output), elapsed, parse_output_json(output)
    except Exception as e:
        elapsed = int((time.monotonic() - t0) * 1000)
        return "", 0, elapsed, None


def read_file(path):
    """Read a file and return its content."""
    full = os.path.join(PROJECT, path) if not os.path.isabs(path) else path
    try:
        with open(full, "r", errors="replace") as f:
            return f.read()
    except:
        return ""


def run_search(pattern, include="*.rs", max_lines=50):
    """Run ripgrep and return output (simulating what an agent search tool returns)."""
    t0 = time.monotonic()
    try:
        r = subprocess.run(
            ["rg", "-n", pattern, ".", "-g", include],
            capture_output=True,
            text=True,
            timeout=10,
            cwd=PROJECT,
        )
        lines = r.stdout.strip().split("\n")[:max_lines]
        elapsed = int((time.monotonic() - t0) * 1000)
        return "\n".join(lines), elapsed
    except:
        return "", 0


def run_sequence(label, steps):
    """Run a tool sequence and compute workflow-visible token/latency metrics."""
    outputs = []
    total_tokens = 0
    total_ms = 0
    retries = 0
    low_level_calls = 0
    for step in steps:
        timeout = step.get("timeout", 20)
        output, tokens, elapsed_ms, payload = codelens(
            step["cmd"],
            step["args"],
            timeout=timeout,
            preset=step.get("preset"),
            profile=step.get("profile"),
        )
        if not output and not payload:
            retries += 1
            output, tokens, elapsed_ms, payload = codelens(
                step["cmd"],
                step["args"],
                timeout=timeout,
                preset=step.get("preset"),
                profile=step.get("profile"),
            )
        outputs.append(
            {
                "tool": step["cmd"],
                "surface": step.get("profile") or f"preset:{step.get('preset', 'balanced')}",
                "elapsed_ms": elapsed_ms,
                "tokens": tokens,
                "success": bool(payload and payload.get("success")),
            }
        )
        total_tokens += tokens
        total_ms += elapsed_ms
        if step["cmd"] in LOW_LEVEL_TOOLS:
            low_level_calls += 1
    return {
        "label": label,
        "tool_call_count": len(steps),
        "low_level_chain_count": low_level_calls if low_level_calls > 1 else 0,
        "total_tokens": total_tokens,
        "total_ms": total_ms,
        "retry_count": retries,
        "p95_latency_ms": percentile_95([entry["elapsed_ms"] for entry in outputs]),
        "steps": outputs,
    }


def compare_workflows(name, baseline_steps, compressed_steps):
    baseline = run_sequence(f"{name} baseline", baseline_steps)
    compressed = run_sequence(f"{name} compressed", compressed_steps)
    savings_pct = 0.0
    if baseline["total_tokens"] > 0:
        savings_pct = round(
            (1 - compressed["total_tokens"] / baseline["total_tokens"]) * 100, 1
        )
    return {
        "scenario": name,
        "baseline": baseline,
        "compressed": compressed,
        "savings_pct": savings_pct,
    }


def reserve_port():
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


def start_http_daemon():
    port = reserve_port()
    proc = subprocess.Popen(
        [BIN, PROJECT, "--transport", "http", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    base_url = f"http://127.0.0.1:{port}"
    card_url = f"{base_url}/.well-known/mcp.json"
    for _ in range(50):
        if proc.poll() is not None:
            return None, None, proc
        try:
            with urllib_request.urlopen(card_url, timeout=0.5) as resp:
                if resp.status == 200:
                    return base_url, port, proc
        except Exception:
            time.sleep(0.1)
    return None, None, proc


def stop_http_daemon(proc):
    if not proc:
        return
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)


def mcp_http_call(base_url, method, params=None, request_id=1):
    payload = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
    }
    if params is not None:
        payload["params"] = params
    req = urllib_request.Request(
        f"{base_url}/mcp",
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib_request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read().decode("utf-8"))


def mcp_http_tool_call(base_url, name, arguments, request_id=1):
    return mcp_http_call(
        base_url,
        "tools/call",
        {"name": name, "arguments": arguments},
        request_id=request_id,
    )


def run_queue_observability_benchmark():
    base_url, port, proc = start_http_daemon()
    if not base_url:
        stop_http_daemon(proc)
        return {"supported": False, "reason": "http transport unavailable"}
    try:
        first = mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": key_file or test_file or ".",
                "debug_step_delay_ms": 80,
                "profile_hint": "reviewer-graph",
            },
            request_id=11,
        )
        second = mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": test_file or key_file or ".",
                "debug_step_delay_ms": 20,
                "profile_hint": "reviewer-graph",
            },
            request_id=12,
        )
        third = mcp_http_tool_call(
            base_url,
            "start_analysis_job",
            {
                "kind": "impact_report",
                "path": key_file or test_file or ".",
                "debug_step_delay_ms": 20,
                "profile_hint": "reviewer-graph",
            },
            request_id=13,
        )
        first_job = first.get("result", {}).get("content", [{}])[0].get("text", "{}")
        second_job = second.get("result", {}).get("content", [{}])[0].get("text", "{}")
        third_job = third.get("result", {}).get("content", [{}])[0].get("text", "{}")
        first_payload = json.loads(first_job)
        second_payload = json.loads(second_job)
        third_payload = json.loads(third_job)
        first_id = first_payload["data"]["job_id"]
        second_id = second_payload["data"]["job_id"]
        third_id = third_payload["data"]["job_id"]
        saw_queued = False
        saw_running = False
        for idx in range(60):
            first_status = mcp_http_tool_call(
                base_url, "get_analysis_job", {"job_id": first_id}, request_id=100 + idx
            )
            second_status = mcp_http_tool_call(
                base_url, "get_analysis_job", {"job_id": second_id}, request_id=200 + idx
            )
            third_status = mcp_http_tool_call(
                base_url, "get_analysis_job", {"job_id": third_id}, request_id=300 + idx
            )
            first_data = json.loads(
                first_status.get("result", {}).get("content", [{}])[0].get("text", "{}")
            )["data"]
            second_data = json.loads(
                second_status.get("result", {}).get("content", [{}])[0].get("text", "{}")
            )["data"]
            third_data = json.loads(
                third_status.get("result", {}).get("content", [{}])[0].get("text", "{}")
            )["data"]
            saw_running = saw_running or any(
                job.get("status") == "running"
                for job in (first_data, second_data, third_data)
            )
            saw_queued = saw_queued or any(
                job.get("status") == "queued"
                for job in (first_data, second_data, third_data)
            )
            if (
                first_data.get("status") == "completed"
                and second_data.get("status") == "completed"
                and third_data.get("status") == "completed"
            ):
                break
            time.sleep(0.1)
        metrics_resp = mcp_http_tool_call(base_url, "get_tool_metrics", {}, request_id=999)
        metrics_payload = json.loads(
            metrics_resp.get("result", {}).get("content", [{}])[0].get("text", "{}")
        )["data"]
        session = metrics_payload.get("session", {})
        derived = metrics_payload.get("derived_kpis", {})
        queue_failures = session.get("analysis_jobs_failed", 0)
        queue_max_depth = session.get("analysis_queue_max_depth", 0)
        peak_workers = session.get("peak_active_analysis_workers", 0)
        success_rate = derived.get("analysis_job_success_rate", 0.0)
        queue_checks = {
            "min_queue_depth": ARGS.min_queue_depth,
            "min_peak_workers": ARGS.min_peak_workers,
            "min_queue_success_rate": ARGS.min_queue_success_rate,
            "max_queue_failures": ARGS.max_queue_failures,
        }
        queue_failures_list = []
        if queue_max_depth < ARGS.min_queue_depth:
            queue_failures_list.append(
                f"queue depth {queue_max_depth} < required {ARGS.min_queue_depth}"
            )
        if peak_workers < ARGS.min_peak_workers:
            queue_failures_list.append(
                f"peak workers {peak_workers} < required {ARGS.min_peak_workers}"
            )
        if queue_failures > ARGS.max_queue_failures:
            queue_failures_list.append(
                f"queue failures {queue_failures} > allowed {ARGS.max_queue_failures}"
            )
        if success_rate < ARGS.min_queue_success_rate:
            queue_failures_list.append(
                f"queue success rate {success_rate:.2f} < required {ARGS.min_queue_success_rate:.2f}"
            )
        return {
            "supported": True,
            "saw_running": saw_running,
            "saw_queued": saw_queued,
            "session": {
                "analysis_jobs_enqueued": session.get("analysis_jobs_enqueued", 0),
                "analysis_jobs_started": session.get("analysis_jobs_started", 0),
                "analysis_jobs_completed": session.get("analysis_jobs_completed", 0),
                "analysis_jobs_failed": session.get("analysis_jobs_failed", 0),
                "analysis_jobs_cancelled": session.get("analysis_jobs_cancelled", 0),
                "analysis_queue_depth": session.get("analysis_queue_depth", 0),
                "analysis_queue_max_depth": session.get("analysis_queue_max_depth", 0),
                "active_analysis_workers": session.get("active_analysis_workers", 0),
                "peak_active_analysis_workers": session.get(
                    "peak_active_analysis_workers", 0
                ),
                "analysis_worker_limit": session.get("analysis_worker_limit", 0),
                "analysis_transport_mode": session.get("analysis_transport_mode", "unknown"),
            },
            "derived_kpis": {
                "analysis_job_success_rate": success_rate
            },
            "checks": queue_checks,
            "gate_passed": len(queue_failures_list) == 0 and saw_running and saw_queued,
            "gate_failures": queue_failures_list,
            "port": port,
        }
    except Exception as exc:
        return {"supported": False, "reason": str(exc)}
    finally:
        stop_http_daemon(proc)


# --- Discover project info ---
info_out, _, _, info_payload = codelens("get_project_structure", {}, preset="balanced")
total_files = total_symbols = "?"
if info_payload:
    try:
        total_files = info_payload["data"]["total_files"]
        total_symbols = info_payload["data"]["total_symbols"]
    except:
        pass

# Detect language (exclude vendored dirs)
EXCLUDE_DIRS = {
    "node_modules",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".git",
}
src_exts = [".rs", ".py", ".ts", ".js", ".go", ".java"]
ext_counts = {}
for ext in src_exts:
    all_files = glob.glob(os.path.join(PROJECT, "**/*" + ext), recursive=True)
    filtered = [
        f for f in all_files if not any(d in f.split(os.sep) for d in EXCLUDE_DIRS)
    ]
    if filtered:
        ext_counts[ext] = len(filtered)
primary_ext = max(ext_counts, key=ext_counts.get) if ext_counts else ".rs"
grep_include = "*" + primary_ext

# Find test targets — use a specific, non-trivial symbol (not "main" which matches too broadly)
CANDIDATE_SYMBOLS = [
    "dispatch_tool",
    "handle_request",
    "process",
    "execute",
    "run_server",
    "parse_args",
    "build",
    "create_app",
    "init",
    "setup",
]
test_symbol = None
test_file = None

for candidate in CANDIDATE_SYMBOLS:
    sym_out, _, _, sym_payload = codelens(
        "find_symbol", {"name": candidate, "max_matches": 1}, preset="balanced"
    )
    if sym_payload:
        try:
            sd = sym_payload
            if sd.get("data", {}).get("count", 0) > 0:
                s = sd["data"]["symbols"][0]
                test_symbol = s.get("name", candidate)
                test_file = s.get("file_path")
                break
        except:
            pass

# Fallback to "main" only if nothing better found
if not test_symbol:
    sym_out, _, _, sym_payload = codelens(
        "find_symbol", {"name": "main", "max_matches": 1}, preset="balanced"
    )
    test_symbol = "main"
    if sym_payload:
        try:
            sd = sym_payload
            if sd.get("data", {}).get("count", 0) > 0:
                s = sd["data"]["symbols"][0]
                test_symbol = s.get("name", "main")
                test_file = s.get("file_path")
        except:
            pass

onboard_out, _, _, onboard_payload = codelens(
    "onboard_project", {}, timeout=30, preset="balanced"
)
key_file = None
key_files_list = []
if onboard_payload:
    try:
        kd = onboard_payload
        kf = kd.get("data", {}).get("key_files", [])
        key_files_list = [f["file"] for f in kf[:5]]
        if kf:
            key_file = kf[0]["file"]
    except:
        pass

if not test_file:
    all_src = glob.glob(os.path.join(PROJECT, "**/*" + primary_ext), recursive=True)
    if all_src:
        test_file = os.path.relpath(all_src[0], PROJECT)
if not key_file:
    key_file = test_file

# ================================================================
# BENCHMARKS — fair 1:1 comparisons
# ================================================================
results = []

print(f"## CodeLens Token Efficiency Benchmark (tiktoken cl100k_base)")
print(
    f"Project: {os.path.basename(PROJECT)} ({total_files} files, {total_symbols} symbols)"
)
print(f"Primary language: {primary_ext}\n")

# --- Task 1: Find a symbol ---
# Baseline: grep for the symbol name (what an agent would actually do)
if test_file:
    search_out, search_ms = run_search(test_symbol, grep_include, max_lines=30)
    grep_tokens = count_tokens(search_out)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "find_symbol", {"name": test_symbol, "include_body": True}, preset="balanced"
    )

    if cl_tokens > 0 and grep_tokens > 0:
        results.append(
            {
                "task": f"Find symbol '{test_symbol}'",
                "baseline_method": f"rg -n '{test_symbol}' (30 lines)",
                "baseline_tokens": grep_tokens,
                "baseline_ms": search_ms,
                "codelens_method": "find_symbol include_body=true",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# --- Task 2: Understand file structure ---
# Baseline: read the entire file (what an agent does to understand structure)
if test_file:
    file_content = read_file(test_file)
    file_tokens = count_tokens(file_content)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "get_symbols_overview", {"path": test_file}, preset="balanced"
    )

    if cl_tokens > 0 and file_tokens > 0:
        results.append(
            {
                "task": "Understand file structure",
                "baseline_method": f"Read entire {test_file}",
                "baseline_tokens": file_tokens,
                "baseline_ms": 0,
                "codelens_method": "get_symbols_overview",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# --- Task 3: Impact analysis ---
# Baseline: read the target file + grep for its imports/references
if key_file:
    file_content = read_file(key_file)
    basename = os.path.splitext(os.path.basename(key_file))[0]
    search_out, search_ms = run_search(basename, grep_include, max_lines=50)
    baseline_text = file_content + "\n" + search_out
    baseline_tokens = count_tokens(baseline_text)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "get_impact_analysis", {"file_path": key_file}, preset="balanced"
    )

    if cl_tokens > 0 and baseline_tokens > 0:
        results.append(
            {
                "task": "Impact analysis",
                "baseline_method": f"Read {key_file} + rg references",
                "baseline_tokens": baseline_tokens,
                "baseline_ms": search_ms,
                "codelens_method": "get_impact_analysis",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# --- Task 4: Find references ---
# Baseline: grep for the symbol across the project
if test_file:
    search_out, search_ms = run_search(test_symbol, grep_include, max_lines=50)
    grep_tokens = count_tokens(search_out)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "find_referencing_symbols",
        {"file_path": test_file, "symbol_name": test_symbol},
        preset="balanced",
    )

    if cl_tokens > 0 and grep_tokens > 0:
        results.append(
            {
                "task": "Find references",
                "baseline_method": f"rg -n '{test_symbol}' (50 lines)",
                "baseline_tokens": grep_tokens,
                "baseline_ms": search_ms,
                "codelens_method": "find_referencing_symbols",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# --- Task 5: Project onboarding ---
# Baseline: read key files (Cargo.toml/package.json + main entry + README head)
if True:
    onboard_baseline = ""
    manifest = None
    for m in ["Cargo.toml", "package.json", "go.mod", "pyproject.toml", "setup.py"]:
        if os.path.isfile(os.path.join(PROJECT, m)):
            manifest = m
            break
    if manifest:
        onboard_baseline += read_file(manifest) + "\n"
    if test_file:
        onboard_baseline += read_file(test_file) + "\n"
    readme = None
    for r in ["README.md", "readme.md", "README.rst"]:
        if os.path.isfile(os.path.join(PROJECT, r)):
            readme = r
            break
    if readme:
        content = read_file(readme)
        onboard_baseline += "\n".join(content.split("\n")[:80]) + "\n"
    # Also list source files (like `find`)
    src_list = glob.glob(os.path.join(PROJECT, "**/*" + primary_ext), recursive=True)
    onboard_baseline += "\n".join(os.path.relpath(f, PROJECT) for f in src_list[:30])

    baseline_tokens = count_tokens(onboard_baseline)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "onboard_project", {}, timeout=30, preset="balanced"
    )

    if cl_tokens > 0 and baseline_tokens > 0:
        results.append(
            {
                "task": "Project onboarding",
                "baseline_method": "Read manifest + entry + README(80L) + file list",
                "baseline_tokens": baseline_tokens,
                "baseline_ms": 0,
                "codelens_method": "onboard_project",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# --- Task 6: Context retrieval ---
# Baseline: grep for query keywords + read top matching files
# This simulates what an agent actually does: grep → find files → read them
if key_file:
    query = f"how does {test_symbol} work"
    search_out, search_ms = run_search(test_symbol, grep_include, max_lines=40)
    # Agent would read files from grep results (or the key file if no grep hits)
    grep_files = set()
    for line in search_out.split("\n"):
        if ":" in line:
            fpath = line.split(":")[0].lstrip("./")
            if os.path.isfile(os.path.join(PROJECT, fpath)):
                grep_files.add(fpath)
            if len(grep_files) >= 3:
                break
    # If grep found nothing, agent would read the file it knows about
    if not grep_files and test_file:
        grep_files.add(test_file)
    extra_content = ""
    for gf in grep_files:
        extra_content += read_file(gf) + "\n"
    baseline_text = (
        search_out + "\n" + extra_content if search_out.strip() else extra_content
    )
    baseline_tokens = count_tokens(baseline_text)

    cl_out, cl_tokens, cl_ms, _ = codelens(
        "get_ranked_context", {"query": query, "max_tokens": 8000}, preset="balanced"
    )

    if cl_tokens > 0 and baseline_tokens > 0:
        results.append(
            {
                "task": "Context retrieval",
                "baseline_method": f"rg '{test_symbol}' + read 2 files",
                "baseline_tokens": baseline_tokens,
                "baseline_ms": search_ms,
                "codelens_method": "get_ranked_context max_tokens=8000",
                "codelens_tokens": cl_tokens,
                "codelens_ms": cl_ms,
            }
        )

# ================================================================
# PROFILE / COMPOSITE WORKFLOW COMPARISON
# ================================================================
workflow_results = []

planner_task = f"understand where to implement changes around {test_symbol}"
workflow_results.append(
    compare_workflows(
        "Planner change request",
        baseline_steps=[
            {
                "cmd": "get_ranked_context",
                "args": {"query": planner_task, "max_tokens": 1200, "include_body": False, "depth": 2},
                "preset": "balanced",
            },
            {
                "cmd": "get_changed_files",
                "args": {"include_untracked": True},
                "preset": "balanced",
            },
        ],
        compressed_steps=[
            {
                "cmd": "analyze_change_request",
                "args": {"task": planner_task, "profile_hint": "planner-readonly"},
                "profile": "planner-readonly",
            }
        ],
    )
)

if key_file:
    workflow_results.append(
        compare_workflows(
            "Reviewer impact analysis",
            baseline_steps=[
                {
                    "cmd": "get_changed_files",
                    "args": {"include_untracked": True},
                    "preset": "balanced",
                },
                {
                    "cmd": "get_impact_analysis",
                    "args": {"file_path": key_file, "max_depth": 2},
                    "preset": "balanced",
                },
                {
                    "cmd": "find_referencing_symbols",
                    "args": {"file_path": key_file, "symbol_name": test_symbol, "max_results": 25},
                    "preset": "balanced",
                },
            ],
            compressed_steps=[
                {
                    "cmd": "impact_report",
                    "args": {"path": key_file},
                    "profile": "reviewer-graph",
                }
            ],
        )
    )

if test_file:
    workflow_results.append(
        compare_workflows(
            "Refactor safety",
            baseline_steps=[
                {
                    "cmd": "find_referencing_symbols",
                    "args": {"file_path": test_file, "symbol_name": test_symbol, "max_results": 50},
                    "preset": "balanced",
                },
                {
                    "cmd": "find_tests",
                    "args": {"path": test_file, "max_results": 10},
                    "preset": "balanced",
                },
                {
                    "cmd": "get_type_hierarchy",
                    "args": {"relative_path": test_file, "hierarchy_type": "both", "depth": 1},
                    "preset": "balanced",
                },
            ],
            compressed_steps=[
                {
                    "cmd": "refactor_safety_report",
                    "args": {
                        "task": f"refactor {test_symbol} safely",
                        "symbol": test_symbol,
                        "path": test_file,
                        "file_path": test_file,
                    },
                    "profile": "refactor-full",
                }
            ],
        )
    )

queue_observability = run_queue_observability_benchmark()

# ================================================================
# OUTPUT
# ================================================================
print("| Task | Baseline (tokens) | CodeLens (tokens) | Savings | Baseline Method |")
print("|------|-------------------|-------------------|---------|-----------------|")

total_baseline = 0
total_codelens = 0

for r in results:
    bl = r["baseline_tokens"]
    cl = r["codelens_tokens"]
    total_baseline += bl
    total_codelens += cl

    if cl < bl:
        pct = (1 - cl / bl) * 100
        ratio = bl / cl
        savings = f"{ratio:.1f}x ({pct:.0f}%)"
    elif cl > bl:
        pct = (cl / bl - 1) * 100
        savings = f"+{pct:.0f}% (CodeLens larger)"
    else:
        savings = "same"

    print(f"| {r['task']} | {bl:,} | {cl:,} | {savings} | {r['baseline_method']} |")

if total_codelens > 0 and total_baseline > 0:
    if total_codelens < total_baseline:
        ratio = total_baseline / total_codelens
        pct = (1 - total_codelens / total_baseline) * 100
        savings = f"{ratio:.1f}x ({pct:.0f}%)"
    else:
        pct = (total_codelens / total_baseline - 1) * 100
        savings = f"+{pct:.0f}%"
    print(
        f"| **Total** | **{total_baseline:,}** | **{total_codelens:,}** | **{savings}** | |"
    )

print()

print("## Profile / Composite Workflow Comparison\n")
print(
    "| Scenario | Balanced Tokens | Profile Tokens | Savings | Balanced Calls | Profile Calls | Balanced p95(ms) | Profile p95(ms) |"
)
print(
    "|----------|-----------------|----------------|---------|----------------|---------------|------------------|-----------------|"
)

for result in workflow_results:
    baseline = result["baseline"]
    compressed = result["compressed"]
    baseline_tokens = baseline["total_tokens"]
    compressed_tokens = compressed["total_tokens"]
    if baseline_tokens > 0:
        if compressed_tokens < baseline_tokens:
            ratio = baseline_tokens / compressed_tokens
            savings = f"{ratio:.1f}x ({result['savings_pct']:.0f}%)"
        else:
            savings = f"+{abs(result['savings_pct']):.0f}%"
    else:
        savings = "n/a"
    print(
        f"| {result['scenario']} | {baseline_tokens:,} | {compressed_tokens:,} | {savings} | "
        f"{baseline['tool_call_count']} | {compressed['tool_call_count']} | "
        f"{baseline['p95_latency_ms']} | {compressed['p95_latency_ms']} |"
    )

print("\n### Workflow KPI Details\n")
for result in workflow_results:
    baseline = result["baseline"]
    compressed = result["compressed"]
    print(f"- **{result['scenario']}**")
    print(
        f"  - Balanced: {baseline['tool_call_count']} calls, "
        f"{baseline['low_level_chain_count']} low-level chain steps, "
        f"{baseline['total_tokens']:,} tokens, p95 {baseline['p95_latency_ms']}ms, retries {baseline['retry_count']}"
    )
    print(
        f"  - Profile: {compressed['tool_call_count']} calls, "
        f"{compressed['low_level_chain_count']} low-level chain steps, "
        f"{compressed['total_tokens']:,} tokens, p95 {compressed['p95_latency_ms']}ms, retries {compressed['retry_count']}"
    )

print("\n## Queue Observability\n")
if queue_observability.get("supported"):
    session = queue_observability.get("session", {})
    checks = queue_observability.get("checks", {})
    print(
        f"- jobs: enqueued {session.get('analysis_jobs_enqueued', 0)}, "
        f"started {session.get('analysis_jobs_started', 0)}, "
        f"completed {session.get('analysis_jobs_completed', 0)}, "
        f"failed {session.get('analysis_jobs_failed', 0)}, "
        f"cancelled {session.get('analysis_jobs_cancelled', 0)}"
    )
    print(
        f"- queue depth: current {session.get('analysis_queue_depth', 0)}, "
        f"max {session.get('analysis_queue_max_depth', 0)}, "
        f"active workers {session.get('active_analysis_workers', 0)}, "
        f"peak workers {session.get('peak_active_analysis_workers', 0)}"
    )
    print(
        f"- worker pool: limit={session.get('analysis_worker_limit', 0)}, "
        f"transport={session.get('analysis_transport_mode', 'unknown')}"
    )
    print(
        f"- state transitions: running={queue_observability.get('saw_running')}, "
        f"queued={queue_observability.get('saw_queued')}, "
        f"success_rate={queue_observability.get('derived_kpis', {}).get('analysis_job_success_rate', 0.0):.2f}"
    )
    print(
        f"- gate: passed={queue_observability.get('gate_passed')}, "
        f"min_depth>={checks.get('min_queue_depth', 0)}, "
        f"min_peak_workers>={checks.get('min_peak_workers', 0)}, "
        f"min_success_rate>={checks.get('min_queue_success_rate', 0.0):.2f}, "
        f"max_failures<={checks.get('max_queue_failures', 0)}"
    )
    if queue_observability.get("gate_failures"):
        for failure in queue_observability["gate_failures"]:
            print(f"  - queue gate failure: {failure}")
else:
    print(f"- skipped: {queue_observability.get('reason', 'unavailable')}")

# Detailed breakdown
print("### Detailed Breakdown\n")
for r in results:
    bl = r["baseline_tokens"]
    cl = r["codelens_tokens"]
    if cl < bl:
        pct = (1 - cl / bl) * 100
        print(f"- **{r['task']}**: {bl:,} → {cl:,} tokens ({pct:.0f}% reduction)")
    else:
        print(f"- **{r['task']}**: {bl:,} → {cl:,} tokens (no reduction)")
    print(f"  - Baseline: {r['baseline_method']}")
    print(f"  - CodeLens: {r['codelens_method']} ({r['codelens_ms']}ms)")

# JSON output for programmatic use
json_out = {
    "project": os.path.basename(PROJECT),
    "total_files": total_files,
    "total_symbols": total_symbols,
    "token_counter": "tiktoken cl100k_base",
    "results": results,
    "workflow_results": workflow_results,
    "queue_observability": queue_observability,
    "totals": {
        "baseline_tokens": total_baseline,
        "codelens_tokens": total_codelens,
        "savings_pct": (
            round((1 - total_codelens / total_baseline) * 100, 1)
            if total_baseline > 0 and total_codelens < total_baseline
            else 0
        ),
    },
}
json_path = os.path.join(os.path.dirname(__file__), "benchmark_results.json")
with open(json_path, "w") as f:
    json.dump(json_out, f, indent=2)
print(f"\nResults saved to {json_path}")

if ARGS.check:
    failures = []
    for result in workflow_results:
        baseline = result["baseline"]
        compressed = result["compressed"]
        if result["savings_pct"] < ARGS.min_workflow_savings:
            failures.append(
                f"{result['scenario']}: savings {result['savings_pct']}% < {ARGS.min_workflow_savings}%"
            )
        baseline_chain = baseline["low_level_chain_count"]
        compressed_chain = compressed["low_level_chain_count"]
        reduction = 100.0 if baseline_chain == 0 else (
            (baseline_chain - compressed_chain) / baseline_chain * 100.0
        )
        if reduction < ARGS.min_chain_reduction:
            failures.append(
                f"{result['scenario']}: low-level chain reduction {reduction:.1f}% < {ARGS.min_chain_reduction}%"
            )
    if ARGS.require_handle_win:
        handle_read_results = []
        for result in workflow_results:
            compressed = result["compressed"]
            if compressed["tool_call_count"] == 1:
                handle_read_results.append(result["scenario"])
        if not handle_read_results:
            failures.append("no single-call compressed workflow found for handle/resource win")
    if queue_observability.get("supported"):
        session = queue_observability.get("session", {})
        checks = queue_observability.get("checks", {})
        if session.get("analysis_queue_max_depth", 0) < ARGS.min_queue_depth:
            failures.append(
                f"queue benchmark did not observe queue depth >= {ARGS.min_queue_depth}"
            )
        if session.get("peak_active_analysis_workers", 0) < ARGS.min_peak_workers:
            failures.append(
                f"queue benchmark did not observe peak workers >= {ARGS.min_peak_workers}"
            )
        if session.get("analysis_jobs_failed", 0) > ARGS.max_queue_failures:
            failures.append(
                f"queue benchmark observed failures {session.get('analysis_jobs_failed', 0)} > {ARGS.max_queue_failures}"
            )
        if (
            queue_observability.get("derived_kpis", {}).get("analysis_job_success_rate", 0.0)
            < ARGS.min_queue_success_rate
        ):
            failures.append(
                "queue benchmark success rate "
                f"{queue_observability.get('derived_kpis', {}).get('analysis_job_success_rate', 0.0):.2f} "
                f"< {ARGS.min_queue_success_rate:.2f}"
            )
        if not queue_observability.get("saw_queued"):
            failures.append("queue benchmark did not observe a queued job state")
        if not queue_observability.get("saw_running"):
            failures.append("queue benchmark did not observe a running job state")
        for failure in queue_observability.get("gate_failures", []):
            if failure not in failures:
                failures.append(f"queue gate: {failure}")
    if failures:
        print("\nBenchmark gate failed:")
        for failure in failures:
            print(f"- {failure}")
        sys.exit(1)
    print(
        "\nBenchmark gate passed: "
        f"workflow savings >= {ARGS.min_workflow_savings}%, "
        f"chain reduction >= {ARGS.min_chain_reduction}%, "
        f"queue depth >= {ARGS.min_queue_depth}, "
        f"peak workers >= {ARGS.min_peak_workers}, "
        f"queue success rate >= {ARGS.min_queue_success_rate:.2f}, "
        f"queue failures <= {ARGS.max_queue_failures}"
    )
