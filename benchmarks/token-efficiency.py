#!/usr/bin/env python3
"""Token efficiency benchmark: CodeLens vs typical agent workflows.

Usage:
    python3 benchmarks/token-efficiency.py [project_path] [--check]

Measures actual token counts using tiktoken (cl100k_base) for both
CodeLens tool output and equivalent agent workflows (search + file reads).
No arbitrary multipliers — only real data.
"""

import argparse, subprocess, json, os, sys, glob

import benchmark_project_context as project_context
import benchmark_runtime_common as runtime_common
import token_efficiency_scenarios as scenario_runner

count_tokens, token_warning = runtime_common.build_token_counter()
if token_warning:
    print(token_warning)
    print("Install with: pip3 install tiktoken\n")


parser = argparse.ArgumentParser()
parser.add_argument("project_path", nargs="?", default=".")
parser.add_argument("--check", action="store_true")
parser.add_argument(
    "--output-json",
    default=os.path.join(os.path.dirname(__file__), "benchmark_results.json"),
)
parser.add_argument("--markdown-output", default="")
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


def codelens(cmd, args, timeout=15, preset=None, profile=None):
    return runtime_common.codelens(BIN, PROJECT, cmd, args, count_tokens, timeout=timeout, preset=preset, profile=profile)


def read_file(path):
    return runtime_common.read_file(PROJECT, path)


def run_search(pattern, include="*.rs", max_lines=50):
    return runtime_common.run_search(PROJECT, pattern, include=include, max_lines=max_lines)


def start_http_daemon(profile=None, preset="full"):
    return runtime_common.start_http_daemon(BIN, PROJECT, profile=profile, preset=preset)


def stop_http_daemon(proc):
    return runtime_common.stop_http_daemon(proc)


def mcp_http_call(
    base_url,
    method,
    params=None,
    request_id=1,
    headers=None,
    include_headers=False,
    timeout_seconds=runtime_common.DEFAULT_HTTP_REQUEST_TIMEOUT_SECONDS,
):
    return runtime_common.mcp_http_call(
        base_url,
        method,
        params=params,
        request_id=request_id,
        headers=headers,
        include_headers=include_headers,
        timeout_seconds=timeout_seconds,
    )


def initialize_http_session(
    base_url,
    profile=None,
    deferred_tool_loading=False,
    trusted_client=None,
    client_name="BenchmarkHarness",
    request_id=1,
    timeout_seconds=runtime_common.DEFAULT_HTTP_BOOTSTRAP_TIMEOUT_SECONDS,
):
    return runtime_common.initialize_http_session(
        base_url,
        profile=profile,
        deferred_tool_loading=deferred_tool_loading,
        trusted_client=trusted_client,
        client_name=client_name,
        request_id=request_id,
        timeout_seconds=timeout_seconds,
    )


def mcp_http_tool_call(
    base_url,
    name,
    arguments,
    request_id=1,
    session_id=None,
    headers=None,
    timeout_seconds=runtime_common.DEFAULT_HTTP_TOOL_TIMEOUT_SECONDS,
):
    return runtime_common.mcp_http_tool_call(
        base_url,
        name,
        arguments,
        request_id=request_id,
        session_id=session_id,
        headers=headers,
        timeout_seconds=timeout_seconds,
    )


def mcp_http_resource_read(
    base_url,
    uri,
    request_id=1,
    session_id=None,
    params=None,
    headers=None,
    timeout_seconds=runtime_common.DEFAULT_HTTP_BOOTSTRAP_TIMEOUT_SECONDS,
):
    return runtime_common.mcp_http_resource_read(
        base_url,
        uri,
        request_id=request_id,
        session_id=session_id,
        params=params,
        headers=headers,
        timeout_seconds=timeout_seconds,
    )


def extract_tool_payload(response):
    return runtime_common.extract_tool_payload(response)

def count_json_tokens(payload):
    return runtime_common.count_json_tokens(payload, count_tokens)


RUNTIME = runtime_common.BenchmarkRuntime(
    codelens=codelens,
    percentile_95=runtime_common.percentile_95,
    start_http_daemon=start_http_daemon,
    stop_http_daemon=stop_http_daemon,
    mcp_http_call=mcp_http_call,
    initialize_http_session=initialize_http_session,
    mcp_http_tool_call=mcp_http_tool_call,
    mcp_http_resource_read=mcp_http_resource_read,
    extract_tool_payload=extract_tool_payload,
    count_json_tokens=count_json_tokens,
    project=PROJECT,
)
QUEUE_THRESHOLDS = scenario_runner.QueueGateThresholds(
    min_queue_depth=ARGS.min_queue_depth,
    min_peak_workers=ARGS.min_peak_workers,
    min_queue_success_rate=ARGS.min_queue_success_rate,
    max_queue_failures=ARGS.max_queue_failures,
)


