#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Plan the public-channel transcript without network or install side effects:
#      uv run scripts/public_release_channel_smoke.py --version X.Y.Z
# 2. Verify public release metadata:
#      python3 scripts/public_release_channel_smoke.py --version X.Y.Z --mode metadata
# 3. Run the public installer in an isolated HOME and install dir:
#      python3 scripts/public_release_channel_smoke.py --version X.Y.Z --mode installer
# ------------------

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Final


DEFAULT_REPO: Final[str] = "mupozg823/codelens-mcp-plugin"
DEFAULT_TAP_REPO: Final[str] = "mupozg823/homebrew-tap"
DEFAULT_FORMULA: Final[str] = "mupozg823/tap/codelens-mcp"
REQUIRED_CHECKSUM_ASSETS: Final[tuple[str, ...]] = (
    "codelens-mcp-darwin-arm64.tar.gz",
    "codelens-mcp-linux-x86_64.tar.gz",
    "codelens-mcp-windows-x86_64.zip",
    "release-manifest.json",
)


class ReleaseChannelSmokeError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class ChannelPlan:
    version: str
    tag: str
    repo: str
    tap_repo: str

    @property
    def installer_url(self) -> str:
        return f"https://raw.githubusercontent.com/{self.repo}/main/install.sh"

    @property
    def checksums_url(self) -> str:
        return f"https://github.com/{self.repo}/releases/download/{self.tag}/checksums-sha256.txt"

    @property
    def formula_url(self) -> str:
        return f"https://raw.githubusercontent.com/{self.tap_repo}/main/Formula/codelens-mcp.rb"


@dataclass(frozen=True, slots=True)
class CommandSpec:
    label: str
    argv: tuple[str, ...]
    cwd: Path


@dataclass(frozen=True, slots=True)
class RunContext:
    root: Path
    timeout: int
    env: dict[str, str]


def build_plan(version_or_tag: str, repo: str, tap_repo: str) -> ChannelPlan:
    version = version_or_tag.removeprefix("v")
    if not version:
        raise ReleaseChannelSmokeError("version must be non-empty")
    return ChannelPlan(
        version=version,
        tag=f"v{version}",
        repo=repo,
        tap_repo=tap_repo,
    )


def render_plan(plan: ChannelPlan) -> str:
    commands = [
        f"curl -fsSL {plan.checksums_url} -o checksums-sha256.txt",
        f"curl -fsSL {plan.installer_url} -o install.sh",
        f"curl -fsSL {plan.formula_url} -o codelens-mcp.rb",
        'ROOT=$(mktemp -d); export HOME="$ROOT/home" CODELENS_INSTALL_DIR="$ROOT/install/bin"; bash install.sh',
        "python3 scripts/smoke-clean-quickstart.py --binary $CODELENS_INSTALL_DIR/codelens-mcp --model-root $CODELENS_INSTALL_DIR",
        f"brew info --json=v2 {DEFAULT_FORMULA}",
    ]
    lines = [
        f"# Public release-channel smoke plan for {plan.tag}",
        "",
        "This plan is side-effect free until it is re-run with `--mode metadata`,",
        "`--mode installer`, or `--mode homebrew-info`.",
        "",
        "## Public endpoints",
        f"- GitHub checksums: {plan.checksums_url}",
        f"- Installer script: {plan.installer_url}",
        f"- Homebrew formula: {plan.formula_url}",
        "",
        "## Commands",
    ]
    lines.extend(
        f"{index}. `{command}`" for index, command in enumerate(commands, start=1)
    )
    return "\n".join(lines)


