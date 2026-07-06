#!/usr/bin/env python3
"""Smoke CodeLens on a matrix of real or fixture projects."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MATRIX = ROOT / "benchmarks" / "external-project-smoke-matrix.json"
DEFAULT_BINARY = ROOT / "target" / "debug" / "codelens-mcp"


class MatrixItemError(ValueError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", default=str(DEFAULT_MATRIX))
    parser.add_argument("--binary", default=str(DEFAULT_BINARY))
    parser.add_argument("--output", default=None)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--keep-workdirs", action="store_true", help="Keep copied projects for debugging")
    return parser.parse_args()


def load_matrix(path: Path) -> list[dict[str, Any]]:
    payload = json.loads(path.read_text())
    projects = payload["projects"] if isinstance(payload, dict) else payload
    if not isinstance(projects, list) or not projects:
        raise SystemExit(f"matrix has no projects: {path}")
    return projects


def projects_missing_expected_search(projects: list[dict[str, Any]]) -> list[str]:
    return [
        str(item.get("name", item.get("git_url", item.get("path", "unknown"))))
        for item in projects
        if not isinstance(item.get("expected_search"), dict)
    ]


def copy_project(source: Path, keep: bool) -> tuple[Path, tempfile.TemporaryDirectory[str] | None]:
    if keep:
        dest = Path(tempfile.mkdtemp(prefix="codelens-external-smoke-")) / source.name
        shutil.copytree(source, dest)
        return dest, None
    tmp = tempfile.TemporaryDirectory(prefix="codelens-external-smoke-")
    dest = Path(tmp.name) / source.name
    shutil.copytree(source, dest)
    return dest, tmp


def materialize_project(
    item: dict[str, Any],
    keep: bool,
    timeout: int,
) -> tuple[Path, tempfile.TemporaryDirectory[str] | None]:
    if "path" in item:
        source = (ROOT / item["path"]).resolve()
        if not source.is_dir():
            raise FileNotFoundError(f"project path missing: {source}")
        return copy_project(source, keep)

    git_url = item.get("git_url") or item.get("repo_url")
    if not git_url:
        raise MatrixItemError("matrix item must set either path or git_url")

    tmp = None if keep else tempfile.TemporaryDirectory(prefix="codelens-external-smoke-")
    work_root = Path(tempfile.mkdtemp(prefix="codelens-external-smoke-")) if keep else Path(tmp.name)
    name = item.get("name", "project").replace("/", "-")
    dest = work_root / name
    clone_depth = item.get("clone_depth", 1)
    clone_cmd = ["git", "clone", "--quiet"]
    if clone_depth:
        clone_cmd.extend(["--depth", str(clone_depth)])
    clone_cmd.extend([str(git_url), str(dest)])
    subprocess.run(
        clone_cmd,
        cwd=ROOT,
        timeout=timeout,
        check=True,
    )
    revision = item.get("revision") or item.get("rev")
    if revision:
        checkout = subprocess.run(
            ["git", "checkout", "--quiet", str(revision)],
            cwd=dest,
            timeout=timeout,
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if checkout.returncode != 0:
            subprocess.run(
                ["git", "fetch", "--quiet", "--depth", "1", "origin", str(revision)],
                cwd=dest,
                timeout=timeout,
                check=True,
            )
            subprocess.run(
                ["git", "checkout", "--quiet", str(revision)],
                cwd=dest,
                timeout=timeout,
                check=True,
            )
    return dest, tmp


def run_tool(
    binary: Path,
    project: Path,
    tool: str,
    args: dict[str, Any],
    timeout: int,
    env: dict[str, str],
) -> dict[str, Any]:
    started = time.perf_counter()
    completed = subprocess.run(
        [str(binary), str(project), "--cmd", tool, "--args", json.dumps(args)],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    elapsed_ms = round((time.perf_counter() - started) * 1000)
    parsed: Any = None
    if completed.stdout.strip():
        try:
            parsed = json.loads(completed.stdout)
        except json.JSONDecodeError:
            parsed = completed.stdout.strip()
    return {
        "tool": tool,
        "ok": completed.returncode == 0 and payload_success(parsed),
        "exit_code": completed.returncode,
        "elapsed_ms": elapsed_ms,
        "stdout": parsed,
        "stderr_tail": completed.stderr[-4000:],
    }


def payload_success(payload: Any) -> bool:
    if isinstance(payload, dict) and "success" in payload:
        return bool(payload["success"])
    return True


def expected_search_error(
    search_step: dict[str, Any],
    expected: dict[str, Any],
) -> str | None:
    stdout = search_step.get("stdout")
    data = stdout.get("data") if isinstance(stdout, dict) else None
    results = data.get("results") if isinstance(data, dict) else None
    if not isinstance(results, list):
        return "semantic_search response did not include a results list"

    expected_symbol = expected.get("symbol_name")
    expected_file = expected.get("file_path")
    if expected_symbol is None and expected_file is None:
        return "expected_search must set symbol_name or file_path"

    max_rank_raw = expected.get("max_rank", len(results))
    max_rank = max_rank_raw if isinstance(max_rank_raw, int) and max_rank_raw > 0 else len(results)
    for rank, hit in enumerate(results[:max_rank], start=1):
        if not isinstance(hit, dict):
            continue
        symbol_matches = expected_symbol is None or hit.get("symbol_name") == expected_symbol
        file_matches = expected_file is None or hit.get("file_path") == expected_file
        if symbol_matches and file_matches:
            return None

    return (
        "expected semantic_search hit not found within "
        f"top {max_rank}: symbol_name={expected_symbol!r}, file_path={expected_file!r}"
    )


def default_env() -> dict[str, str]:
    env = os.environ.copy()
    env.setdefault("CODELENS_LOG", "warn")
    model_dir = ROOT / "crates" / "codelens-engine" / "models"
    if "CODELENS_MODEL_DIR" not in env and model_dir.exists():
        env["CODELENS_MODEL_DIR"] = str(model_dir)
    return env


def smoke_project(
    item: dict[str, Any],
    binary: Path,
    timeout: int,
    keep_workdirs: bool,
    env: dict[str, str],
) -> dict[str, Any]:
    try:
        project, tmp = materialize_project(item, keep_workdirs, timeout)
    except (FileNotFoundError, MatrixItemError, subprocess.SubprocessError) as error:
        return {
            "name": item.get("name", item.get("git_url", item.get("path", "unknown"))),
            "kind": item.get("kind"),
            "ok": False,
            "error": str(error),
            "steps": [],
        }

    try:
        refresh_step = run_tool(binary, project, "refresh_symbol_index", {}, timeout, env)
        index_step = run_tool(
            binary,
            project,
            "index_embeddings",
            {"prewarm_queries": item.get("prewarm_queries", [item["query"]])},
            timeout,
            env,
        )
        search_step = run_tool(
            binary,
            project,
            "semantic_search",
            {
                "query": item["query"],
                "max_results": item.get("max_results", 8),
                "path_hint": item.get("path_hint"),
            },
            timeout,
            env,
        )
        if isinstance(item.get("expected_search"), dict):
            quality_error = expected_search_error(search_step, item["expected_search"])
            if quality_error is not None:
                search_step = {**search_step, "ok": False, "quality_error": quality_error}
        rename_step = run_tool(
            binary,
            project,
            "rename_symbol",
            {**item["mutation"], "dry_run": True},
            timeout,
            env,
        )
        steps = [refresh_step, index_step, search_step, rename_step]
        return {
            "name": item["name"],
            "kind": item.get("kind"),
            "project": str(project),
            "ok": all(step["ok"] for step in steps),
            "steps": steps,
        }
    finally:
        if tmp is not None:
            tmp.cleanup()


def main() -> None:
    args = parse_args()
    matrix_path = Path(args.matrix).resolve()
    binary = Path(args.binary).resolve()
    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}; run cargo build -p codelens-mcp first")

    projects = load_matrix(matrix_path)
    if args.check:
        missing_expected = projects_missing_expected_search(projects)
        if missing_expected:
            raise SystemExit(
                "--check requires expected_search for every project: "
                + ", ".join(missing_expected)
            )

    results = [
        smoke_project(item, binary, args.timeout, args.keep_workdirs, default_env())
        for item in projects
    ]
    report = {
        "schema_version": "codelens-external-project-smoke-v1",
        "matrix": str(matrix_path),
        "binary": str(binary),
        "ok": all(item["ok"] for item in results),
        "projects": results,
    }
    text = json.dumps(report, indent=2)
    if args.output:
        Path(args.output).write_text(text + "\n")
    print(text)
    if args.check and not report["ok"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
