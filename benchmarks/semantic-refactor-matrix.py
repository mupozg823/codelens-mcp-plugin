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
    parser.add_argument("--preset", default="full")
    parser.add_argument("--profile", default=None)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--require-all-backends", action="store_true")
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
        for key in ("required_command", "skip_if_missing_command"):
            if key in item and not isinstance(item[key], str):
                raise ValueError(f"{item['name']}: {key} must be a string")
        validate_env(item, item["name"])
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
            for key in ("required_command", "skip_if_missing_command", "preset", "profile"):
                if key in operation and not isinstance(operation[key], str):
                    raise ValueError(f"{item['name']}:{tool}: {key} must be a string")
            validate_env(operation, f"{item['name']}:{tool}")


def validate_env(scope: dict[str, Any], label: str) -> None:
    env = scope.get("env")
    if env is None:
        return
    if not isinstance(env, dict):
        raise ValueError(f"{label}: env must be an object")
    for key, value in env.items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise ValueError(f"{label}: env keys and values must be strings")


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
    default_preset: str,
    default_profile: str | None,
) -> dict[str, Any]:
    missing_command = missing_required_command(operation)
    if missing_command:
        return skipped_step(operation["tool"], f"missing command: {missing_command}")

    tool_env = merged_env(env, operation.get("env"))
    command = [str(binary), str(project)]
    preset = operation.get("preset", default_preset)
    profile = operation.get("profile", default_profile)
    if preset:
        command.extend(["--preset", preset])
    if profile:
        command.extend(["--profile", profile])
    command.extend(
        [
            "--cmd",
            operation["tool"],
            "--args",
            json.dumps(operation["args"]),
        ]
    )
    started = time.perf_counter()
    max_attempts = int(operation.get("max_attempts", 2))
    max_attempts = max(1, min(max_attempts, 3))
    last_result: dict[str, Any] | None = None
    for attempt in range(1, max_attempts + 1):
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=tool_env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
        elapsed_ms = round((time.perf_counter() - started) * 1000)
        payload = parse_stdout(completed.stdout)
        unsupported = is_unsupported_payload(payload) if isinstance(payload, dict) else False
        ok = completed.returncode == 0 and expected_payload(payload, operation.get("expect", {}))
        last_result = {
            "tool": operation["tool"],
            "ok": ok,
            "unsupported": unsupported,
            "exit_code": completed.returncode,
            "elapsed_ms": elapsed_ms,
            "attempts": attempt,
            "stdout": payload,
            "stderr_tail": completed.stderr[-4000:],
        }
        if ok or attempt == max_attempts or not retryable_lsp_failure(payload, completed.stderr):
            return last_result
        time.sleep(1)
    assert last_result is not None
    return last_result


def missing_required_command(scope: dict[str, Any]) -> str | None:
    command = scope.get("skip_if_missing_command") or scope.get("required_command")
    if command and shutil.which(command) is None:
        return command
    return None


def skipped_step(tool: str, reason: str) -> dict[str, Any]:
    return {
        "tool": tool,
        "ok": True,
        "skipped": True,
        "skip_reason": reason,
        "exit_code": None,
        "elapsed_ms": 0,
        "stdout": None,
        "stderr_tail": "",
    }


def retryable_lsp_failure(payload: Any, stderr: str) -> bool:
    haystack = " ".join(
        item
        for item in [
            payload.get("error") if isinstance(payload, dict) else None,
            stderr,
        ]
        if isinstance(item, str)
    ).lower()
    return "content modified" in haystack or "server not initialized" in haystack


def merged_env(base: dict[str, str], overlay: dict[str, str] | None) -> dict[str, str]:
    if not overlay:
        return base
    merged = base.copy()
    merged.update(overlay)
    return merged


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
    unsupported = is_unsupported_payload(payload)
    if unsupported and not expect.get("unsupported"):
        return False
    if expect.get("unsupported") and not unsupported:
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


def is_unsupported_payload(payload: dict[str, Any]) -> bool:
    values = [
        payload.get("status"),
        payload.get("support"),
        lookup(payload, "data.status"),
        lookup(payload, "data.support"),
        lookup(payload, "data.blocker_reason"),
        payload.get("error"),
    ]
    return any(
        isinstance(value, str)
        and (
            value == "unsupported"
            or value == "unsupported_semantic_refactor"
            or "unsupported_semantic_refactor" in value
        )
        for value in values
    )


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
    env.setdefault("CODELENS_LSP_STARTUP_GRACE_MS", "8000")
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
    default_preset: str,
    default_profile: str | None,
) -> dict[str, Any]:
    missing_command = missing_required_command(item)
    if missing_command:
        return {
            "name": item.get("name", "unknown"),
            "kind": item.get("kind"),
            "ok": True,
            "skipped": True,
            "skip_reason": f"missing command: {missing_command}",
            "steps": [],
        }

    try:
        project, cleanup = materialize_project(item, keep_workdirs, timeout)
    except (FileNotFoundError, ValueError, subprocess.SubprocessError) as error:
        return {"name": item.get("name", "unknown"), "kind": item.get("kind"), "ok": False, "error": str(error), "steps": []}

    try:
        project_env = merged_env(env, item.get("env"))
        steps = [
            run_tool(
                binary,
                project,
                operation,
                timeout,
                project_env,
                default_preset,
                default_profile,
            )
            for operation in item["operations"]
        ]
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
    results = [
        run_project(
            item,
            binary,
            args.timeout,
            args.keep_workdirs,
            env,
            args.preset,
            args.profile,
        )
        for item in projects
    ]
    skipped = sum(1 for item in results if item.get("skipped"))
    skipped += sum(
        1
        for item in results
        for step in item.get("steps", [])
        if step.get("skipped")
    )
    unsupported = sum(
        1
        for item in results
        for step in item.get("steps", [])
        if step.get("unsupported")
    )
    ok = all(item["ok"] for item in results) and (
        skipped == 0 or not args.require_all_backends
    )
    summary = {
        "schema_version": "codelens-semantic-refactor-results-v1",
        "matrix": str(matrix_path),
        "binary": str(binary),
        "ok": ok,
        "skipped": skipped,
        "unsupported": unsupported,
        "require_all_backends": args.require_all_backends,
        "projects": results,
    }
    encoded = json.dumps(summary, indent=2)
    if args.output:
        Path(args.output).write_text(encoded + "\n")
    print(encoded)
    return 0 if summary["ok"] or not args.check else 1


if __name__ == "__main__":
    raise SystemExit(main())
