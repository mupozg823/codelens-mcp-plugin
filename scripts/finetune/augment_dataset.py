#!/usr/bin/env python3
"""Augment the embedding quality dataset with auto-generated query-symbol pairs.

Generates natural language queries from symbol signatures using templates.
This expands the 24 curated queries to 100+ for better fine-tuning coverage.

Output: benchmarks/embedding-quality-dataset-augmented.json
"""

import json
import os
import random
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
ORIGINAL_DATASET = ROOT / "benchmarks" / "embedding-quality-dataset.json"
OUTPUT = ROOT / "benchmarks" / "embedding-quality-dataset-augmented.json"

# Templates for generating natural language queries from symbols
NL_TEMPLATES = [
    "how does {name} work",
    "what does {name} do",
    "find the {kind} that handles {topic}",
    "where is {topic} implemented",
    "code that {action}",
    "{topic} implementation",
    "the {kind} responsible for {topic}",
    "{action} logic",
]

# Map symbol kinds to natural topics
KIND_ACTIONS = {
    "function": [
        ("processes", "processing"),
        ("handles", "handling"),
        ("computes", "computation"),
        ("builds", "building"),
        ("parses", "parsing"),
        ("creates", "creation"),
        ("validates", "validation"),
        ("loads", "loading"),
        ("resolves", "resolution"),
        ("dispatches", "dispatching"),
    ],
    "struct": [
        ("stores", "storage"),
        ("represents", "representation"),
        ("manages", "management"),
        ("tracks", "tracking"),
    ],
    "enum": [
        ("categorizes", "categorization"),
        ("represents", "variants"),
    ],
    "impl": [
        ("implements", "implementation"),
    ],
}


# Words to extract topic from symbol name
def name_to_topic(name):
    """Convert CamelCase/snake_case to space-separated topic words."""
    import re

    # Split CamelCase
    words = re.sub(r"([A-Z])", r" \1", name).strip().split()
    # Split snake_case
    expanded = []
    for w in words:
        expanded.extend(w.split("_"))
    # Filter short words and lowercase
    topic_words = [w.lower() for w in expanded if len(w) > 2]
    return " ".join(topic_words)


def generate_queries(sym):
    """Generate diverse queries for a single symbol."""
    name = sym.get("name", "")
    kind = sym.get("kind", "function")
    file_path = sym.get("file", sym.get("file_path", ""))
    signature = sym.get("signature", "")

    topic = name_to_topic(name)
    if not topic or len(topic) < 4:
        return []

    queries = []
    actions = KIND_ACTIONS.get(kind, KIND_ACTIONS["function"])

    # Template-based queries
    for action_verb, action_noun in random.sample(actions, min(2, len(actions))):
        queries.append(
            {
                "query": f"find the {kind} that {action_verb} {topic}",
                "query_type": "natural_language",
                "expected_symbol": name,
                "expected_file_suffix": file_path,
            }
        )
        queries.append(
            {
                "query": f"{topic} {action_noun}",
                "query_type": "short_phrase",
                "expected_symbol": name,
                "expected_file_suffix": file_path,
            }
        )

    # Direct natural language
    queries.append(
        {
            "query": f"how does {topic} work",
            "query_type": "natural_language",
            "expected_symbol": name,
            "expected_file_suffix": file_path,
        }
    )

    # Identifier query (should always work)
    queries.append(
        {
            "query": name,
            "query_type": "identifier",
            "expected_symbol": name,
            "expected_file_suffix": file_path,
        }
    )

    return queries


def run_tool(binary, project, cmd, args, timeout=30):
    argv = [
        binary,
        project,
        "--preset",
        "full",
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return None


def get_important_symbols(binary, project):
    """Get high-importance symbols for query generation."""
    symbols = []

    # Get important files via PageRank
    resp = run_tool(binary, project, "get_symbol_importance", {"top_n": 20})
    if resp and resp.get("success") and resp.get("data"):
        files = resp["data"].get("ranking", resp["data"].get("files", []))
        for f in files:
            path = f.get("file", f.get("path", ""))
            if not path or path.endswith(".py"):
                continue
            # Get symbols from each important file
            resp2 = run_tool(binary, project, "get_symbols_overview", {"path": path})
            if resp2 and resp2.get("success") and resp2.get("data"):
                syms = resp2["data"].get("symbols", resp2["data"].get("results", []))
                for sym in syms:
                    sym["file"] = path
                    symbols.append(sym)

    return symbols


def main():
    binary = os.environ.get(
        "CODELENS_BIN", str(ROOT / "target" / "release" / "codelens-mcp")
    )

    # Load original dataset
    original = []
    if ORIGINAL_DATASET.exists():
        with open(ORIGINAL_DATASET) as f:
            original = json.load(f)

    existing_queries = {e["query"] for e in original}
    print(f"Original dataset: {len(original)} queries")

    # Get important symbols
    symbols = get_important_symbols(binary, str(ROOT))
    print(f"Found {len(symbols)} symbols from important files")

    # Generate queries
    generated = []
    for sym in symbols:
        new_queries = generate_queries(sym)
        for q in new_queries:
            if q["query"] not in existing_queries:
                generated.append(q)
                existing_queries.add(q["query"])

    print(f"Generated {len(generated)} new queries")

    # Combine
    augmented = original + generated
    random.shuffle(augmented)

    with open(OUTPUT, "w") as f:
        json.dump(augmented, f, indent=2, ensure_ascii=False)

    print(f"\nTotal: {len(augmented)} queries → {OUTPUT}")
    print(
        f"  identifier: {sum(1 for q in augmented if q['query_type'] == 'identifier')}"
    )
    print(
        f"  natural_language: {sum(1 for q in augmented if q['query_type'] == 'natural_language')}"
    )
    print(
        f"  short_phrase: {sum(1 for q in augmented if q['query_type'] == 'short_phrase')}"
    )


if __name__ == "__main__":
    main()
