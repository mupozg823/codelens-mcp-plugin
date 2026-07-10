"""Private process-home isolation for productivity studies."""

from __future__ import annotations

import os
import shutil
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Final, Iterator, Mapping


SAFE_GIT_SETTINGS: Final = (
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", os.devnull),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "core.hooksPath"),
    ("GIT_CONFIG_VALUE_0", os.devnull),
)
SAFE_SHELL_SETTINGS: Final = (
    ("ZDOTDIR", os.devnull),
    ("BASH_ENV", os.devnull),
    ("ENV", os.devnull),
)


@dataclass(frozen=True, slots=True)
class StudyHomeError(RuntimeError):
    message: str

    def __str__(self) -> str:
        return self.message


def source_home_from_environment() -> Path:
    """Return the source user home before a child receives its isolated HOME."""
    value = os.environ.get("HOME")
    if not value:
        raise StudyHomeError("HOME must be set for productivity-study execution")
    return Path(value)


@contextmanager
def isolated_process_home(
    source_home: Path,
    excluded_root: Path,
) -> Iterator[Path]:
    """Yield a private home beside, never inside, the process work directory."""
    with tempfile.TemporaryDirectory(
        prefix=".codelens-study-home-",
        dir=excluded_root.resolve().parent,
    ) as raw_home:
        process_home = Path(raw_home)
        process_home.chmod(0o700)
        (process_home / ".claude").mkdir(mode=0o700)

        claude_identity = source_home / ".claude.json"
        if claude_identity.is_file():
            shutil.copy2(claude_identity, process_home / ".claude.json")

        keychains = source_home / "Library/Keychains"
        if keychains.is_dir():
            library = process_home / "Library"
            library.mkdir(mode=0o700)
            (library / "Keychains").symlink_to(keychains, target_is_directory=True)

        codelens_cache = source_home / ".cache/codelens"
        if codelens_cache.is_dir():
            cache = process_home / ".cache"
            cache.mkdir(mode=0o700)
            (cache / "codelens").symlink_to(
                codelens_cache,
                target_is_directory=True,
            )
        yield process_home


@contextmanager
def isolated_study_environment(
    excluded_root: Path,
    overlays: Mapping[str, str] | None = None,
) -> Iterator[dict[str, str]]:
    """Yield a child environment whose HOME is private and ephemeral."""
    source_home = source_home_from_environment()
    with isolated_process_home(source_home, excluded_root) as process_home:
        yield study_process_environment(
            overlays,
            process_home=process_home,
            source_home=source_home,
        )


def study_process_environment(
    overlays: Mapping[str, str] | None = None,
    *,
    process_home: Path | None = None,
    source_home: Path | None = None,
) -> dict[str, str]:
    """Return an inert Git environment with optional explicit process home."""
    environment = {
        key: value for key, value in os.environ.items() if not key.startswith("GIT_")
    }
    if overlays is not None:
        git_overlays = tuple(key for key in overlays if key.startswith("GIT_"))
        if git_overlays:
            raise StudyHomeError(
                f"study environment overlays cannot set Git variables: {git_overlays}"
            )
        environment.update(overlays)
    if (process_home is None) != (source_home is None):
        raise StudyHomeError("process_home and source_home must be provided together")
    if process_home is not None and source_home is not None:
        environment["HOME"] = str(process_home)
        environment["CODEX_HOME"] = str(source_home / ".codex")
        environment.pop("CARGO_HOME", None)
        environment.pop("RUSTUP_HOME", None)
        cargo_home = source_home / ".cargo"
        rustup_home = source_home / ".rustup"
        if cargo_home.is_dir():
            environment["CARGO_HOME"] = str(cargo_home)
        if rustup_home.is_dir():
            environment["RUSTUP_HOME"] = str(rustup_home)
    environment.update(SAFE_GIT_SETTINGS)
    environment.update(SAFE_SHELL_SETTINGS)
    return environment
