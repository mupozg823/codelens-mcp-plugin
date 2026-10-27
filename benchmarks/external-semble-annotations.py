#!/usr/bin/env python3
"""File-level NDCG@10 of CodeLens on semble's public annotations.

    python3 benchmarks/external-semble-annotations.py \
        --semble <semble checkout> --binary target/release/codelens-mcp \
        --work /tmp/semble-work --repos tokio,serde,axum --output out.json

Reads semble's annotation and repos.json files as data only (MIT); none of
its code runs. A repo is fetched at the pinned revision, its benchmark_root
is copied to a standalone git root, and CodeLens answers every query once
per method. Symbol results become a de-duplicated file list (first
occurrence wins), scored like semble's file-path baselines: targets are
relevant + secondary, a file matches when either path is a suffix of the
other, NDCG@10 against the ideal for that many targets.
"""

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
from pathlib import Path


def ndcg_at_k(ranks, n_relevant, k=10):
    if n_relevant == 0:
        return 0.0
    rel = [0] * k
    for r in ranks:
        if 1 <= r <= k:
            rel[r - 1] = 1
    dcg = sum(x / math.log2(i + 2) for i, x in enumerate(rel))
    ideal = sum(1 / math.log2(i + 2) for i in range(min(k, n_relevant)))
    return dcg / ideal if ideal else 0.0


def path_matches(file_path, target):
    a, b = file_path.replace("\\", "/"), target.replace("\\", "/")
    return a == b or a.endswith("/" + b) or b.endswith("/" + a)


def files_from(method, payload):
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
    if method == "semantic_search":
        rows = [r.get("file_path") for r in data.get("results", [])]
    else:
        rows = [r.get("file") for r in data.get("symbols", [])]
    out = []
    for f in rows:
        if f and f not in out:
            out.append(f)
    return out


def cache_dirs(repo, work):
    """Checkout and root directories keyed by name, pinned revision and
    benchmark_root, so a changed pin never scores against an old copy."""
    pinned = f"{repo['name']}@{repo['revision'][:12]}"
    root_slug = (repo.get("benchmark_root") or ".").replace("/", "_")
    return work / "checkouts" / pinned, work / "roots" / f"{pinned}-{root_slug}"


def failed_calls(batch, payloads, returncode):
    """Calls that did not succeed. A failed call has no ranking to score:
    counting it as an empty result would turn an infrastructure failure
    into a retrieval miss."""
    failed = [
        f"#{i} {call['name']}: {payload.get('error') or 'success=false'}"
        for i, (call, payload) in enumerate(zip(batch, payloads))
        if payload.get("success") is not True
    ]
    if returncode != 0 and not failed:
        failed.append(f"batch exited {returncode}")
    return failed


def checkout(repo, work):
    dest, root = cache_dirs(repo, work)
    if not dest.exists():
        dest.mkdir(parents=True)
        run = lambda *a: subprocess.run(a, cwd=dest, check=True, capture_output=True)
        run("git", "init", "-q")
        run("git", "remote", "add", "origin", repo["url"])
        run("git", "fetch", "-q", "--depth", "1", "origin", repo["revision"])
        run("git", "checkout", "-q", "FETCH_HEAD")
    if not root.exists():
        src = dest / repo["benchmark_root"] if repo.get("benchmark_root") else dest
        shutil.copytree(src, root, ignore=shutil.ignore_patterns(".git"))
        subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    return root


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--semble", required=True, help="semble checkout (data only)")
    ap.add_argument("--binary", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--repos", default="tokio,serde,axum")
    ap.add_argument("--output", required=True)
    ap.add_argument("--max-results", type=int, default=50)
    args = ap.parse_args()
    work = Path(args.work)
    repos = {r["name"]: r for r in json.load(open(Path(args.semble) / "benchmarks/repos.json"))}
    report = {"binary": args.binary, "model_dir": os.environ.get("CODELENS_MODEL_DIR"), "repos": []}
    for name in args.repos.split(","):
        repo = repos[name]
        tasks = json.load(open(Path(args.semble) / f"benchmarks/annotations/{name}.json"))
        root = checkout(repo, work)
        methods = {
            "get_ranked_context": lambda q: {"query": q, "max_tokens": 50000, "include_body": False},
            "semantic_search": lambda q: {"query": q, "max_results": args.max_results},
            "get_ranked_context_no_semantic": lambda q: {"query": q, "max_tokens": 50000, "include_body": False, "disable_semantic": True},
        }
        batch = [{"name": "index_embeddings", "arguments": {"background": False}}]
        order = []
        for method, build in methods.items():
            tool = "semantic_search" if method == "semantic_search" else "get_ranked_context"
            for t in tasks:
                batch.append({"name": tool, "arguments": build(t["query"])})
                order.append((method, t))
        proc = subprocess.run(
            [args.binary, str(root), "--preset", "balanced", "--batch", json.dumps(batch)],
            capture_output=True, text=True, timeout=3600,
        )
        payloads = json.loads(proc.stdout)
        if not isinstance(payloads, list) or len(payloads) != len(batch):
            sys.exit(f"{name}: bad batch output ({proc.returncode}): {proc.stderr[-2000:]}")
        failed = failed_calls(batch, payloads, proc.returncode)
        if failed:
            sys.exit(f"{name}: {len(failed)} failed call(s), not scoring: " + "; ".join(failed[:5]))
        index_ok = payloads[0].get("success")
        per_method = {}
        rows = []
        for (method, t), payload in zip(order, payloads[1:]):
            targets = list(t.get("relevant", [])) + list(t.get("secondary", []))
            files = files_from(method, payload)[:10]
            ranks = []
            for tgt in targets:
                r = next((i for i, f in enumerate(files, 1) if path_matches(f, tgt)), None)
                if r is not None:
                    ranks.append(r)
            score = ndcg_at_k(ranks, len(targets))
            per_method.setdefault(method, []).append(score)
            rows.append({"method": method, "query": t["query"], "category": t.get("category"),
                         "ndcg10": round(score, 4), "ranks": ranks, "n_targets": len(targets),
                         "ok": payload.get("success"), "top_files": files[:5]})
        summary = {m: round(sum(v) / len(v), 4) for m, v in per_method.items()}
        report["repos"].append({"repo": name, "revision": repo["revision"], "queries": len(tasks),
                                "index_ok": index_ok, "ndcg10": summary, "rows": rows})
        print(name, "index_ok", index_ok, summary, flush=True)
    json.dump(report, open(args.output, "w"), indent=1)


if __name__ == "__main__":
    main()
