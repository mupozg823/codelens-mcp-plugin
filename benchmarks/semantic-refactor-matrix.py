#!/usr/bin/env python3
"""Gate semantic refactor operations against fixture or pinned external projects."""

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
DEFAULT_MATRIX = ROOT / "benchmarks" / "semantic-refactor-upstream-matrix.json"
DEFAULT_BINARY = ROOT / "target" / "debug" / "codelens-mcp"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", default=str(DEFAULT_MATRIX))
    parser.add_argument("--binary", default=str(DEFAULT_BINARY))
    parser.add_argument("--output", default=None)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--validate-only", action="store_true")
    parser.add_argument("--keep-workdirs", action="store_true")
    return parser.parse_args()


def load_matrix(path: Path) -> list[dict[str, Any]]:
    payload = json.loads(path.read_text())
    projects = payload["projects"] if isinstance(payload, dict) else payload
    if not isinstance(projects, list) or not projects:
        raise SystemExit(f"matrix has no projects: {path}")
    validate_matrix(projects)
    return projects


def validate_matrix(projects: list[dict[str, Any]]) -> None:
    for item in projects:
        if not item.get("name"):
            raise ValueError("matrix project missing name")
        if not (item.get("path") or item.get("git_url") or item.get("repo_url")):
            raise ValueError(f"{item['name']}: set path or git_url")
        operations = item.get("operations")
        if not isinstance(operations, list) or not operations:
            raise ValueError(f"{item['name']}: operations must be a non-empty list")
        for operation in operations:
            tool = operation.get("tool")
            if tool not in {
                "resolve_symbol_target",
                "rename_symbol",
                "propagate_deletions",
                "refactor_extract_function",
                "refactor_inline_function",
                "refactor_move_to_file",
                "refactor_change_signature",
            }:
                raise ValueError(f"{item['name']}: unsupported semantic refactor tool {tool!r}")
            if not isinstance(operation.get("args"), dict):
                raise ValueError(f"{item['name']}:{tool}: args must be an object")


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
    tmp = None if keep else tempfile.TemporaryDirectory(prefix="codelens-semantic-refactor-")
    work_root = Path(tempfile.mkdtemp(prefix="codelens-semantic-refactor-")) if keep else Path(tmp.name)
    name = item.get("name", "project").replace("/", "-")
    dest = work_root / name
    clone_cmd = ["git", "clone", "--quiet"]
    clone_depth = item.get("clone_depth", 1)
    if clone_depth:
        clone_cmd.extend(["--depth", str(clone_depth)])
    clone_cmd.extend([str(git_url), str(dest)])
    subprocess.run(clone_cmd, cwd=ROOT, timeout=timeout, check=True)
    revision = item.get("revision") or item.get("rev")
    if revision:
        checkout_revision(dest, revision, timeout)
    return dest, tmp


def copy_project(source: Path, keep: bool) -> tuple[Path, tempfile.TemporaryDirectory[str] | None]:
    if keep:
        dest = Path(tempfile.mkdtemp(prefix="codelens-semantic-refactor-")) / source.name
        shutil.copytree(source, dest)
        return dest, None
    tmp = tempfile.TemporaryDirectory(prefix="codelens-semantic-refactor-")
    dest = Path(tmp.name) / source.name
    shutil.copytree(source, dest)
    return dest, tmp


