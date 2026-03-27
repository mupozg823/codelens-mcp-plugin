from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


PROJECT_ROOT = Path(__file__).resolve().parents[1]
CONTRACT_PATH = PROJECT_ROOT / "src" / "main" / "resources" / "codelens-contract.json"


def load_shared_contract() -> Dict[str, Any]:
    with CONTRACT_PATH.open("r", encoding="utf-8") as handle:
        return json.load(handle)


SHARED_CONTRACT = load_shared_contract()
REQUIRED_ONBOARDING_MEMORIES = SHARED_CONTRACT["required_onboarding_memories"]
SERENA_BASELINE_TOOLS = set(SHARED_CONTRACT["serena_baseline_tools"])
JETBRAINS_ALIAS_TOOLS = set(SHARED_CONTRACT["jetbrains_alias_tools"])
SEARCHABLE_EXTENSIONS = set(SHARED_CONTRACT["workspace_searchable_extensions"])


def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Standalone workspace MCP server for CodeLens")
    parser.add_argument("--workspace-root", help="Workspace root to operate on. Defaults to CODELENS_WORKSPACE_ROOT or the current working directory.")
    return parser.parse_args(argv)


def resolve_workspace_root(args: argparse.Namespace) -> Tuple[Path, str]:
    if args.workspace_root:
        return Path(args.workspace_root), "argument"
    env_root = os.environ.get("CODELENS_WORKSPACE_ROOT")
    if env_root:
        return Path(env_root), "environment"
    return Path.cwd(), "cwd"
