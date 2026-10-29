#!/usr/bin/env python3
"""Time analysis-lite stdio calls on a real working tree and compare builds.

Earlier latency checks ran on `git archive` snapshots, which drop untracked
files. A working tree with untracked 2.4 MB bundles stalled the installed
server for over 300 s while every snapshot measurement looked fine (#471), so
this runner calls the server on the checkout as it is, or on a generated
project with one deeply nested bundle (`--synthetic-bundle`).

Each binary gets one stdio session; targets run in order (the first is the
cold call). With two or more binaries the normalized results are compared
byte for byte and a mismatch exits 1. Normalization drops only fields that
change between identical runs: `generation`, `freshness`, `watcher_running`.

Usage:
  python3 benchmarks/analysis-lite-latency.py --bin ~/.local/bin/codelens-lite \\
      --target /abs/repo/src/a.ts@name[@refs|impact] [--target ...] [--json out.json]
  python3 benchmarks/analysis-lite-latency.py --bin OLD --bin NEW --synthetic-bundle
"""

import argparse
import json
import os
import re
import select
import subprocess
import sys
import tempfile
import time
from pathlib import Path

VOLATILE_KEYS = frozenset({"generation", "freshness", "watcher_running"})


def parse_target(spec):
    """`file@name[@mode]` -> (file, name, mode); mode is refs or impact."""
    parts = spec.split("@")
    if len(parts) not in (2, 3) or not parts[0] or not parts[1]:
        raise ValueError(f"target must be file@name[@refs|impact]: {spec!r}")
    mode = parts[2] if len(parts) == 3 else "refs"
    if mode not in ("refs", "impact"):
        raise ValueError(f"mode must be refs or impact: {spec!r}")
    return parts[0], parts[1], mode


def normalize(value):
    if isinstance(value, dict):
        return {k: normalize(v) for k, v in value.items() if k not in VOLATILE_KEYS}
    if isinstance(value, list):
        return [normalize(v) for v in value]
    return value


def write_synthetic_project(root, depth=250, modules=40):
    """A project whose bundle nests `depth` callbacks per module, the shape
    that made per-call parent walks cost depth squared. Returns targets."""
    root = Path(root)
    (root / "src").mkdir(parents=True, exist_ok=True)
    (root / "src" / "api.ts").write_text(
        "export function track(event: string) { return event.length; }\n",
        encoding="utf-8",
    )
    (root / "src" / "use.ts").write_text(
        'import { track } from "./api";\nexport function page() { track("view"); }\n',
        encoding="utf-8",
    )
    lines = ['"use strict";', "(() => {"]
    for module in range(modules):
        lines.append(f"  var m{module} = (exports) => {{")
        lines.extend("    f(() => {" for _ in range(depth))
        lines.extend("    });" for _ in range(depth))
        lines.append("  };")
    lines.append("})();")
    # Not under dist/ or build/: those are excluded from discovery. The
    # bundles that stalled the server lived in an untracked evidence folder.
    (root / "evidence").mkdir(exist_ok=True)
    (root / "evidence" / "bundle.js").write_text("\n".join(lines) + "\n", encoding="utf-8")
    api = str(root / "src" / "api.ts")
    return [f"{api}@track@refs", f"{api}@track@impact"]


def footprint(pid):
    try:
        out = subprocess.run(
            ["footprint", str(pid)], capture_output=True, text=True, timeout=30
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    match = re.search(r"Footprint: ([\d.]+ [KMG]B)", out)
    return match.group(1) if match else None


class Session:
    def __init__(self, binary):
        self.proc = subprocess.Popen(
            [binary, "--profile", "analysis-lite", "--daemon-mode", "read-only"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        self.next_id = 1

    def request(self, method, params, timeout):
        request_id = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            ready, _, _ = select.select([self.proc.stdout], [], [], max(0.0, remaining))
            if not ready:
                return None
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("server closed stdout")
            message = json.loads(line)
            if message.get("id") == request_id:
                return message

    def send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def close(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def run_binary(binary, targets, timeout):
    session = Session(binary)
    rows = []
    try:
        session.request(
            "initialize",
            {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "analysis-lite-latency", "version": "1"},
            },
            60,
        )
        session.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        for spec in targets:
            file, name, mode = parse_target(spec)
            started = time.monotonic()
            reply = session.request(
                "tools/call",
                {
                    "name": "search" if mode == "refs" else "graph",
                    "arguments": {"mode": mode, "target": {"file": file, "name": name}},
                },
                timeout,
            )
            elapsed = time.monotonic() - started
            row = {"target": spec, "seconds": round(elapsed, 2), "footprint": footprint(session.proc.pid)}
            if reply is None:
                row["status"] = "timeout"
                rows.append(row)
                break
            content = (reply.get("result") or {}).get("structuredContent") or reply.get("error") or {}
            results = content.get("results") or {}
            row.update(
                status=content.get("status"),
                references=len(results.get("references") or []),
                edges=len(results.get("edges") or []),
                result=normalize(content),
            )
            rows.append(row)
    finally:
        session.close()
    return rows


def compare(runs):
    """Targets whose normalized results differ between the first binary and any other."""
    first = {row["target"]: row.get("result") for row in runs[0]["rows"]}
    differing = []
    for run in runs[1:]:
        for row in run["rows"]:
            if json.dumps(row.get("result"), sort_keys=True) != json.dumps(
                first.get(row["target"]), sort_keys=True
            ):
                differing.append({"binary": run["binary"], "target": row["target"]})
    return differing


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--bin", action="append", required=True, help="analysis-lite binary; repeat to compare")
    parser.add_argument("--target", action="append", default=[], help="file@name[@refs|impact], absolute file")
    parser.add_argument("--synthetic-bundle", action="store_true", help="add a generated nested-bundle project")
    parser.add_argument("--timeout", type=float, default=300.0, help="seconds per call")
    parser.add_argument("--json", help="write rows and the comparison here")
    args = parser.parse_args(argv)
    targets = list(args.target)
    scratch = None
    if args.synthetic_bundle:
        scratch = tempfile.TemporaryDirectory(prefix="analysis-lite-bundle-")
        subprocess.run(["git", "init", "-q", scratch.name], check=True)
        targets += write_synthetic_project(scratch.name)
    if not targets:
        parser.error("give --target or --synthetic-bundle")
    for spec in targets:
        parse_target(spec)
    runs = []
    for binary in args.bin:
        binary = os.path.expanduser(binary)
        rows = run_binary(binary, targets, args.timeout)
        runs.append({"binary": binary, "rows": rows})
        for row in rows:
            print(
                f"{Path(binary).parent.name}/{Path(binary).name}  {row['target'].rsplit('/', 1)[-1]}  "
                f"{row['seconds']}s  {row.get('status')}  refs={row.get('references')} "
                f"edges={row.get('edges')}  fp={row.get('footprint')}",
                flush=True,
            )
    differing = compare(runs) if len(runs) > 1 else []
    if len(runs) > 1:
        print("identical" if not differing else f"DIFFERENT: {differing}")
    if args.json:
        Path(args.json).write_text(
            json.dumps({"runs": runs, "differing": differing}, indent=1, ensure_ascii=False),
            encoding="utf-8",
        )
    if scratch:
        scratch.cleanup()
    timed_out = any(row.get("status") == "timeout" for run in runs for row in run["rows"])
    return 1 if differing or timed_out else 0


if __name__ == "__main__":
    sys.exit(main())