def checkout_revision(project: Path, revision: str, timeout: int) -> None:
    checkout = subprocess.run(
        ["git", "checkout", "--quiet", revision],
        cwd=project,
        timeout=timeout,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if checkout.returncode == 0:
        return
    subprocess.run(
        ["git", "fetch", "--quiet", "--depth", "1", "origin", revision],
        cwd=project,
        timeout=timeout,
        check=True,
    )
    subprocess.run(["git", "checkout", "--quiet", revision], cwd=project, timeout=timeout, check=True)


def run_tool(
    binary: Path,
    project: Path,
    operation: dict[str, Any],
    timeout: int,
    env: dict[str, str],
) -> dict[str, Any]:
    started = time.perf_counter()
    completed = subprocess.run(
        [
            str(binary),
            str(project),
            "--cmd",
            operation["tool"],
            "--args",
            json.dumps(operation["args"]),
        ],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    elapsed_ms = round((time.perf_counter() - started) * 1000)
    payload = parse_stdout(completed.stdout)
    ok = completed.returncode == 0 and expected_payload(payload, operation.get("expect", {}))
    return {
        "tool": operation["tool"],
        "ok": ok,
        "exit_code": completed.returncode,
        "elapsed_ms": elapsed_ms,
        "stdout": payload,
        "stderr_tail": completed.stderr[-4000:],
    }


def parse_stdout(stdout: str) -> Any:
    if not stdout.strip():
        return None
    try:
        return json.loads(stdout)
    except json.JSONDecodeError:
        return stdout.strip()


def expected_payload(payload: Any, expect: dict[str, Any]) -> bool:
    if not isinstance(payload, dict):
        return False
    if "success" in expect and bool(payload.get("success")) != bool(expect["success"]):
        return False
    for dotted, expected in expect.get("equals", {}).items():
        if lookup(payload, dotted) != expected:
            return False
    for dotted in expect.get("present", []):
        if lookup(payload, dotted) is None:
            return False
    return True


def lookup(payload: Any, dotted: str) -> Any:
    current = payload
    for part in dotted.split("."):
        if isinstance(current, dict):
            current = current.get(part)
        elif isinstance(current, list) and part.isdigit():
            index = int(part)
            current = current[index] if index < len(current) else None
        else:
            return None
    return current


def default_env() -> dict[str, str]:
    env = os.environ.copy()
    env.setdefault("CODELENS_LOG", "warn")
    env.setdefault("CODELENS_SEMANTIC_EDIT_BACKEND", "lsp")
    model_dir = ROOT / "crates" / "codelens-engine" / "models"
    if "CODELENS_MODEL_DIR" not in env and model_dir.exists():
        env["CODELENS_MODEL_DIR"] = str(model_dir)
    return env


def run_project(
    item: dict[str, Any],
    binary: Path,
    timeout: int,
    keep_workdirs: bool,
    env: dict[str, str],
) -> dict[str, Any]:
    try:
        project, cleanup = materialize_project(item, keep_workdirs, timeout)
    except (FileNotFoundError, ValueError, subprocess.SubprocessError) as error:
        return {"name": item.get("name", "unknown"), "kind": item.get("kind"), "ok": False, "error": str(error), "steps": []}

    try:
        steps = [run_tool(binary, project, operation, timeout, env) for operation in item["operations"]]
        return {
            "name": item["name"],
            "kind": item.get("kind"),
            "project": str(project),
            "ok": all(step["ok"] for step in steps),
            "steps": steps,
        }
    finally:
        if cleanup is not None:
            cleanup.cleanup()


def main() -> int:
    args = parse_args()
    matrix_path = Path(args.matrix)
    projects = load_matrix(matrix_path)
    if args.validate_only:
        print(json.dumps({"schema_version": "codelens-semantic-refactor-matrix-v1", "valid": True, "projects": len(projects)}, indent=2))
        return 0

    binary = Path(args.binary)
    if not binary.is_file():
        raise SystemExit(f"binary missing: {binary}")
    env = default_env()
    results = [run_project(item, binary, args.timeout, args.keep_workdirs, env) for item in projects]
    summary = {
        "schema_version": "codelens-semantic-refactor-results-v1",
        "matrix": str(matrix_path),
        "binary": str(binary),
        "ok": all(item["ok"] for item in results),
        "projects": results,
    }
    encoded = json.dumps(summary, indent=2)
    if args.output:
        Path(args.output).write_text(encoded + "\n")
    print(encoded)
    return 0 if summary["ok"] or not args.check else 1


if __name__ == "__main__":
    raise SystemExit(main())
