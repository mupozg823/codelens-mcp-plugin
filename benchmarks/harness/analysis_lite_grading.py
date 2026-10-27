"""Independent gold comparison and post-run E2 acceptance; never grades telemetry."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

from analysis_lite_snapshots import source_hashes


def run_owned(command, *, timeout, cwd=None, env=None, input=None):
    """Bounded child execution; terminate only the new process group we own."""
    started = time.monotonic()
    process = subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True,
        stdin=subprocess.PIPE if input is not None else subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    timed_out, signals, cleanup_error = False, [], None
    try:
        stdout, stderr = process.communicate(input.encode() if input is not None else None, timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        stdout = stderr = b""
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(process.pid, sig)
                signals.append(sig.name)
            except ProcessLookupError:
                pass
            try:
                stdout, stderr = process.communicate(timeout=2)
                break
            except subprocess.TimeoutExpired as exc:
                stdout, stderr = exc.output or b"", exc.stderr or b""
        else:
            cleanup_error = "group output pipes did not close after TERM/KILL"
            process.stdout.close(); process.stderr.close()
            process.wait(timeout=2)
        try:
            os.killpg(process.pid, signal.SIGKILL)
            signals.append("SIGKILL")
        except ProcessLookupError:
            pass
    return {"command": command, "pid": process.pid, "process_group": process.pid,
            "returncode": process.returncode, "timed_out": timed_out,
            "stdout": stdout.decode("utf-8", errors="replace"), "stderr": stderr.decode("utf-8", errors="replace"),
            "stdout_base64": base64.b64encode(stdout).decode(), "stderr_base64": base64.b64encode(stderr).decode(),
            "cleanup_signals": signals, "cleanup_error": cleanup_error,
            "leader_reaped": process.returncode is not None, "elapsed_seconds": time.monotonic() - started}


def grade_answer(task, answer):
    """Mechanical exactness only. Semantic approval always remains independent."""
    report = {"raw_answer": answer, "status": "manual-review", "mechanical_status": None,
              "task_quality": None, "semantic_review_required": True,
              "rubric": task["gold"].get("acceptance", []), "mismatches": []}
    try:
        parsed = json.loads(answer) if isinstance(answer, str) else answer
        if not isinstance(parsed, dict):
            raise TypeError("answer is not a structured object")
        gold = task["gold"]; actual = parsed["declaration"]; expected = gold["declaration"]
        if not isinstance(actual, dict) or not all(isinstance(actual.get(k), str) for k in ("file", "symbol")):
            raise ValueError("declaration identity unavailable")
        correct = all(actual[k] == expected[k] for k in ("file", "symbol"))
        if "line" in expected:
            if type(actual.get("line")) is int:
                correct = correct and actual["line"] == expected["line"]
            elif all(type(actual.get(k)) is int for k in ("byte_start", "byte_end")) and "byte_start" in expected:
                correct = correct and all(actual[k] == expected[k] for k in ("byte_start", "byte_end"))
            else:
                raise ValueError("declaration location unavailable")
        if not correct:
            report["mismatches"].append("wrong-declaration")
        if any(actual.get(k) is not None for k in ("byte_start", "byte_end")):
            if not all(type(actual.get(k)) is int for k in ("byte_start", "byte_end")):
                raise ValueError("incomplete declaration byte identity")
            if "byte_start" in expected and any(actual[k] != expected[k] for k in ("byte_start", "byte_end")):
                report["mismatches"].append("wrong-declaration-byte-identity")

        def locations(rows, *, edges=False, non_calls=False):
            if not isinstance(rows, list):
                raise TypeError("location collection unavailable")
            found = set()
            for row in rows:
                if not isinstance(row, dict) or not isinstance(row.get("file"), str) or row["file"].startswith("/"):
                    raise ValueError("relative source location unavailable")
                lines = row.get("lines", [row.get("line")])
                if not isinstance(lines, list) or not lines or any(type(n) is not int or n < 1 for n in lines):
                    raise ValueError("source line unavailable")
                extra = (row["caller"], row["callee"], row["depth"]) if edges else ()
                if edges and (not all(isinstance(row[k], str) for k in ("caller", "callee")) or type(row["depth"]) is not int):
                    raise ValueError("edge identity unavailable")
                if non_calls:
                    if not isinstance(row.get("kind"), str) or not row["kind"]:
                        raise ValueError("non-call relation unavailable")
                    extra = (row["kind"].split()[0],)
                found.update((row["file"], n, *extra) for n in lines)
            return found

        if "direct_calls" in gold:
            wanted, got = locations(gold["direct_calls"]), locations(parsed["calls"])
            report.update(missing_calls=sorted(wanted - got), extra_calls=sorted(got - wanted))
            if wanted != got:
                report["mismatches"].append("callsite-set")
        if "non_calls" in gold and locations(gold["non_calls"], non_calls=True) != locations(parsed["non_calls"], non_calls=True):
            report["mismatches"].append("non-call-set")
        if "reverse_edges" in gold:
            impact = parsed["impact"]
            if not isinstance(impact, dict) or not isinstance(impact.get("bounds"), dict):
                raise ValueError("impact bounds unavailable")
            if not isinstance(impact.get("nodes"), list) or not all(isinstance(n, str) for n in impact["nodes"]):
                raise ValueError("impact node identities unavailable")
            wanted = locations(gold["reverse_edges"], edges=True)
            got = locations(impact["edges"], edges=True)
            nodes = {expected["symbol"], *(row["caller"] for row in gold["reverse_edges"])}
            if got != wanted or set(impact["nodes"]) != nodes:
                report["mismatches"].append("bounded-impact-set")
            if any(impact["bounds"].get(k) != task["limits"][k] for k in ("reverse_depth", "node_cap", "edge_cap")):
                report["mismatches"].append("impact-bounds")
        report["mechanical_status"] = "fail" if report["mismatches"] else "pass"
        if report["mismatches"]:
            report["status"] = "fail"
    except (ValueError, TypeError, KeyError) as exc:
        report["review_reason"] = str(exc)
    return report


ORACLE_JS = r"""
const fs=require('node:fs'), vm=require('node:vm');
const ts=require(process.argv[2]+'/node_modules/typescript');
const source=fs.readFileSync(process.argv[1],'utf8');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const box={exports:{}};vm.runInNewContext(code,box,{timeout:2000});
const cases=JSON.parse(fs.readFileSync(0,'utf8')), errors=[];
for(const row of cases) {const actual=box.exports.formatCrewValue(...row.args);if(actual!==row.expected)errors.push({args:row.args,expected:row.expected,actual});}
let misses=0;
for(let n=0;n<=100000;n++){const value=n/100;if(box.exports.formatCrewValue(value,'contribution',2)!==value.toFixed(2))misses++;}
console.log(JSON.stringify({passed:errors.length===0&&misses===0,literal_errors:errors,exhaustive_cases:100001,exhaustive_failures:misses}));
"""


def run_oracle(task, source, dependencies, timeout=30):
    started = time.monotonic()
    receipt = None
    try:
        receipt = run_owned(["node", "-e", ORACLE_JS, str(Path(source).resolve()), str(Path(dependencies).resolve())],
            input=json.dumps(task["gold"]["acceptance_cases"]), timeout=timeout, env={"PATH": os.environ["PATH"]})
        receipt["result"] = json.loads(receipt["stdout"]) if receipt["returncode"] == 0 and not receipt["timed_out"] else None
        return receipt
    except (ValueError, OSError) as exc:
        report = receipt if receipt is not None else {"returncode": None, "elapsed_seconds": time.monotonic() - started}
        report.update(error=str(exc), result=None)
        return report


def grade_edit(task, baseline, final, dependencies, replay_directory, *, eval_root, timeout=120):
    """Post-agent only: replay its own new test against base, then final source.

    Caller supplies isolated frozen sources and task-local installed dependencies.
    No test/gold is injected before agent execution. npm output is retained raw;
    exit!=0 on base is not alone proof of the intended assertion (manual review).
    """
    baseline, final, dependencies, replay = map(Path, (baseline, final, dependencies, replay_directory))
    root = Path(eval_root).resolve()
    if not all(path.resolve().is_relative_to(root) for path in (baseline, final, dependencies, replay)):
        raise ValueError("grading inputs must remain within the task-owned evaluation root")
    if not (dependencies / "node_modules").resolve().is_relative_to(dependencies.resolve()):
        raise ValueError("task dependencies cannot escape their setup directory")
    for name in ("package.json", "package-lock.json"):
        if (baseline / name).read_bytes() != (dependencies / name).read_bytes():
            raise ValueError("dependencies differ from the frozen baseline manifest/lock")
    env = {"PATH": os.environ["PATH"], "NPM_CONFIG_CACHE": str(root / "npm-cache")}
    for scope in ("USER", "GLOBAL"):
        empty = dependencies / f"empty-{scope.lower()}.npmrc"
        if empty.is_symlink() or (empty.exists() and empty.stat().st_size):
            raise ValueError("task-local npm config must be empty")
        empty.touch()
        env[f"NPM_CONFIG_{scope}CONFIG"] = str(empty)
    before, after = source_hashes(baseline), source_hashes(final)
    changed = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
    allowed = task["limits"]["changed_paths"]
    report = {"status": "manual-review", "task_quality": None, "changed_files": changed,
              "semantic_review_required": True, "base_red": None, "final_green": None,
              "dependency_source": str(dependencies), "dependency_delivery": []}
    if set(changed) != set(allowed) or not (final / allowed[1]).is_file():
        report.update(status="fail", reason="edits outside allowed files or missing actual regression test")
        return report
    shutil.copytree(baseline, replay, ignore=shutil.ignore_patterns("node_modules", ".codelens"))
    (replay / allowed[1]).write_bytes((final / allowed[1]).read_bytes())
    for label, directory in (("base_red", replay), ("final_green", final)):
        modules = directory / "node_modules"
        if not modules.exists():
            started = time.monotonic()
            method = "copy"
            if sys.platform == "darwin":
                copied = subprocess.run(["cp", "-cR", str(dependencies / "node_modules"), str(modules)],
                                        capture_output=True, text=True, check=False)
                if copied.returncode:
                    report.update(status="error", error=copied.stderr, dependency_copy_exit=copied.returncode)
                    return report
                method = "APFS-clone"
            else:
                shutil.copytree(dependencies / "node_modules", modules, symlinks=True)
            for cache in (".vite", ".vite-temp", ".cache"):
                path = modules / cache
                if path.is_symlink():
                    path.unlink()
                elif path.is_dir():
                    shutil.rmtree(path)
            report["dependency_delivery"].append({"destination": str(modules), "method": method,
                "elapsed_seconds": time.monotonic() - started, "excluded_caches": [".vite", ".vite-temp", ".cache"]})
        if modules.is_symlink() or any(p.is_symlink() and not p.resolve().is_relative_to(modules.resolve()) for p in modules.rglob("*")):
            raise ValueError("run dependencies contain an external symlink")
        report[label] = run_owned(["npm", "test", "--", "--run", allowed[1]], cwd=directory, timeout=timeout, env=env)
        if report[label]["timed_out"]:
            report.update(status="error", error="owned acceptance process group timed out")
            return report
    report["actual_test_sha256"] = hashlib.sha256((final / allowed[1]).read_bytes()).hexdigest()
    report["oracle"] = run_oracle(task, final / allowed[0], dependencies)
    oracle = report["oracle"]["result"]
    if report["base_red"]["returncode"] == 0 or report["final_green"]["returncode"] != 0 or not oracle or not oracle["passed"]:
        report["status"] = "fail"
    return report
