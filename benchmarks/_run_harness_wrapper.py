#!/usr/bin/env python3
"""Compatibility launcher for harness-specific benchmark scripts."""

from __future__ import annotations

import runpy
import sys
from pathlib import Path


def run(script_name: str):
    harness_dir = Path(__file__).resolve().parent / "harness"
    script_path = harness_dir / script_name
    if not script_path.exists():
        raise FileNotFoundError(f"missing harness script: {script_path}")
    original_sys_path = list(sys.path)
    sys.path.insert(0, str(harness_dir))
    try:
        runpy.run_path(str(script_path), run_name="__main__")
    finally:
        sys.path[:] = original_sys_path