def run_command(spec: CommandSpec, context: RunContext) -> str:
    try:
        completed = subprocess.run(
            list(spec.argv),
            cwd=spec.cwd,
            env=context.env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=context.timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ReleaseChannelSmokeError(
            f"{spec.label} timed out after {context.timeout}s"
        ) from error
    if completed.returncode != 0:
        raise ReleaseChannelSmokeError(
            f"{spec.label} failed exit={completed.returncode}: "
            f"{completed.stderr.strip()[-4000:]}"
        )
    return completed.stdout.strip()


def curl_to(url: str, destination: Path, context: RunContext) -> str:
    command = ("curl", "-fsSL", url, "-o", str(destination))
    return run_command(
        CommandSpec("curl", command, context.root),
        context,
    )


def validate_checksums(text: str) -> list[str]:
    missing = [asset for asset in REQUIRED_CHECKSUM_ASSETS if asset not in text]
    if missing:
        raise ReleaseChannelSmokeError(
            "checksums-sha256.txt missing assets: " + ", ".join(missing)
        )
    return [f"checksums include {asset}" for asset in REQUIRED_CHECKSUM_ASSETS]


def validate_formula(text: str, plan: ChannelPlan) -> list[str]:
    required = [
        f'version "{plan.version}"',
        f"releases/download/v#{{version}}/{REQUIRED_CHECKSUM_ASSETS[0]}",
        f"releases/download/v#{{version}}/{REQUIRED_CHECKSUM_ASSETS[1]}",
        'prefix.install "models" if File.directory?("models")',
    ]
    missing = [needle for needle in required if needle not in text]
    if missing:
        raise ReleaseChannelSmokeError("Homebrew formula missing: " + ", ".join(missing))
    if "RELEASE_SHA256_" in text:
        raise ReleaseChannelSmokeError("Homebrew formula still has checksum placeholders")
    return ["formula version/checksums/model sidecar contract verified"]


def run_metadata(plan: ChannelPlan, context: RunContext) -> list[str]:
    checksums = context.root / "checksums-sha256.txt"
    formula = context.root / "codelens-mcp.rb"
    installer = context.root / "install.sh"
    curl_to(plan.checksums_url, checksums, context)
    curl_to(plan.formula_url, formula, context)
    curl_to(plan.installer_url, installer, context)
    evidence = validate_checksums(checksums.read_text(encoding="utf-8"))
    evidence.extend(validate_formula(formula.read_text(encoding="utf-8"), plan))
    if f'REPO="{plan.repo}"' not in installer.read_text(encoding="utf-8"):
        raise ReleaseChannelSmokeError("installer does not target the expected repo")
    evidence.append("installer public URL targets expected repo")
    return evidence


def run_installer(plan: ChannelPlan, context: RunContext) -> list[str]:
    evidence = run_metadata(plan, context)
    install_dir = context.root / "installer" / "bin"
    installer = context.root / "install.sh"
    env = dict(context.env)
    env["HOME"] = str(context.root / "home")
    env["CODELENS_INSTALL_DIR"] = str(install_dir)
    env["CODELENS_LOG"] = "error"
    installer_context = RunContext(context.root, context.timeout, env)
    run_command(
        CommandSpec("public installer", ("bash", str(installer)), context.root),
        installer_context,
    )
    binary = install_dir / "codelens-mcp"
    version = run_command(
        CommandSpec("installed version", (str(binary), "--version"), context.root),
        installer_context,
    )
    if plan.version not in version:
        raise ReleaseChannelSmokeError(
            f"installer produced {version!r}, expected {plan.version}"
        )
    run_command(
        CommandSpec(
            "clean quickstart from installer",
            (
                "python3",
                "scripts/smoke-clean-quickstart.py",
                "--binary",
                str(binary),
                "--model-root",
                str(install_dir),
                "--timeout",
                str(context.timeout),
            ),
            Path(__file__).resolve().parents[1],
        ),
        installer_context,
    )
    evidence.append("public installer quickstart smoke passed")
    return evidence


def run_homebrew_info(plan: ChannelPlan, context: RunContext) -> list[str]:
    evidence = run_metadata(plan, context)
    output = run_command(
        CommandSpec(
            "brew info",
            ("brew", "info", "--json=v2", DEFAULT_FORMULA),
            context.root,
        ),
        context,
    )
    parsed = json.dumps(json.loads(output), sort_keys=True)
    if plan.version not in parsed:
        raise ReleaseChannelSmokeError("brew info did not report the expected version")
    evidence.append("tapped Homebrew formula reports expected version")
    return evidence


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Post-tag public release-channel smoke transcript generator."
    )
    parser.add_argument("--version", required=True, help="Release version or tag")
    parser.add_argument("--repo", default=DEFAULT_REPO)
    parser.add_argument("--tap-repo", default=DEFAULT_TAP_REPO)
    parser.add_argument(
        "--mode",
        choices=("plan", "metadata", "installer", "homebrew-info"),
        default="plan",
    )
    parser.add_argument("--output", help="Optional markdown transcript path")
    parser.add_argument("--timeout", type=int, default=240)
    parser.add_argument("--keep-temp", action="store_true")
    return parser.parse_args()


def run(args: argparse.Namespace, root: Path) -> str:
    plan = build_plan(args.version, args.repo, args.tap_repo)
    context = RunContext(root=root, timeout=args.timeout, env=os.environ.copy())
    match args.mode:
        case "plan":
            return render_plan(plan)
        case "metadata":
            evidence = run_metadata(plan, context)
        case "installer":
            evidence = run_installer(plan, context)
        case "homebrew-info":
            evidence = run_homebrew_info(plan, context)
        case unreachable:
            raise ReleaseChannelSmokeError(f"unsupported mode: {unreachable}")
    lines = [f"# Public release-channel smoke transcript for {plan.tag}", ""]
    lines.extend(f"- {entry}" for entry in evidence)
    return "\n".join(lines)


def main() -> None:
    args = parse_args()
    try:
        if args.keep_temp:
            root = Path(tempfile.mkdtemp(prefix="codelens-release-channel-smoke."))
            transcript = run(args, root)
        else:
            with tempfile.TemporaryDirectory(
                prefix="codelens-release-channel-smoke."
            ) as raw:
                transcript = run(args, Path(raw))
    except (OSError, json.JSONDecodeError, ReleaseChannelSmokeError) as error:
        raise SystemExit(str(error)) from error
    if args.output:
        Path(args.output).write_text(transcript + "\n", encoding="utf-8")
    print(transcript)


if __name__ == "__main__":
    main()
