#!/usr/bin/env python3
"""Deprecated heuristic external benchmark.

Do not use this script as promotion evidence. It relies on keyword-hit heuristics,
not exact expected-symbol labels. Use `benchmarks/external-retrieval.py` instead.
"""

import json
import subprocess
import time

BIN = "/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp"

PROJECTS = {
    "claw-dev (TS/JS)": {
        "path": "/Users/bagjaeseog/Downloads/claudex/claw-dev",
        "queries": [
            {
                "query": "agent configuration and setup",
                "keywords": ["agent", "config", "configure", "setup"],
            },
            {
                "query": "send request to anthropic API",
                "keywords": ["send", "request", "anthropic", "api"],
            },
            {
                "query": "handle user input from command line",
                "keywords": ["input", "handle", "command", "cli", "auth"],
            },
            {"query": "define available tools", "keywords": ["tool", "list", "define"]},
            {
                "query": "process streaming response",
                "keywords": ["stream", "sse", "response", "process"],
            },
            {
                "query": "error handling and retry logic",
                "keywords": ["error", "handler", "retry", "handle"],
            },
            {
                "query": "parse and validate configuration",
                "keywords": ["parse", "config", "valid"],
            },
            {
                "query": "render terminal output",
                "keywords": ["render", "terminal", "output", "print"],
            },
            {
                "query": "manage authentication tokens",
                "keywords": ["auth", "token", "credential", "login"],
            },
            {
                "query": "execute shell command in sandbox",
                "keywords": ["shell", "command", "bash", "sandbox", "exec"],
            },
        ],
    },
    "Flask (Python)": {
        "path": "/tmp/flask-test",
        "queries": [
            {
                "query": "handle HTTP request routing",
                "keywords": ["handle", "http", "route", "request"],
            },
            {
                "query": "render template with context",
                "keywords": ["render", "template"],
            },
            {
                "query": "register blueprint for modular app",
                "keywords": ["register", "blueprint"],
            },
            {
                "query": "parse JSON request body",
                "keywords": ["json", "request", "parse"],
            },
            {
                "query": "handle application errors",
                "keywords": ["error", "handler", "exception"],
            },
            {"query": "create test client for testing", "keywords": ["test", "client"]},
            {"query": "manage session and cookies", "keywords": ["session", "cookie"]},
            {"query": "configure logging", "keywords": ["log", "config"]},
            {
                "query": "send HTTP response with status code",
                "keywords": ["response", "status", "send", "make"],
            },
            {
                "query": "serve static files",
                "keywords": ["static", "file", "send", "serve"],
            },
        ],
    },
    "curl (C)": {
        "path": "/tmp/curl-test",
        "queries": [
            {
                "query": "establish connection to remote server",
                "keywords": ["connect", "connection", "remote"],
            },
            {"query": "parse URL into components", "keywords": ["parse", "url", "uri"]},
            {
                "query": "handle SSL certificate verification",
                "keywords": ["ssl", "tls", "cert", "verify"],
            },
            {
                "query": "send HTTP POST request with data",
                "keywords": ["post", "send", "request", "http"],
            },
            {
                "query": "handle authentication credentials",
                "keywords": ["auth", "credential", "user", "password"],
            },
            {
                "query": "set transfer options and configuration",
                "keywords": ["set", "opt", "config", "transfer"],
            },
            {
                "query": "read response headers",
                "keywords": ["header", "response", "read"],
            },
            {
                "query": "handle timeout and retry",
                "keywords": ["timeout", "retry", "expire"],
            },
            {
                "query": "resolve DNS hostname",
                "keywords": ["dns", "resolve", "host", "name"],
            },
            {
                "query": "manage cookies for requests",
                "keywords": ["cookie", "jar", "manage"],
            },
        ],
    },
    "rg-family (Next.js)": {
        "path": "/Users/bagjaeseog/Projects/rg-family-clone",
        "queries": [
            {
                "query": "user authentication and login",
                "keywords": ["auth", "login", "sign", "user"],
            },
            {
                "query": "fetch data from API endpoint",
                "keywords": ["fetch", "api", "get", "data", "query"],
            },
            {
                "query": "render page component",
                "keywords": ["page", "render", "component"],
            },
            {
                "query": "handle form submission",
                "keywords": ["form", "submit", "handle"],
            },
            {
                "query": "database query and connection",
                "keywords": ["db", "database", "query", "prisma", "sql"],
            },
            {
                "query": "manage application state",
                "keywords": ["state", "store", "context", "provider"],
            },
            {
                "query": "style component with CSS",
                "keywords": ["style", "css", "class", "theme"],
            },
            {
                "query": "route navigation and middleware",
                "keywords": ["route", "middleware", "navigation", "next"],
            },
            {"query": "upload file or image", "keywords": ["upload", "file", "image"]},
            {
                "query": "send notification or email",
                "keywords": ["notification", "email", "send", "message"],
            },
        ],
    },
}


