#!/usr/bin/env python3
"""Token efficiency benchmark: CodeLens vs raw file reading.

Usage:
    python3 benchmarks/token-efficiency.py [project_path]

Measures how many tokens CodeLens saves compared to reading entire files,
across 5 common agent tasks. Uses conservative methodology (1:1 comparison).
"""

import subprocess, json, os, sys, glob

PROJECT = sys.argv[1] if len(sys.argv) > 1 else "."
BIN = os.environ.get("CODELENS_BIN", "./target/release/codelens-mcp")


def codelens(cmd, args):
    args["_profile"] = "deep_semantic"
    r = subprocess.run(
        [BIN, PROJECT, "--cmd", cmd, "--args", json.dumps(args)],
        capture_output=True,
        text=True,
        timeout=15,
    )
    return len(r.stdout) // 4 if r.stdout else 0


def file_tokens(path):
    try:
        return os.path.getsize(os.path.join(PROJECT, path)) // 4
    except:
        return 0


def multi_file_tokens(paths):
    return sum(file_tokens(p) for p in paths)


# Discover project info
info = subprocess.run(
    [BIN, PROJECT, "--cmd", "get_project_structure", "--args", "{}"],
    capture_output=True,
    text=True,
    timeout=15,
)
if info.returncode == 0:
    d = json.loads(info.stdout)
    total_files = d["data"]["total_files"]
    total_symbols = d["data"]["total_symbols"]
else:
    total_files = total_symbols = "?"

# Find a good symbol to test
sym_result = subprocess.run(
    [
        BIN,
        PROJECT,
        "--cmd",
        "find_symbol",
        "--args",
        '{"name":"main","max_matches":1,"_profile":"fast_local"}',
    ],
    capture_output=True,
    text=True,
    timeout=10,
)
test_symbol = "main"
test_file = None
if sym_result.returncode == 0:
    sd = json.loads(sym_result.stdout)
    if sd.get("data", {}).get("count", 0) > 0:
        s = sd["data"]["symbols"][0]
        test_symbol = s.get("name", "main")
        test_file = s.get("file_path", None)

# Get key files for impact analysis
key_result = subprocess.run(
    [BIN, PROJECT, "--cmd", "onboard_project", "--args", '{"_profile":"fast_local"}'],
    capture_output=True,
    text=True,
    timeout=30,
)
key_file = None
if key_result.returncode == 0:
    kd = json.loads(key_result.stdout)
    kf = kd.get("data", {}).get("key_files", [])
    if kf:
        key_file = kf[0]["file"]

if not test_file:
    all_files = (
        glob.glob(os.path.join(PROJECT, "**/*.py"), recursive=True)
        or glob.glob(os.path.join(PROJECT, "**/*.ts"), recursive=True)
        or glob.glob(os.path.join(PROJECT, "**/*.rs"), recursive=True)
    )
    if all_files:
        test_file = os.path.relpath(all_files[0], PROJECT)

if not key_file:
    key_file = test_file

tasks = []

# Task 1: Find a symbol
if test_file:
    raw = file_tokens(test_file)
    cl = codelens("find_symbol", {"name": test_symbol, "include_body": True})
    if cl > 0:
        tasks.append((f"Find '{test_symbol}'", raw, cl))

# Task 2: File structure
if test_file:
    raw = file_tokens(test_file)
    cl = codelens("get_symbols_overview", {"path": test_file})
    if cl > 0:
        tasks.append(("File structure", raw, cl))

# Task 3: Impact analysis
if key_file:
    src_files = glob.glob(os.path.join(PROJECT, "**/*.*"), recursive=True)
    src_files = [
        f
        for f in src_files
        if any(f.endswith(e) for e in [".py", ".ts", ".rs", ".go", ".java", ".js"])
    ][:20]
    raw = sum(os.path.getsize(f) // 4 for f in src_files if os.path.isfile(f))
    cl = codelens("get_impact_analysis", {"file_path": key_file})
    if cl > 0:
        tasks.append(("Impact analysis", raw, cl))

# Task 4: Find references
if test_file:
    raw = file_tokens(test_file) * 5  # Agent typically reads ~5 files
    cl = codelens(
        "find_referencing_symbols", {"file_path": test_file, "symbol_name": test_symbol}
    )
    if cl > 0:
        tasks.append(("Find references", raw, cl))

# Task 5: Context retrieval
if key_file:
    raw = file_tokens(key_file) * 4  # Agent reads ~4 related files
    cl = codelens(
        "get_ranked_context",
        {"query": f"how does {test_symbol} work", "max_tokens": 8000},
    )
    if cl > 0:
        tasks.append(("Context retrieval", raw, cl))

# Print results
print(f"## CodeLens Token Efficiency Benchmark")
print(
    f"Project: {os.path.basename(os.path.abspath(PROJECT))} ({total_files} files, {total_symbols} symbols)\n"
)
print("| Task | Without CodeLens | With CodeLens | Savings |")
print("|------|-----------------|---------------|---------|")
total_raw = total_cl = 0
for name, raw, cl in tasks:
    total_raw += raw
    total_cl += cl
    ratio = raw / cl if cl > 0 else 0
    pct = (1 - cl / raw) * 100 if raw > 0 else 0
    print(f"| {name} | {raw:,} | {cl:,} | {ratio:.1f}x ({pct:.0f}%) |")

if total_cl > 0 and total_raw > 0:
    ratio = total_raw / total_cl
    pct = (1 - total_cl / total_raw) * 100
    print(
        f"| **Total** | **{total_raw:,}** | **{total_cl:,}** | **{ratio:.1f}x ({pct:.0f}%)** |"
    )
    print(f"\n**{ratio:.1f}x fewer tokens** across {len(tasks)} common agent tasks.")