context = project_context.discover_project_context(PROJECT, codelens)
total_files = context["total_files"]
total_symbols = context["total_symbols"]
primary_ext = context["primary_ext"]
grep_include = context["grep_include"]
test_symbol = context["test_symbol"]
test_file = context["test_file"]
key_file = context["key_file"]
key_files_list = context["key_files_list"]

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
    scenario_runner.compare_workflows(
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
        runtime=RUNTIME,
        low_level_tools=LOW_LEVEL_TOOLS,
    )
)

if key_file:
    workflow_results.append(
        scenario_runner.compare_workflows(
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
            runtime=RUNTIME,
            low_level_tools=LOW_LEVEL_TOOLS,
        )
    )

if test_file:
    workflow_results.append(
        scenario_runner.compare_workflows(
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
            runtime=RUNTIME,
            low_level_tools=LOW_LEVEL_TOOLS,
        )
    )

queue_observability = scenario_runner.run_queue_observability_benchmark(
    RUNTIME,
    QUEUE_THRESHOLDS,
    key_file,
    test_file,
)
watcher_observability = scenario_runner.run_watcher_observability_benchmark(RUNTIME)
quality_contract = scenario_runner.summarize_quality_contract(workflow_results)
verifier_contract = scenario_runner.summarize_verifier_contract(workflow_results)
gate_observability = scenario_runner.run_gate_observability_benchmark(RUNTIME)

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

print("\nWATCHER OBSERVABILITY")
if watcher_observability.get("supported"):
    watcher_session = watcher_observability.get("session", {})
    watcher_status = watcher_observability.get("watch_status", {})
    print(
        f"- running={watcher_session.get('watcher_running')}, "
        f"events={watcher_session.get('watcher_events_processed', 0)}, "
        f"reindexed={watcher_session.get('watcher_files_reindexed', 0)}"
    )
    print(
        f"- contention_batches={watcher_session.get('watcher_lock_contention_batches', 0)}, "
        f"recent_failures={watcher_session.get('watcher_index_failures', 0)}, "
        f"total_failures={watcher_session.get('watcher_index_failures_total', 0)}, "
        f"contention_rate={watcher_observability.get('derived_kpis', {}).get('watcher_lock_contention_rate', 0.0):.4f}"
    )
    print(
        f"- watch_status parity: running={watcher_status.get('running')}, "
        f"contention={watcher_status.get('lock_contention_batches', 0)}, "
        f"recent_failures={watcher_status.get('index_failures', 0)}, "
        f"total_failures={watcher_status.get('index_failures_total', 0)}"
    )
else:
    print(f"- skipped: {watcher_observability.get('reason', 'unavailable')}")

print("\n## Quality Contract Summary\n")
print(
    f"- present_rate={quality_contract.get('quality_contract_present_rate', 0.0):.2f}, "
    f"recommended_checks_total={quality_contract.get('recommended_checks_total', 0)}, "
    f"performance_watchpoints_total={quality_contract.get('performance_watchpoints_total', 0)}"
)
for item in quality_contract.get("scenarios", []):
    print(
        f"  - {item['scenario']}: present={item['has_quality_contract']}, "
        f"quality_focus={item['quality_focus_count']}, "
        f"recommended_checks={item['recommended_check_count']}, "
        f"performance_watchpoints={item['performance_watchpoint_count']}"
    )

print("\n## Verifier Contract Summary\n")
print(
    f"- present_rate={verifier_contract.get('verifier_contract_present_rate', 0.0):.2f}, "
    f"blockers_total={verifier_contract.get('blocker_total', 0)}, "
    f"verifier_checks_total={verifier_contract.get('verifier_checks_total', 0)}, "
    f"blocked_mutation_ready={verifier_contract.get('blocked_mutation_ready_scenarios', 0)}"
)
for item in verifier_contract.get("scenarios", []):
    print(
        f"  - {item['scenario']}: present={item['has_verifier_contract']}, "
        f"blockers={item['blocker_count']}, "
        f"verifier_checks={item['verifier_check_count']}, "
        f"mutation_ready={item['mutation_ready']}, "
        f"test_readiness={item['test_readiness']}"
    )

