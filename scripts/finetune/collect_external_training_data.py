#!/usr/bin/env python3
"""Collect contrastive training pairs from external projects.

Uses CodeLens binary to extract (docstring, embedding_text) pairs from
cloned repos. Produces JSONL in the standard format:
  {"query": "NL docstring", "positive": "embedding text", "negative": "...", "language": "..."}

Usage:
  python3 scripts/finetune/collect_external_training_data.py \
    --repos /tmp/codelens-ext-repos/flask /tmp/codelens-ext-repos/curl \
    --output scripts/finetune/external_training_pairs.jsonl
"""

import argparse
import json
import os
import random
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_BINARY = os.environ.get(
    "CODELENS_BIN", str(ROOT / "target" / "release" / "codelens-mcp")
)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repos",
        nargs="+",
        required=True,
        help="Paths to external project repos",
    )
    parser.add_argument("--binary", default=DEFAULT_BINARY)
    parser.add_argument(
        "--output",
        default=str(SCRIPT_DIR / "external_training_pairs.jsonl"),
    )
    parser.add_argument("--negatives-per-positive", type=int, default=3)
    parser.add_argument("--max-symbols-per-repo", type=int, default=500)
    return parser.parse_args()


def run_tool(binary, project, cmd, args, timeout=120):
    argv = [binary, project, "--cmd", cmd, "--args", json.dumps(args)]
    result = subprocess.run(
        argv, capture_output=True, text=True, timeout=timeout, check=False
    )
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout.strip())
    except json.JSONDecodeError:
        return None


LANG_MAP = {
    "py": "python",
    "js": "javascript",
    "ts": "typescript",
    "tsx": "typescript",
    "go": "go",
    "rs": "rust",
    "java": "java",
    "c": "c",
    "cpp": "cpp",
    "rb": "ruby",
    "php": "php",
    "kt": "kotlin",
    "swift": "swift",
}


# Simple docstring extraction patterns per language
def extract_docstring_from_source(source, line, language):
    """Extract docstring near the symbol definition."""
    lines = source.split("\n")
    if line <= 0 or line > len(lines):
        return ""

    # Python: look for triple-quoted string after def/class line
    if language == "python":
        for i in range(line, min(line + 3, len(lines))):
            stripped = lines[i].strip()
            if stripped.startswith('"""') or stripped.startswith("'''"):
                quote = stripped[:3]
                if stripped.count(quote) >= 2:
                    return stripped.strip(quote).strip()
                # Multi-line
                doc_lines = [stripped.lstrip(quote)]
                for j in range(i + 1, min(i + 10, len(lines))):
                    if quote in lines[j]:
                        doc_lines.append(lines[j].split(quote)[0].strip())
                        return " ".join(l for l in doc_lines if l)
                    doc_lines.append(lines[j].strip())
                return " ".join(l for l in doc_lines if l)

    # C: look for /** ... */ block comment before the symbol
    if language in ("c", "cpp"):
        for i in range(max(0, line - 10), line):
            stripped = lines[i].strip()
            if stripped.startswith("/**"):
                doc_lines = []
                for j in range(i, min(i + 15, len(lines))):
                    l = lines[j].strip().lstrip("/* ").rstrip("*/").strip()
                    if l:
                        doc_lines.append(l)
                    if "*/" in lines[j]:
                        break
                return " ".join(doc_lines)

    # Rust: look for /// comments before the symbol
    if language == "rust":
        doc_lines = []
        for i in range(max(0, line - 15), line):
            stripped = lines[i].strip()
            if stripped.startswith("///"):
                doc_lines.append(stripped.lstrip("/ ").strip())
        return " ".join(doc_lines)

    # JS/TS/Go/Java: JSDoc /** ... */
    for i in range(max(0, line - 10), line):
        stripped = lines[i].strip()
        if stripped.startswith("/**") or stripped.startswith("//"):
            if stripped.startswith("//"):
                return stripped.lstrip("/ ").strip()
            doc_lines = []
            for j in range(i, min(i + 15, len(lines))):
                l = lines[j].strip().lstrip("/* ").rstrip("*/").strip()
                if l:
                    doc_lines.append(l)
                if "*/" in lines[j]:
                    break
            return " ".join(doc_lines)

    return ""


