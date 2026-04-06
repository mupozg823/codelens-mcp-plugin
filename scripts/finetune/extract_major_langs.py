#!/usr/bin/env python3
"""Extract docstring+function pairs from real projects for languages missing from CSN.

Target: TypeScript, Rust, C/C++
Output: Runtime embedding format with real file paths.
"""

import json
import os
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT = SCRIPT_DIR / "major_langs_extra.jsonl"


def split_identifier(name: str) -> str:
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


def build_positive(
    name: str, kind: str, sig: str, file_path: str, doc: str = ""
) -> str:
    split = split_identifier(name)
    name_fmt = f"{name} ({split})" if split != name.lower() else name
    file_ctx = f" in {file_path}" if file_path else ""
    base = (
        f"{kind} {name_fmt}{file_ctx}: {sig}" if sig else f"{kind} {name_fmt}{file_ctx}"
    )
    if doc:
        first_line = doc.strip().split("\n")[0].strip()
        if len(first_line) > 60:
            first_line = first_line[:60] + "..."
        if len(first_line) > 10:
            return f"{base} — {first_line}"
    return base


def extract_rust(project_dir: str, max_pairs: int = 5000) -> list[dict]:
    """Extract from Rust source files using regex (doc comments + fn)."""
    pairs = []
    root = Path(project_dir)
    for rs_file in root.rglob("*.rs"):
        if "target/" in str(rs_file) or "test" in rs_file.name:
            continue
        try:
            content = rs_file.read_text()
        except Exception:
            continue

        rel_path = str(rs_file.relative_to(root))

        # Find /// doc comments followed by fn/pub fn
        pattern = r"((?:\s*///.*\n)+)\s*(?:pub(?:\(crate\))?\s+)?fn\s+(\w+)\s*([^{]*)"
        for m in re.finditer(pattern, content):
            doc_lines = m.group(1).strip()
            name = m.group(2)
            sig = f"fn {name}{m.group(3).strip()}"[:150]

            # Clean doc comment
            doc = "\n".join(
                line.strip().lstrip("/").strip() for line in doc_lines.split("\n")
            ).strip()

            if not doc or len(doc) < 15 or name.startswith("test"):
                continue

            positive = build_positive(name, "function", sig, rel_path, doc)
            pairs.append({"query": doc[:300], "positive": positive, "language": "rust"})
            if len(pairs) >= max_pairs:
                return pairs
    return pairs


def extract_typescript(project_dir: str, max_pairs: int = 5000) -> list[dict]:
    """Extract from TypeScript source files using regex (JSDoc + function/method)."""
    pairs = []
    root = Path(project_dir)
    for ts_file in root.rglob("*.ts"):
        if "node_modules/" in str(ts_file) or "dist/" in str(ts_file):
            continue
        try:
            content = ts_file.read_text()
        except Exception:
            continue

        rel_path = str(ts_file.relative_to(root))

        # JSDoc comment followed by function/method
        pattern = r"/\*\*\s*(.*?)\*/\s*(?:export\s+)?(?:async\s+)?(?:function\s+(\w+)|(\w+)\s*[=(]\s*(?:async\s+)?(?:function|\([^)]*\)\s*(?:=>|:)))"
        for m in re.finditer(pattern, content, re.DOTALL):
            doc_raw = m.group(1)
            name = m.group(2) or m.group(3)
            if not name:
                continue

            # Clean JSDoc
            doc = "\n".join(
                line.strip().lstrip("*").strip() for line in doc_raw.split("\n")
            ).strip()
            # Remove @param, @returns etc for query
            query_lines = [l for l in doc.split("\n") if not l.strip().startswith("@")]
            query = "\n".join(query_lines).strip()

            if not query or len(query) < 15:
                continue

            # Get signature line
            sig_start = m.start()
            sig_end = content.find("{", m.end())
            if sig_end == -1:
                sig_end = m.end() + 100
            sig = content[m.end() - len(name) - 20 : sig_end].strip()[:150]

            positive = build_positive(name, "function", sig, rel_path, query)
            pairs.append(
                {"query": query[:300], "positive": positive, "language": "typescript"}
            )
            if len(pairs) >= max_pairs:
                return pairs
    return pairs


def extract_c(project_dir: str, max_pairs: int = 5000) -> list[dict]:
    """Extract from C source/header files (/* */ comments + function defs)."""
    pairs = []
    root = Path(project_dir)
    for c_file in root.rglob("*.[ch]"):
        if "test" in str(c_file).lower():
            continue
        try:
            content = c_file.read_text()
        except Exception:
            continue

        rel_path = str(c_file.relative_to(root))

        # Block comment followed by function definition
        pattern = r"/\*\*?\s*(.*?)\*/\s*(?:static\s+|extern\s+)?(?:(?:unsigned|signed|const|struct|enum|void|int|long|char|float|double|size_t|bool|CURLcode|\w+_t)\s+\*?\s*)(\w+)\s*\([^)]*\)"
        for m in re.finditer(pattern, content, re.DOTALL):
            doc_raw = m.group(1)
            name = m.group(2)
            if not name or name in ("if", "while", "for", "switch"):
                continue

            doc = "\n".join(
                line.strip().lstrip("*").strip() for line in doc_raw.split("\n")
            ).strip()

            if not doc or len(doc) < 15:
                continue

            sig_start = m.start(2) - 20
            sig_end = content.find("{", m.end())
            if sig_end == -1:
                sig_end = m.end() + 50
            sig = content[max(0, sig_start) : min(sig_end, sig_start + 150)].strip()

            positive = build_positive(name, "function", sig, rel_path, doc)
            pairs.append({"query": doc[:300], "positive": positive, "language": "c"})
            if len(pairs) >= max_pairs:
                return pairs
    return pairs


def main():
    all_pairs = []

    # Rust — from CodeLens itself + any other Rust projects
    print("=== Rust ===")
    rust_pairs = extract_rust("/Users/bagjaeseog/codelens-mcp-plugin")
    print(f"  CodeLens: {len(rust_pairs)}")
    all_pairs.extend(rust_pairs)

    # TypeScript — from claw-dev
    print("=== TypeScript ===")
    ts_pairs = extract_typescript(
        "/Users/bagjaeseog/Downloads/claudex/claw-dev/Leonxlnx-claude-code/src"
    )
    print(f"  claw-dev/src: {len(ts_pairs)}")
    all_pairs.extend(ts_pairs)

    # C — from curl
    print("=== C ===")
    if Path("/tmp/curl-test").exists():
        c_pairs = extract_c("/tmp/curl-test")
        print(f"  curl: {len(c_pairs)}")
        all_pairs.extend(c_pairs)
    else:
        print("  curl not found, skipping")

    # Deduplicate
    seen = set()
    deduped = []
    for p in all_pairs:
        key = (p["query"][:50], p["positive"][:50])
        if key not in seen:
            seen.add(key)
            deduped.append(p)

    import random

    random.seed(42)
    random.shuffle(deduped)

    with OUTPUT.open("w") as f:
        for p in deduped:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    from collections import Counter

    langs = Counter(p["language"] for p in deduped)
    print(f"\n=== Total: {len(deduped)} pairs ===")
    for lang, cnt in langs.most_common():
        print(f"  {lang}: {cnt}")


if __name__ == "__main__":
    main()
