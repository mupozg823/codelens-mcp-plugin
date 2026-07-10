"""Isolated execution primitives for productivity-study-v1."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator, Sequence

from productivity_study_contract import Agent, Condition


TASK_PACK_SCHEMA = "productivity-study-task-pack-v1"


@dataclass(frozen=True, slots=True)
class TaskPackError(Exception):
    message: str

    def __str__(self) -> str:
        return self.message


@dataclass(frozen=True, slots=True)
class StudyTask:
    task_id: str
    repo_id: str
    repo_path: Path
    task_kind: str
    base_sha: str
    target_sha: str
    read_only: bool
    prompt: str
    verification_commands: tuple[str, ...]
    allowed_paths: tuple[str, ...]
    hidden_test_paths: tuple[str, ...]
    hidden_rubric: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class PlannedRun:
    task: StudyTask
    agent: Agent
    condition: Condition
    sequence_order: int


@dataclass(frozen=True, slots=True)
class CandidateGrade:
    accepted: bool
    allowed_diff: bool
    verification_passed: bool
    changed_paths: tuple[str, ...]
    failure_excerpt: str | None


@dataclass(frozen=True, slots=True)
class WorktreeRequest:
    source_repo: Path
    base_sha: str
    run_root: Path
    run_id: str

    @property
    def destination(self) -> Path:
        return self.run_root / self.run_id


@dataclass(frozen=True, slots=True)
class PolicySnapshot:
    path: Path
    sha256: str

    @classmethod
    def capture(cls, path: Path) -> PolicySnapshot:
        return cls(path=path, sha256=file_sha256(path))

    def matches(self, path: Path) -> bool:
        return self.path == path and self.sha256 == file_sha256(path)


@contextmanager
def disposable_worktree(request: WorktreeRequest) -> Iterator[Path]:
    request.run_root.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "git",
            "worktree",
            "add",
            "--detach",
            str(request.destination),
            request.base_sha,
        ],
        cwd=request.source_repo,
        check=True,
        capture_output=True,
        text=True,
    )
    try:
        yield request.destination
    finally:
        subprocess.run(
            ["git", "worktree", "remove", "--force", str(request.destination)],
            cwd=request.source_repo,
            check=True,
            capture_output=True,
            text=True,
        )


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def grade_candidate(
    candidate: Path,
    evaluator: Path,
    *,
    hidden_test_paths: Sequence[str],
    verification_commands: Sequence[str],
    allowed_paths: Sequence[str],
) -> CandidateGrade:
    changed_paths = candidate_changed_paths(candidate)
    allowed_diff = all(path_is_allowed(path, allowed_paths) for path in changed_paths)
    if not allowed_diff:
        return CandidateGrade(False, False, False, changed_paths, "candidate diff exceeded allowed paths")
    for relative_path in hidden_test_paths:
        copy_hidden_test(evaluator, candidate, relative_path)
    verification_passed, failure_excerpt = run_verification_commands(
        candidate, verification_commands
    )
    return CandidateGrade(
        accepted=verification_passed,
        allowed_diff=True,
        verification_passed=verification_passed,
        changed_paths=changed_paths,
        failure_excerpt=failure_excerpt,
    )


def candidate_changed_paths(candidate: Path) -> tuple[str, ...]:
    tracked = run_git_text(candidate, "diff", "--name-only", "--no-renames")
    staged = run_git_text(candidate, "diff", "--cached", "--name-only", "--no-renames")
    untracked = run_git_text(candidate, "ls-files", "--others", "--exclude-standard")
    paths = {line for output in (tracked, staged, untracked) for line in output.splitlines() if line}
    return tuple(sorted(paths))


def path_is_allowed(path: str, allowed_paths: Sequence[str]) -> bool:
    return any(path == allowed or path.startswith(f"{allowed.rstrip('/')}/") for allowed in allowed_paths)


def copy_hidden_test(evaluator: Path, candidate: Path, relative_path: str) -> None:
    relative = Path(relative_path)
    if relative.is_absolute() or ".." in relative.parts:
        raise TaskPackError(f"hidden test path must be relative: {relative_path}")
    source = evaluator / relative
    if not source.is_file():
        raise TaskPackError(f"hidden test does not exist in evaluator: {relative_path}")
    destination = candidate / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def run_verification_commands(
    candidate: Path, commands: Sequence[str]
) -> tuple[bool, str | None]:
    for command in commands:
        completed = subprocess.run(
            ["zsh", "-lc", command],
            cwd=candidate,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            output = f"{completed.stdout}\n{completed.stderr}".strip()
            return False, output[-500:] or f"verification command failed: {command}"
    return True, None


def run_git_text(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout


def load_task_pack(path: Path) -> tuple[StudyTask, ...]:
    try:
        raw_value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise TaskPackError(f"cannot read task pack {path}: {error}") from error
    if not isinstance(raw_value, dict):
        raise TaskPackError("task pack root must be an object")
    if raw_value.get("schema_version") != TASK_PACK_SCHEMA:
        raise TaskPackError("task pack schema_version is unsupported")
    raw_tasks = raw_value.get("tasks")
    if not isinstance(raw_tasks, list) or not raw_tasks:
        raise TaskPackError("task pack must contain at least one task")
    tasks = tuple(_parse_task(raw_task, index) for index, raw_task in enumerate(raw_tasks))
    task_ids = tuple(task.task_id for task in tasks)
    if len(set(task_ids)) != len(task_ids):
        raise TaskPackError("task pack task_id values must be unique")
    return tasks


def expand_run_plan(
    tasks: Sequence[StudyTask], agent_names: Sequence[str],
) -> tuple[PlannedRun, ...]:
    if not tasks:
        raise TaskPackError("cannot expand an empty task list")
    agents = tuple(_parse_agent(agent_name) for agent_name in agent_names)
    if not agents:
        raise TaskPackError("at least one agent is required")
    conditions = (Condition.BASELINE, Condition.NAIVE, Condition.ROUTED)
    runs: list[PlannedRun] = []
    for task_index, task in enumerate(tasks):
        for agent_index, agent in enumerate(agents):
            offset = (task_index + agent_index) % len(conditions)
            for condition_index in range(len(conditions)):
                runs.append(
                    PlannedRun(
                        task=task,
                        agent=agent,
                        condition=conditions[(offset + condition_index) % len(conditions)],
                        sequence_order=len(runs),
                    )
                )
    return tuple(runs)


def _parse_task(raw_task: object, index: int) -> StudyTask:
    if not isinstance(raw_task, dict):
        raise TaskPackError(f"tasks[{index}] must be an object")
    task_id = _required_string(raw_task, "task_id", index)
    base_sha = _required_sha(raw_task, "base_sha", index)
    target_sha = _required_sha(raw_task, "target_sha", index)
    read_only = raw_task.get("read_only")
    if not isinstance(read_only, bool):
        raise TaskPackError(f"tasks[{index}].read_only must be a boolean")
    return StudyTask(
        task_id=task_id,
        repo_id=_required_string(raw_task, "repo_id", index),
        repo_path=Path(_required_string(raw_task, "repo_path", index)),
        task_kind=_required_string(raw_task, "task_kind", index),
        base_sha=base_sha,
        target_sha=target_sha,
        read_only=read_only,
        prompt=_required_string(raw_task, "prompt", index),
        verification_commands=_required_strings(raw_task, "verification_commands", index),
        allowed_paths=_required_strings(raw_task, "allowed_paths", index),
        hidden_test_paths=_required_strings(raw_task, "hidden_test_paths", index),
        hidden_rubric=_optional_strings(raw_task, "hidden_rubric", index),
    )


def _parse_agent(agent_name: str) -> Agent:
    try:
        return Agent(agent_name)
    except ValueError as error:
        raise TaskPackError(f"unsupported agent: {agent_name}") from error


def _required_string(raw_task: dict[str, object], field: str, index: int) -> str:
    value = raw_task.get(field)
    if not isinstance(value, str) or not value:
        raise TaskPackError(f"tasks[{index}].{field} must be a non-empty string")
    return value


def _required_sha(raw_task: dict[str, object], field: str, index: int) -> str:
    value = _required_string(raw_task, field, index)
    if len(value) != 40 or any(character not in "0123456789abcdef" for character in value):
        raise TaskPackError(f"tasks[{index}].{field} must be a lowercase 40-character SHA")
    return value


def _required_strings(raw_task: dict[str, object], field: str, index: int) -> tuple[str, ...]:
    value = raw_task.get(field)
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise TaskPackError(f"tasks[{index}].{field} must be a string list")
    return tuple(value)


def _optional_strings(raw_task: dict[str, object], field: str, index: int) -> tuple[str, ...]:
    if field not in raw_task:
        return ()
    return _required_strings(raw_task, field, index)
