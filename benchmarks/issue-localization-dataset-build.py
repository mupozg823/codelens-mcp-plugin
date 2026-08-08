#!/usr/bin/env python3
"""Mine fix commits into issue-localization ground-truth candidates.

Protocol follows CodeScout (arXiv 2603.17829): an issue text is mapped to a
three-level location set (file / container / function) parsed from the gold
patch. This miner emits *candidates* only -- every entry is meant to be read
against ``git show`` and hand-corrected before it lands in the labeled set.

Ground-truth parsing rules implemented here:

* member addition is attributed to the enclosing container (and the file), not
  to a synthetic function entry;
* import / top-level-static edits are file-level only;
* doc-comment and comment-only hunks are ignored;
* commits whose gold patch is entirely file creation or deletion are dropped;
* a candidate is dropped when any gold file is absent from the current HEAD,
  because the runner scores against a HEAD-state index rather than the paper's
  pre-PR snapshot.

Enclosing symbols are resolved against the *post-commit* blob (``git show
sha:path``) rather than HEAD, so hunk line numbers never drift.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

CODE_SUFFIXES = (".rs",)

HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")

CONTAINER_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:unsafe\s+|default\s+)?"
    r"(impl|mod|struct|enum|trait|union)\b(.*)$"
)
FN_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r"(?:extern\s+\"[^\"]*\"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"
)
USE_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:use|extern\s+crate)\b")
STATIC_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:static|const)\s+[A-Z_][A-Z0-9_]*"
)

SKIP_SUBJECT_RE = re.compile(r"\b(typo|rustfmt|cargo fmt|formatting|whitespace)\b", re.I)


def git(args: list[str], repo: Path) -> str:
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        return ""
    return proc.stdout


def strip_line_comment(line: str) -> str:
    out = []
    in_str = False
    escaped = False
    idx = 0
    while idx < len(line):
        ch = line[idx]
        if in_str:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_str = False
            out.append(ch)
        else:
            if ch == '"':
                in_str = True
                out.append(ch)
            elif ch == "/" and idx + 1 < len(line) and line[idx + 1] == "/":
                break
            else:
                out.append(ch)
        idx += 1
    return "".join(out)


def is_comment_only(text: str) -> bool:
    stripped = text.strip()
    if not stripped:
        return True
    if stripped.startswith("#![doc"):
        return True
    return stripped.startswith(("//", "/*", "*", "*/"))


def container_label(kind: str, rest: str) -> str | None:
    """Render an ``impl``/``mod``/``struct`` header into a stable label."""
    rest = rest.split("//")[0]
    rest = rest.split("{")[0].split("where")[0].strip()
    if kind == "impl":
        if " for " in rest:
            trait_part, type_part = rest.split(" for ", 1)
            trait_name = base_name(trait_part)
            type_name = base_name(type_part)
            if trait_name and type_name:
                return f"impl {trait_name} for {type_name}"
            return f"impl {type_name}" if type_name else None
        name = base_name(rest)
        return f"impl {name}" if name else None
    name = base_name(rest)
    return f"{kind} {name}" if name else None


def base_name(text: str) -> str:
    text = text.strip()
    text = re.sub(r"^<[^>]*>\s*", "", text)
    text = re.sub(r"^(?:&(?:'\w+\s*)?(?:mut\s+)?)+", "", text)
    text = text.split("<")[0]
    text = text.split("(")[0]
    text = text.strip().rstrip(":")
    if "::" in text:
        text = text.split("::")[-1]
    return text.strip()


def scan_rust(source: str) -> list[dict]:
    """Return one record per line: enclosing container stack and function."""
    records: list[dict] = []
    stack: list[dict] = []
    pending: dict | None = None
    depth = 0
    in_block_comment = False

    for line in source.splitlines():
        code = line
        if in_block_comment:
            if "*/" in code:
                code = code.split("*/", 1)[1]
                in_block_comment = False
            else:
                code = ""
        code = strip_line_comment(code)
        if "/*" in code:
            head, _, tail = code.partition("/*")
            if "*/" in tail:
                code = head + tail.split("*/", 1)[1]
            else:
                code = head
                in_block_comment = True

        opened = code.count("{")
        closed = code.count("}")

        entry_container = [
            frame["label"] for frame in stack if frame["kind"] == "container"
        ]
        entry_fn = next(
            (frame["label"] for frame in reversed(stack) if frame["kind"] == "fn"),
            None,
        )
        records.append(
            {
                "containers": list(entry_container),
                "function": entry_fn,
                "raw": line,
            }
        )

        if pending is None:
            fn_match = FN_RE.match(code)
            if fn_match:
                pending = {"kind": "fn", "label": fn_match.group(1)}
            else:
                cm = CONTAINER_RE.match(code)
                if cm:
                    label = container_label(cm.group(1), cm.group(2))
                    if label:
                        pending = {"kind": "container", "label": label}

        for _ in range(opened):
            if pending is not None:
                stack.append({**pending, "depth": depth})
                pending = None
            else:
                stack.append({"kind": "block", "label": None, "depth": depth})
            depth += 1
        for _ in range(closed):
            if stack:
                stack.pop()
            depth = max(0, depth - 1)
        if closed and pending is not None and opened == 0:
            pending = None
        if ";" in code and opened == 0:
            pending = None

    return records


def parse_gold(repo: Path, sha: str) -> dict:
    name_status = git(["show", "--name-status", "--pretty=format:", sha], repo)
    statuses: dict[str, str] = {}
    for row in name_status.splitlines():
        parts = row.split("\t")
        if len(parts) >= 2 and parts[0]:
            statuses[parts[-1]] = parts[0][0]

    diff = git(["show", "--unified=0", "--pretty=format:", sha], repo)
    files: set[str] = set()
    containers: set[str] = set()
    functions: set[str] = set()
    file_level_only: set[str] = set()

    current: str | None = None
    new_line = 0
    scans: dict[str, list[dict]] = {}
    pending_added: list[tuple[str, int]] = []

    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            continue
        if line.startswith("+++ /dev/null"):
            current = None
            continue
        hunk = HUNK_RE.match(line)
        if hunk:
            new_line = int(hunk.group(1))
            continue
        if current is None or not current.endswith(CODE_SUFFIXES):
            continue
        if line.startswith("+") and not line.startswith("+++"):
            body = line[1:]
            if not is_comment_only(body):
                pending_added.append((current, new_line))
            new_line += 1
        elif line.startswith("-") and not line.startswith("---"):
            # Deletion-only hunks anchor at the surrounding new-side line.
            body = line[1:]
            if not is_comment_only(body):
                pending_added.append((current, max(1, new_line - 1)))

    for path, lineno in pending_added:
        if statuses.get(path) in {"A", "D"}:
            continue
        files.add(path)
        if path not in scans:
            blob = git(["show", f"{sha}:{path}"], repo)
            scans[path] = scan_rust(blob) if blob else []
        records = scans[path]
        if not records or lineno - 1 >= len(records):
            file_level_only.add(path)
            continue
        record = records[lineno - 1]
        raw = record["raw"]
        if raw.lstrip().startswith("#!["):
            # Crate/module inner attribute: a file-level location, no container.
            file_level_only.add(path)
            continue
        if USE_RE.match(raw) or STATIC_RE.match(raw):
            file_level_only.add(path)
            continue
        for label in record["containers"]:
            containers.add(f"{path}::{label}")
        if record["function"]:
            if record["containers"]:
                functions.add(
                    f"{path}::{record['containers'][-1]}::{record['function']}"
                )
            else:
                functions.add(f"{path}::{record['function']}")

    return {
        "files": sorted(files),
        "containers": sorted(containers),
        "functions": sorted(functions),
        "file_level_only": sorted(file_level_only),
        "statuses": statuses,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=".", help="repository root")
    parser.add_argument("--limit", type=int, default=400, help="commits to scan")
    parser.add_argument("--max-files", type=int, default=4, help="max gold files")
    parser.add_argument("--output", default="-", help="candidate JSON path")
    args = parser.parse_args()

    repo = Path(args.repo).resolve()
    log = git(["log", f"-{args.limit}", "--pretty=format:%H\x1f%s\x1f%b\x1e"], repo)
    if not log.strip():
        print("no commits scanned", file=sys.stderr)
        return 2

    head_files = set(git(["ls-tree", "-r", "--name-only", "HEAD"], repo).splitlines())

    candidates = []
    rejected: dict[str, int] = {}

    def reject(reason: str) -> None:
        rejected[reason] = rejected.get(reason, 0) + 1

    for record in log.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        parts = record.split("\x1f")
        if len(parts) < 2:
            continue
        sha, subject = parts[0], parts[1]
        body = parts[2] if len(parts) > 2 else ""
        if not re.match(r"^fix[(:]", subject):
            continue
        if SKIP_SUBJECT_RE.search(subject):
            reject("typo_or_format_subject")
            continue

        gold = parse_gold(repo, sha)
        if not gold["files"]:
            reject("no_code_change_after_filters")
            continue
        statuses = gold["statuses"]
        code_paths = [p for p in statuses if p.endswith(CODE_SUFFIXES)]
        if code_paths and all(statuses[p] in {"A", "D"} for p in code_paths):
            reject("create_or_delete_only")
            continue
        missing = [p for p in gold["files"] if p not in head_files]
        if missing:
            reject("gold_file_absent_at_head")
            continue
        if len(gold["files"]) > args.max_files:
            reject("too_many_gold_files")
            continue

        query = subject if not body.strip() else f"{subject}\n\n{body.strip()}"
        candidates.append(
            {
                "sha": sha[:8],
                "query": query,
                "gold": {
                    "files": gold["files"],
                    "containers": gold["containers"],
                    "functions": gold["functions"],
                },
                "file_level_only": gold["file_level_only"],
                "verified": False,
            }
        )

    payload = {
        "repo": str(repo),
        "scanned_commits": args.limit,
        "candidate_count": len(candidates),
        "rejected": rejected,
        "candidates": candidates,
    }
    text = json.dumps(payload, indent=2, ensure_ascii=False)
    if args.output == "-":
        print(text)
    else:
        Path(args.output).write_text(text + "\n", encoding="utf-8")
        print(
            f"{len(candidates)} candidates -> {args.output} "
            f"(rejected: {json.dumps(rejected)})"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
