#!/usr/bin/env python3
"""Extract in-domain (this repo) NL→code training pairs from rustdoc comments.

Sources of truth:
- Symbol inventory: .codelens/index/symbols.db (name/kind/signature/name_path/
  file/line exactly as the runtime indexes them).
- NL queries: leading `///` rustdoc blocks in the source files.

Positive text replicates crates/codelens-engine/src/embedding/prompt.rs
build_embedding_text() (filename-only file ctx + parent ctx + module dir ctx),
WITHOUT the docstring suffix — the query is derived from the docstring, so
including it would collapse the pair into a lexical copy.

Output: scripts/finetune/indomain_rust_pairs.jsonl
  {"query", "positive", "language": "rust", "source": "indomain-rustdoc",
   "name", "parent", "file_path", "signature"}
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
DB_PATH = ROOT / ".codelens/index/symbols.db"
OUTPUT = SCRIPT_DIR / "indomain_rust_pairs.jsonl"

MIN_QUERY_LEN = 15
MAX_QUERY_LEN = 300

KINDS = (
    "function",
    "method",
    "class",
    "enum",
    "trait",
    "interface",
    "type_alias",
    "variable",
)


def split_identifier(name: str) -> str:
    """Replicate split_identifier() from embedding/prompt.rs."""
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", part)
            s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
            expanded.extend(s.split())
        return " ".join(w.lower() for w in expanded if w)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
    return " ".join(w.lower() for w in s.split() if w)


def build_embedding_text(
    kind: str, name: str, name_path: str, file_path: str, signature: str
) -> str:
    """Replicate build_embedding_text() from embedding/prompt.rs (sans docstring)."""
    split = split_identifier(name)
    name_with_split = f"{name} ({split})" if split != name else name

    parent_ctx = ""
    if name_path and "/" in name_path:
        parent = name_path.rsplit("/", 1)[0]
        if parent:
            parent_ctx = f" (in {parent})"

    module_ctx = ""
    if "/" in file_path:
        segments = file_path.split("/")
        if len(segments) >= 2:
            dir_name = segments[-2]
            if dir_name not in ("src", "crates"):
                module_ctx = f" [{dir_name}]"

    filename = file_path.rsplit("/", 1)[-1] if file_path else ""
    file_ctx = f" in {filename}" if filename else ""

    if signature:
        return f"{kind} {name_with_split}{parent_ctx}{module_ctx}{file_ctx}: {signature}"
    return f"{kind} {name_with_split}{parent_ctx}{module_ctx}{file_ctx}"


DOC_LINK = re.compile(r"\[`?([^\]`]+)`?\](?:\([^)]*\))?")
TICKS = re.compile(r"`([^`]*)`")


def clean_doc(text: str) -> str:
    text = DOC_LINK.sub(r"\1", text)
    text = TICKS.sub(r"\1", text)
    return " ".join(text.split())


def extract_doc_above(lines: list[str], symbol_line_1based: int) -> str | None:
    """Collect the contiguous `///` block directly above a symbol.

    Attribute lines (`#[...]`, `)]`) between the doc block and the item are
    skipped; a blank or code line terminates the search.
    """
    i = symbol_line_1based - 2  # 0-based line above the symbol
    # Skip attribute lines (incl. simple multi-line attribute tails).
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("#[") or stripped in (")]", "]"):
            i -= 1
            continue
        break
    doc: list[str] = []
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("///"):
            doc.append(stripped[3:].strip())
            i -= 1
            continue
        break
    if not doc:
        return None
    doc.reverse()
    # First paragraph only (up to a blank doc line).
    para: list[str] = []
    for line in doc:
        if not line:
            break
        para.append(line)
    text = clean_doc(" ".join(para))
    return text or None


def is_quality_query(q: str) -> bool:
    if len(q) < MIN_QUERY_LEN:
        return False
    low = q.lower()
    if low.startswith(("todo", "fixme", "note:", "safety:", "panics")):
        return False
    # Mostly-code docs are not NL queries.
    if q.count("{") > 2 or q.count(";") > 2:
        return False
    return True


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", default=str(DB_PATH))
    parser.add_argument("--output", default=str(OUTPUT))
    parser.add_argument("--include-tests", action="store_true")
    args = parser.parse_args()

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    rows = conn.execute(
        """
        SELECT s.name, s.kind, s.signature, s.name_path, s.line, f.relative_path
        FROM symbols s JOIN files f ON s.file_id = f.id
        WHERE f.relative_path LIKE 'crates/%'
          AND f.relative_path LIKE '%.rs'
          AND s.kind IN ({})
        ORDER BY f.relative_path, s.line
        """.format(",".join("?" for _ in KINDS)),
        KINDS,
    ).fetchall()
    conn.close()

    file_cache: dict[str, list[str]] = {}
    pairs = []
    seen: set[tuple[str, str]] = set()
    skipped_no_doc = 0
    skipped_quality = 0

    for name, kind, signature, name_path, line, rel_path in rows:
        if not args.include_tests and (
            "/tests/" in rel_path or rel_path.endswith(("tests.rs", "_tests.rs"))
        ):
            continue
        key = (name_path or name, rel_path)
        if key in seen:
            continue
        seen.add(key)

        if rel_path not in file_cache:
            try:
                file_cache[rel_path] = (
                    (ROOT / rel_path).read_text(encoding="utf-8").splitlines()
                )
            except OSError:
                file_cache[rel_path] = []
        lines = file_cache[rel_path]
        if not lines or line < 1 or line > len(lines):
            continue

        doc = extract_doc_above(lines, line)
        if not doc:
            skipped_no_doc += 1
            continue
        query = doc[:MAX_QUERY_LEN]
        if not is_quality_query(query):
            skipped_quality += 1
            continue

        parent = ""
        if name_path and "/" in name_path:
            parent = name_path.rsplit("/", 1)[0]

        pairs.append(
            {
                "query": query,
                "positive": build_embedding_text(
                    kind, name, name_path or "", rel_path, signature or ""
                ),
                "language": "rust",
                "source": "indomain-rustdoc",
                "name": name,
                "parent": parent,
                "file_path": rel_path,
                "signature": signature or "",
            }
        )

    out = Path(args.output)
    with out.open("w", encoding="utf-8") as fh:
        for pair in pairs:
            fh.write(json.dumps(pair, ensure_ascii=False) + "\n")

    print(f"symbols scanned: {len(rows)}")
    print(f"pairs written:   {len(pairs)} -> {out}")
    print(f"skipped no-doc:  {skipped_no_doc}")
    print(f"skipped quality: {skipped_quality}")
    if pairs:
        sample = pairs[len(pairs) // 2]
        print("sample query:   ", sample["query"][:100])
        print("sample positive:", sample["positive"][:140])


if __name__ == "__main__":
    main()