def extract_symbols_with_docs(binary, project, max_symbols):
    """Extract symbols with docstrings using find_symbol + source reading."""
    # Index the project
    run_tool(binary, project, "refresh_symbol_index", {}, timeout=300)

    # Get symbols via semantic_search or find_symbol with common names
    # Better: use get_symbols_overview with each source file
    result = run_tool(binary, project, "get_symbols_overview", {"path": "."})
    if not result:
        return []

    data = result.get("data", result)
    # get_symbols_overview returns nested file→symbols structure
    file_entries = data.get("files", data.get("entries", []))

    # If empty, try find_symbol with broad search
    if not file_entries:
        # Fallback: search common function names
        common = ["main", "init", "new", "create", "get", "set", "run", "start"]
        all_syms = []
        for name in common:
            r = run_tool(binary, project, "find_symbol", {"name": name})
            if r:
                rd = r.get("data", r)
                all_syms.extend(rd.get("symbols", []))
        file_entries = [{"symbols": all_syms}]

    symbols = []
    for entry in file_entries:
        entry_syms = entry.get("symbols", [])
        for sym in entry_syms:
            name = sym.get("name", "")
            kind = sym.get("kind", "")
            sig = sym.get("signature", "")
            fp = sym.get("file_path", sym.get("file", ""))
            line = sym.get("line", 0)

            if kind in ("variable", "constant", "import"):
                continue

            ext = Path(fp).suffix.lstrip(".")
            language = LANG_MAP.get(ext, ext)

            # Read source to extract docstring
            source_path = Path(project) / fp
            if source_path.exists():
                try:
                    source = source_path.read_text(encoding="utf-8", errors="ignore")
                    doc = extract_docstring_from_source(source, line - 1, language)
                except Exception:
                    doc = ""
            else:
                doc = ""

            if not doc or len(doc.strip()) < 10:
                continue

            first_line = doc.strip().split("\n")[0].strip()[:200]
            if len(first_line) < 10:
                continue

            filename = Path(fp).name
            parts = fp.rsplit("/", 2)
            module = ""
            if len(parts) >= 2:
                d = parts[-2] if len(parts) == 2 else parts[1]
                if d not in ("src", "crates", "lib", "internal"):
                    module = f" [{d}]"

            embedding_text = f"{kind} {name}{module} in {filename}"
            if sig:
                embedding_text += f": {sig}"

            symbols.append(
                {
                    "query": first_line,
                    "positive": embedding_text,
                    "language": language,
                    "name": name,
                    "file": fp,
                }
            )

            if len(symbols) >= max_symbols:
                return symbols

    return symbols


def generate_hard_negatives(symbols, negatives_per_positive):
    """Generate hard negatives: same-language, different symbol."""
    by_lang = {}
    for sym in symbols:
        lang = sym.get("language", "unknown")
        by_lang.setdefault(lang, []).append(sym)

    pairs = []
    for sym in symbols:
        lang = sym["language"]
        pool = [s for s in by_lang.get(lang, []) if s["name"] != sym["name"]]
        if not pool:
            pool = [s for s in symbols if s["name"] != sym["name"]]
        if not pool:
            continue

        negatives = random.sample(pool, min(negatives_per_positive, len(pool)))
        for neg in negatives:
            pairs.append(
                {
                    "query": sym["query"],
                    "positive": sym["positive"],
                    "negative": neg["positive"],
                    "language": sym["language"],
                }
            )

    return pairs


def main():
    args = parse_args()
    all_symbols = []

    for repo in args.repos:
        repo = os.path.abspath(repo)
        if not os.path.isdir(repo):
            print(f"SKIP: {repo} not found", file=sys.stderr)
            continue

        print(f"Extracting from {repo}...", file=sys.stderr)
        symbols = extract_symbols_with_docs(
            args.binary, repo, args.max_symbols_per_repo
        )
        print(f"  Found {len(symbols)} symbols with docs", file=sys.stderr)
        all_symbols.extend(symbols)

    if not all_symbols:
        print("No symbols found. Check repos and binary.", file=sys.stderr)
        sys.exit(1)

    pairs = generate_hard_negatives(all_symbols, args.negatives_per_positive)
    print(f"Generated {len(pairs)} training pairs", file=sys.stderr)

    # Also include positive-only pairs (no negative)
    positive_only = [
        {"query": s["query"], "positive": s["positive"], "language": s["language"]}
        for s in all_symbols
    ]

    output = Path(args.output)
    with output.open("w", encoding="utf-8") as f:
        for pair in pairs:
            f.write(json.dumps(pair, ensure_ascii=False) + "\n")
        for p in positive_only:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    print(f"Wrote {len(pairs) + len(positive_only)} entries to {output}")

    # Stats
    by_lang = {}
    for s in all_symbols:
        lang = s["language"]
        by_lang[lang] = by_lang.get(lang, 0) + 1
    print("Language distribution:")
    for lang, count in sorted(by_lang.items(), key=lambda x: -x[1]):
        print(f"  {lang}: {count}")


if __name__ == "__main__":
    main()
