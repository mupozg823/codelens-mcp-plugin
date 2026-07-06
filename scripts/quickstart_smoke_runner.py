from __future__ import annotations

import json
import os
import shutil
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Final

from quickstart_smoke_contract import (
    JsonValue,
    QuickstartSmokeError,
    QuickstartSummary,
    parse_json_stdout,
    validate_capabilities,
    validate_coverage,
    validate_index,
    validate_retrieval,
    validate_status,
)


REQUIRED_MODEL_ASSETS: Final[tuple[str, ...]] = (
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
)


def run_stdout(
    command: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str],
    timeout: int,
    label: str,
) -> str:
    try:
        completed = subprocess.run(
            list(command),
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise QuickstartSmokeError(f"{label} timed out after {timeout}s") from error
    if completed.returncode != 0:
        stderr_tail = completed.stderr.strip()[-4000:]
        raise QuickstartSmokeError(
            f"{label} failed with exit={completed.returncode}: {stderr_tail}"
        )
    return completed.stdout.strip()


def run_json(
    command: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str],
    timeout: int,
    label: str,
) -> JsonValue:
    return parse_json_stdout(
        run_stdout(command, cwd=cwd, env=env, timeout=timeout, label=label),
        label,
    )


def find_model_dir(root: Path) -> Path:
    for candidate in (root / "models" / "codesearch", root / "codesearch", root):
        if all((candidate / asset).is_file() for asset in REQUIRED_MODEL_ASSETS):
            return candidate
    raise QuickstartSmokeError(f"model assets not found under {root}")


def install_prefix_layout(binary: Path, model_root: Path, root: Path) -> tuple[Path, Path]:
    installed_binary = root / "prefix" / "bin" / binary.name
    model_dest = root / "prefix" / "models" / "codesearch"
    installed_binary.parent.mkdir(parents=True)
    model_dest.parent.mkdir(parents=True)
    shutil.copy2(binary, installed_binary)
    installed_binary.chmod(0o755)
    shutil.copytree(find_model_dir(model_root), model_dest, symlinks=True)
    return installed_binary, root / "prefix" / "models"


def install_homebrew_layout(binary: Path, model_root: Path, root: Path) -> tuple[Path, Path]:
    installed_binary = root / "Cellar" / "codelens-mcp" / "0.0.0" / "bin" / binary.name
    model_dest = installed_binary.parent.parent / "models" / "codesearch"
    installed_binary.parent.mkdir(parents=True)
    model_dest.parent.mkdir(parents=True)
    shutil.copy2(binary, installed_binary)
    installed_binary.chmod(0o755)
    shutil.copytree(find_model_dir(model_root), model_dest, symlinks=True)
    return installed_binary, model_dest.parent


def write_fixture_project(root: Path) -> Path:
    project = root / "project"
    source_dir = project / "src"
    source_dir.mkdir(parents=True)
    project.joinpath("Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                'name = "codelens-clean-quickstart-fixture"',
                'version = "0.0.0"',
                'edition = "2021"',
                "",
                "[lib]",
                'path = "src/lib.rs"',
                "",
            ]
        ),
        encoding="utf-8",
    )
    source_dir.joinpath("lib.rs").write_text(
        "\n".join(
            [
                "/// Adds two values for the clean quickstart smoke.",
                "pub fn add_values(left: i32, right: i32) -> i32 {",
                "    left + right",
                "}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return project


def write_codex_config(home: Path, binary: Path, project: Path) -> None:
    config_dir = home / ".codex"
    config_dir.mkdir(parents=True)
    config_dir.joinpath("config.toml").write_text(
        "\n".join(
            [
                "[mcp_servers.codelens]",
                f"command = {json.dumps(str(binary))}",
                f"args = [{json.dumps(str(project))}]",
                "",
            ]
        ),
        encoding="utf-8",
    )


def build_smoke_env(home: Path, model_root: Path, *, use_model_env: bool) -> dict[str, str]:
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["CODELENS_LOG"] = "error"
    if use_model_env:
        env["CODELENS_MODEL_DIR"] = str(model_root)
    else:
        env.pop("CODELENS_MODEL_DIR", None)
    return env


def tool_json(
    binary: Path,
    project: Path,
    env: Mapping[str, str],
    timeout: int,
    args: Sequence[str],
    label: str,
) -> JsonValue:
    return run_json(
        [str(binary), str(project), *args],
        cwd=project,
        env=env,
        timeout=timeout,
        label=label,
    )


def run_installed_smoke(
    binary: Path,
    root: Path,
    timeout: int,
    *,
    use_model_env: bool = False,
    model_env_root: Path | None = None,
) -> QuickstartSummary:
    project = write_fixture_project(root)
    home = root / "home"
    write_codex_config(home, binary, project)
    env = build_smoke_env(
        home,
        model_env_root or root / "prefix" / "models",
        use_model_env=use_model_env,
    )

    version = run_stdout(
        [str(binary), "--version"],
        cwd=project,
        env=env,
        timeout=timeout,
        label="version",
    )
    validate_status(
        run_json(
            [str(binary), "status", "codex", "--json"],
            cwd=project,
            env=env,
            timeout=timeout,
            label="status",
        )
    )
    validate_capabilities(
        tool_json(
            binary,
            project,
            env,
            timeout,
            ["--cmd", "get_capabilities", "--args", "{}"],
            "get_capabilities",
        )
    )
    validate_index(
        tool_json(binary, project, env, timeout, ["--cmd", "index_embeddings"], "index_embeddings")
    )
    coverage = validate_coverage(
        tool_json(
            binary,
            project,
            env,
            timeout,
            ["--cmd", "embedding_coverage_report"],
            "embedding_coverage_report",
        )
    )
    retrieval = validate_retrieval(
        tool_json(
            binary,
            project,
            env,
            timeout,
            [
                "--cmd",
                "get_ranked_context",
                "--args",
                '{"query":"function that adds two values","max_tokens":1200,"depth":1,"include_body":false}',
            ],
            "get_ranked_context",
        )
    )
    return QuickstartSummary(version=version, temp_root=root, coverage=coverage, retrieval=retrieval)


def run_smoke(
    binary: Path,
    model_root: Path,
    root: Path,
    timeout: int,
    *,
    use_model_env: bool = False,
    homebrew_layout: bool = False,
) -> QuickstartSummary:
    installed_binary, model_env_root = (
        install_homebrew_layout(binary, model_root, root)
        if homebrew_layout
        else install_prefix_layout(binary, model_root, root)
    )
    return run_installed_smoke(
        installed_binary,
        root,
        timeout,
        use_model_env=use_model_env,
        model_env_root=model_env_root,
    )
