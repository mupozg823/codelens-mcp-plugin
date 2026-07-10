"""Hidden verification command execution for productivity studies."""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Sequence

from productivity_study_home import isolated_study_environment


def run_verification_commands(
    candidate: Path,
    commands: Sequence[str],
) -> tuple[bool, str | None]:
    """Run hidden commands with a private HOME outside the candidate."""
    with isolated_study_environment(candidate) as environment:
        for command in commands:
            completed = subprocess.run(
                ["zsh", "-f", "-c", command],
                cwd=candidate,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )
            if completed.returncode != 0:
                output = f"{completed.stdout}\n{completed.stderr}".strip()
                return (
                    False,
                    output[-500:] or f"verification command failed: {command}",
                )
    return True, None