def run_tool(project, cmd, args, timeout=300):
    argv = [
        BIN,
        project,
        "--preset",
        "balanced",
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        if result.returncode != 0:
            return None
        output = result.stdout.strip()
        if output:
            lines = output.strip().split("\n")
            return json.loads(lines[-1])
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        pass
    return None


def main():
    results = {}

    for proj_name, proj_info in PROJECTS.items():
        path = proj_info["path"]
        queries = proj_info["queries"]
        print(f"\n{'='*60}")
        print(f"  {proj_name} — {path}")
        print(f"{'='*60}")

        # Index
        print("  Indexing...")
        t0 = time.time()
        idx = run_tool(path, "index_embeddings", {})
        idx_time = time.time() - t0
        if not idx or not idx.get("success"):
            print(f"  ✗ Indexing failed")
            results[proj_name] = {"error": "indexing failed"}
            continue
        indexed = idx["data"].get("indexed_symbols", 0)
        print(f"  ✓ {indexed} symbols indexed ({idx_time:.1f}s)")

        # Search
        hits = 0
        total = len(queries)
        query_results = []

        for q in queries:
            result = run_tool(
                path, "semantic_search", {"query": q["query"], "max_results": 5}
            )
            if not result or not result.get("success"):
                query_results.append(
                    {"query": q["query"], "hit": False, "reason": "failed"}
                )
                continue

            matches = result.get("data", {}).get("results", [])
            hit = False
            matched = ""
            score = 0
            for m in matches:
                name = str(m.get("symbol_name", "") or m.get("name", "")).lower()
                file_path = str(m.get("file_path", "") or m.get("file", "")).lower()
                combined = name + " " + file_path
                if any(kw.lower() in combined for kw in q["keywords"]):
                    hit = True
                    matched = m.get("symbol_name", "") or m.get("name", "")
                    score = m.get("score", 0)
                    break

            if hit:
                hits += 1
            top3 = [m.get("symbol_name", "") or m.get("name", "") for m in matches[:3]]
            query_results.append(
                {
                    "query": q["query"],
                    "hit": hit,
                    "matched": matched,
                    "score": round(score, 3),
                    "top3": top3,
                }
            )

        # Print results
        accuracy = hits / total
        print(f"\n  Results: {hits}/{total} ({accuracy*100:.0f}%)")
        for qr in query_results:
            status = "✓" if qr["hit"] else "✗"
            matched = f" → {qr['matched']} ({qr['score']})" if qr.get("matched") else ""
            print(f"    {status} {qr['query'][:45]}{matched}")
            if not qr["hit"] and qr.get("top3"):
                print(f"      got: {', '.join(qr['top3'][:3])}")

        results[proj_name] = {
            "indexed": indexed,
            "index_time_s": round(idx_time, 1),
            "accuracy": accuracy,
            "hits": hits,
            "total": total,
            "queries": query_results,
        }

    # Summary
    print(f"\n{'='*60}")
    print("  SUMMARY")
    print(f"{'='*60}")
    for name, r in results.items():
        if "error" in r:
            print(f"  {name}: ERROR")
        else:
            print(
                f"  {name}: {r['hits']}/{r['total']} ({r['accuracy']*100:.0f}%) — {r['indexed']} symbols"
            )


if __name__ == "__main__":
    main()