print("\n## Execution Gate Summary\n")
if gate_observability.get("supported"):
    mutation_gate = gate_observability.get("mutation_gate", {})
    deferred_gate = gate_observability.get("deferred_gate", {})
    mutation_session = mutation_gate.get("session", {})
    mutation_checks = mutation_gate.get("checks", {})
    deferred_session = deferred_gate.get("session", {})
    deferred_checks = deferred_gate.get("checks", {})
    print(
        f"- mutation gate: denies={mutation_session.get('mutation_preflight_gate_denied_count', 0)}, "
        f"caution_count={mutation_session.get('mutation_with_caution_count', 0)}, "
        f"rename_symbol_preflight_denies={mutation_session.get('rename_without_symbol_preflight_count', 0)}, "
        f"deny_rate={mutation_gate.get('derived_kpis', {}).get('mutation_preflight_gate_deny_rate', 0.0):.2f}"
    )
    print(
        f"  - checks: missing_preflight_denied={mutation_checks.get('missing_preflight_denied')}, "
        f"preflight_mutation_allowed={mutation_checks.get('preflight_mutation_allowed')}, "
        f"rename_requires_symbol_preflight={mutation_checks.get('rename_requires_symbol_preflight')}, "
        f"mutation_ready={mutation_checks.get('preflight_mutation_ready', 'unknown')}"
    )
    print(
        f"- deferred gate: expansions={deferred_session.get('deferred_namespace_expansion_count', 0)}, "
        f"hidden_call_denies={deferred_session.get('deferred_hidden_tool_call_denied_count', 0)}, "
        f"deny_rate={deferred_gate.get('derived_kpis', {}).get('deferred_hidden_tool_call_deny_rate', 0.0):.2f}"
    )
    print(
        f"  - checks: hidden_namespace_denied={deferred_checks.get('hidden_namespace_denied')}, "
        f"hidden_tier_denied={deferred_checks.get('hidden_tier_denied')}, "
        f"filesystem_loaded={deferred_checks.get('filesystem_namespace_loaded')}, "
        f"primitive_loaded={deferred_checks.get('primitive_tier_loaded')}"
    )
else:
    print(f"- skipped: {gate_observability.get('reason', 'unavailable')}")

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
    "watcher_observability": watcher_observability,
    "quality_contract": quality_contract,
    "verifier_contract": verifier_contract,
    "gate_observability": gate_observability,
    "project_context": {
        "primary_ext": primary_ext,
        "grep_include": grep_include,
        "test_symbol": test_symbol,
        "test_file": test_file,
        "key_file": key_file,
        "key_files_list": key_files_list,
    },
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
json_path = os.path.abspath(ARGS.output_json)
os.makedirs(os.path.dirname(json_path), exist_ok=True)
with open(json_path, "w") as f:
    json.dump(json_out, f, indent=2)
print(f"\nResults saved to {json_path}")

if ARGS.markdown_output:
    markdown_path = os.path.abspath(ARGS.markdown_output)
    os.makedirs(os.path.dirname(markdown_path), exist_ok=True)
    with open(markdown_path, "w") as f:
        f.write("# CodeLens Token Efficiency Benchmark\n\n")
        f.write(
            f"- Project: {json_out['project']} ({total_files} files, {total_symbols} symbols)\n"
        )
        f.write(
            f"- Token savings: {json_out['totals']['savings_pct']}% "
            f"({total_baseline:,} -> {total_codelens:,})\n"
        )
        if queue_observability.get("supported"):
            session = queue_observability.get("session", {})
            f.write(
                "- Queue: "
                f"max_depth={session.get('analysis_queue_max_depth', 0)}, "
                f"peak_workers={session.get('peak_active_analysis_workers', 0)}, "
                f"success_rate={queue_observability.get('derived_kpis', {}).get('analysis_job_success_rate', 0.0):.2f}\n"
            )
        f.write("\n## Workflows\n\n")
        for result in workflow_results:
            f.write(
                f"- {result['scenario']}: {result['savings_pct']}% savings, "
                f"{result['baseline']['tool_call_count']} -> {result['compressed']['tool_call_count']} calls\n"
            )
    print(f"Markdown summary saved to {markdown_path}")

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
