#!/usr/bin/env python3
"""NL query augmentation for semantic_search MRR improvement.

Problem: Training data has verbose docstrings, but real queries are short
imperative NL ("parse source code into AST", "find similar code").

This script:
1. Extracts short first-sentence NL queries from existing docstrings
2. Generates imperative-style queries from function names
3. Cleans comment prefixes and annotation noise
4. Outputs augmented pairs in the same JSONL format

Usage:
    python3 scripts/finetune/build_nl_augmentation.py \
        --csn scripts/finetune/csn_runtime_format.jsonl \
        --codexglue scripts/finetune/codexglue_train.jsonl \
        --output scripts/finetune/nl_augmented_pairs.jsonl \
        --stats
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent

# ---------------------------------------------------------------------------
# Query cleaning
# ---------------------------------------------------------------------------

COMMENT_PREFIX_RE = re.compile(r"^(?:\s*(?://+|#+|/\*+|\*+|\"\"\"|\'\'\'))\s*")
ANNOTATION_RE = re.compile(
    r"@(?:param|return|returns|throws|exception|since|deprecated|see|link|"
    r"brief|details|note|warning|author|version|override|inheritdoc|"
    r"type|property|example|code|endcode)\b.*",
    re.IGNORECASE,
)
HTML_TAG_RE = re.compile(r"</?[a-zA-Z][^>]*>")
JAVADOC_INLINE_RE = re.compile(r"\{@\w+\s+([^}]*)\}")


def clean_docstring(raw: str) -> str:
    """Extract a clean, short NL query from a verbose docstring."""
    # Remove comment prefixes
    lines = raw.strip().split("\n")
    cleaned_lines = []
    for line in lines:
        line = COMMENT_PREFIX_RE.sub("", line).strip()
        line = line.rstrip("*/").strip()
        if not line:
            continue
        # Stop at annotations
        if ANNOTATION_RE.match(line):
            break
        # Stop at blank-looking content
        if line.startswith("@"):
            break
        cleaned_lines.append(line)

    if not cleaned_lines:
        return ""

    text = " ".join(cleaned_lines)

    # Remove inline javadoc tags: {@link Foo} → Foo
    text = JAVADOC_INLINE_RE.sub(r"\1", text)
    # Remove HTML tags
    text = HTML_TAG_RE.sub("", text)
    # Collapse whitespace
    text = re.sub(r"\s+", " ", text).strip()

    # Extract first sentence
    # Split on ". " or ".\n" but not on abbreviations like "e.g."
    first_sentence = re.split(r"(?<=[^A-Z])\.\s", text, maxsplit=1)[0]
    first_sentence = first_sentence.rstrip(".")

    # Skip if too short or too long
    words = first_sentence.split()
    if len(words) < 3 or len(words) > 15:
        return ""

    return first_sentence


def to_imperative(declarative: str) -> str:
    """Convert declarative docstring to imperative style.

    "Returns the struct name" → "get the struct name"
    "Checks if X is valid" → "check if X is valid"
    "Creates a new instance" → "create a new instance"
    """
    text = declarative.strip()
    if not text:
        return ""

    # Common verb transformations (3rd person → imperative)
    patterns = [
        (r"^[Rr]eturns?\s+", "get "),
        (r"^[Gg]ets?\s+", "get "),
        (r"^[Ss]ets?\s+", "set "),
        (r"^[Cc]hecks?\s+", "check "),
        (r"^[Cc]reates?\s+", "create "),
        (r"^[Ff]inds?\s+", "find "),
        (r"^[Bb]uilds?\s+", "build "),
        (r"^[Pp]arses?\s+", "parse "),
        (r"^[Cc]omputes?\s+", "compute "),
        (r"^[Cc]alculates?\s+", "calculate "),
        (r"^[Gg]enerates?\s+", "generate "),
        (r"^[Dd]etermines?\s+", "determine "),
        (r"^[Vv]alidates?\s+", "validate "),
        (r"^[Ii]nitializ(?:es?|ing)\s+", "initialize "),
        (r"^[Cc]onverts?\s+", "convert "),
        (r"^[Ee]xtracts?\s+", "extract "),
        (r"^[Rr]eads?\s+", "read "),
        (r"^[Ww]rites?\s+", "write "),
        (r"^[Dd]eletes?\s+", "delete "),
        (r"^[Uu]pdates?\s+", "update "),
        (r"^[Aa]dds?\s+", "add "),
        (r"^[Rr]emoves?\s+", "remove "),
        (r"^[Cc]ounts?\s+", "count "),
        (r"^[Ll]oads?\s+", "load "),
        (r"^[Ss]aves?\s+", "save "),
        (r"^[Ss]ends?\s+", "send "),
        (r"^[Rr]eceives?\s+", "receive "),
        (r"^[Pp]rocesses?\s+", "process "),
        (r"^[Hh]andles?\s+", "handle "),
        (r"^[Rr]uns?\s+", "run "),
        (r"^[Ss]tarts?\s+", "start "),
        (r"^[Ss]tops?\s+", "stop "),
        (r"^[Rr]esolves?\s+", "resolve "),
        (r"^[Mm]erges?\s+", "merge "),
        (r"^[Ss]plits?\s+", "split "),
        (r"^[Ff]ilters?\s+", "filter "),
        (r"^[Ss]orts?\s+", "sort "),
        (r"^[Ff]ormats?\s+", "format "),
        (r"^[Ee]ncodes?\s+", "encode "),
        (r"^[Dd]ecodes?\s+", "decode "),
        (r"^[Rr]egisters?\s+", "register "),
        (r"^[Rr]enders?\s+", "render "),
        (r"^[Aa]pplies\s+", "apply "),
        (r"^[Aa]pply\s+", "apply "),
        (r"^[Dd]ispatches\s+", "dispatch "),
        (r"^[Dd]ispatch\s+", "dispatch "),
    ]

    for pattern, replacement in patterns:
        new_text, count = re.subn(pattern, replacement, text, count=1)
        if count:
            return new_text

    # If starts with a verb already (lowercase), keep as is
    first_word = text.split()[0] if text else ""
    if first_word and first_word[0].islower():
        return text

    # Lowercase first char for imperative style
    return text[0].lower() + text[1:] if text else ""


# ---------------------------------------------------------------------------
# Identifier-based NL queries
# ---------------------------------------------------------------------------


def split_identifier(name: str) -> str:
    """Split camelCase/snake_case into words."""
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", part)
            spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
            expanded.extend(spaced.split())
        return " ".join(word.lower() for word in expanded if word)
    spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
    return " ".join(word.lower() for word in spaced.split() if word)


def identifier_to_nl(name: str, kind: str) -> str | None:
    """Generate a short NL query from identifier name.

    "parse_symbols" → "parse symbols from source code"
    "build_embedding_text" → "build embedding text"
    "find_duplicates" → "find duplicates in code"
    """
    words = split_identifier(name).split()
    if len(words) < 2 or len(words) > 6:
        return None

    phrase = " ".join(words)

    # Add context based on kind
    if kind == "function" and len(words) <= 3:
        # Short function names benefit from context
        return phrase
    elif kind == "class":
        return phrase
    else:
        return phrase


# ---------------------------------------------------------------------------
# Main augmentation pipeline
# ---------------------------------------------------------------------------


def iter_jsonl(path: Path):
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                yield json.loads(line)


def augment_from_docstrings(pairs_path: Path, stats: Counter) -> list[dict]:
    """Generate short NL queries from existing training data docstrings."""
    augmented = []
    seen_queries = set()

    for obj in iter_jsonl(pairs_path):
        raw_query = obj.get("query", "")
        positive = obj.get("positive", "")
        language = obj.get("language", "unknown")

        if not raw_query or not positive:
            continue

        stats["total_input"] += 1

        # 1. Clean docstring → short NL
        short_query = clean_docstring(raw_query)
        if short_query and short_query.lower() not in seen_queries:
            seen_queries.add(short_query.lower())
            augmented.append(
                {
                    "query": short_query,
                    "positive": positive,
                    "language": language,
                    "augmentation": "first_sentence",
                }
            )
            stats["first_sentence"] += 1

        # 2. Imperative rewrite
        if short_query:
            imperative = to_imperative(short_query)
            if (
                imperative
                and imperative != short_query
                and imperative.lower() not in seen_queries
            ):
                seen_queries.add(imperative.lower())
                augmented.append(
                    {
                        "query": imperative,
                        "positive": positive,
                        "language": language,
                        "augmentation": "imperative",
                    }
                )
                stats["imperative"] += 1

        # 3. Identifier-based NL from positive
        # Parse "function addDependencyToGraph (add dependency to graph) in ..."
        name_match = re.match(
            r"(?:function|class|method|variable|struct|enum|trait|interface)\s+"
            r"(\w+)",
            positive,
        )
        if name_match:
            name = name_match.group(1)
            kind_match = re.match(r"(\w+)", positive)
            kind = kind_match.group(1) if kind_match else "function"
            nl_query = identifier_to_nl(name, kind)
            if nl_query and nl_query.lower() not in seen_queries:
                seen_queries.add(nl_query.lower())
                augmented.append(
                    {
                        "query": nl_query,
                        "positive": positive,
                        "language": language,
                        "augmentation": "identifier_nl",
                    }
                )
                stats["identifier_nl"] += 1

    return augmented


# ---------------------------------------------------------------------------
# LLM synthetic query stage (P2)
#
# E5-Mistral 2-step synthesis: (1) brainstorm retrieval task types for a code
# snippet, then (2) generate one natural-language query per task type. This runs
# behind a pluggable generator so real backends ('anthropic', 'local') can be
# added later. The default 'stub' backend performs NO model or network call — it
# only emits a plan of (snippet, prompt, expected schema) records for a real,
# user-approved generation pass. --dry-run therefore stays free of any ML/network
# dependency (stdlib only).
# ---------------------------------------------------------------------------

SYNTH_PLAN_SCHEMA_VERSION = "codelens-nl-synth-plan-v1"

BRAINSTORM_PROMPT_TEMPLATE = (
    "You are curating retrieval training data for a code search model.\n"
    "Given the code snippet below, brainstorm a short list of distinct retrieval "
    "task types a developer might use to find it (for example: 'natural language "
    "intent', 'error-symptom lookup', 'API-usage example').\n\n"
    "Snippet ({language}):\n{snippet}\n\n"
    "Return a JSON list of task-type strings."
)

QUERY_PROMPT_TEMPLATE = (
    "For the retrieval task type '{task_type}', write ONE concise natural-language "
    "search query (3-15 words) a developer would type to find the code snippet "
    "below. Return only the query text.\n\n"
    "Snippet ({language}):\n{snippet}"
)

# Contract a real backend must satisfy when it fills `generated_query`.
EXPECTED_QUERY_SCHEMA = {
    "query": "str",
    "positive": "str",
    "language": "str",
    "augmentation": "llm_synth",
    "task_type": "str",
    "backend": "str",
}


class SynthGenerator:
    """Pluggable NL-query synthesis backend interface.

    Backends implement the same two-method signature:

        brainstorm_task_types(snippet, language) -> list[str]
        generate(snippet, task_type, prompt)     -> str | None

    The 'stub' backend never calls a model; it exists so the pipeline shape is
    validated in CI. Real backends ('anthropic', 'local') plug in behind this
    interface and are only invoked outside --dry-run after explicit user
    approval.
    """

    name = "base"

    def brainstorm_task_types(self, snippet: str, language: str) -> list[str]:
        raise NotImplementedError

    def generate(self, snippet: str, task_type: str, prompt: str) -> str | None:
        raise NotImplementedError


class StubSynthGenerator(SynthGenerator):
    name = "stub"

    # Deterministic task-type seeds shape the plan without an LLM.
    DEFAULT_TASK_TYPES = (
        "natural_language_intent",
        "error_symptom_lookup",
        "api_usage_example",
    )

    def brainstorm_task_types(self, snippet: str, language: str) -> list[str]:
        return list(self.DEFAULT_TASK_TYPES)

    def generate(self, snippet: str, task_type: str, prompt: str) -> str | None:
        # Stub emits no query — real generation happens in a separate,
        # user-approved run. Returning None keeps --dry-run output LLM-free.
        return None


def synth_generator(backend: str) -> SynthGenerator:
    """Return the query-synthesis backend.

    Only 'stub' is wired for dry-run/CI. Future 'anthropic'/'local' backends
    subclass SynthGenerator and are dispatched here.
    """
    if backend == "stub":
        return StubSynthGenerator()
    raise SystemExit(
        f"synth backend '{backend}' is not wired; only 'stub' is available for "
        "dry-run. Real LLM backends run separately after user approval."
    )


def passes_quality_filters(query: str | None, seen: set[str]) -> bool:
    """Quality gate hook for synthesized queries (length / language / dedup).

    Wires the filter *positions* a real backend run enforces. In stub/dry-run
    there are no generated queries, so this is exercised only once a real
    backend fills `generated_query`.
    """
    if not query:
        return False
    words = query.split()
    if len(words) < 3 or len(words) > 15:  # length filter
        return False
    # TODO(P2): plug language detection (e.g. fasttext/langid) here to drop
    # non-English synthesized queries before dedup.
    key = query.lower()
    if key in seen:  # dedup filter
        return False
    seen.add(key)
    return True


def build_synth_plan(
    source_path: Path,
    generator: SynthGenerator,
    *,
    max_snippets: int = 0,
) -> list[dict]:
    """Plan LLM-synthesized NL queries via the E5-Mistral 2-step pattern.

    Emits one record per (snippet, task_type) with both prompts and the expected
    output schema. `generated_query` is None under the stub backend; a real
    backend fills it and the caller runs `passes_quality_filters` before keeping
    it.
    """
    plan: list[dict] = []
    seen_queries: set[str] = set()
    for index, obj in enumerate(iter_jsonl(source_path)):
        if max_snippets and index >= max_snippets:
            break
        positive = obj.get("positive", "")
        language = obj.get("language", "unknown")
        if not positive:
            continue
        for task_type in generator.brainstorm_task_types(positive, language):
            query_prompt = QUERY_PROMPT_TEMPLATE.format(
                task_type=task_type, snippet=positive, language=language
            )
            candidate = generator.generate(positive, task_type, query_prompt)
            kept = passes_quality_filters(candidate, seen_queries)
            plan.append(
                {
                    "snippet": positive,
                    "language": language,
                    "task_type": task_type,
                    "backend": generator.name,
                    "brainstorm_prompt": BRAINSTORM_PROMPT_TEMPLATE.format(
                        snippet=positive, language=language
                    ),
                    "query_prompt": query_prompt,
                    "expected_schema": EXPECTED_QUERY_SCHEMA,
                    "generated_query": candidate,  # None under the stub backend
                    "quality_passed": kept,
                }
            )
    return plan


def run_synth_stage(args) -> Path:
    generator = synth_generator(args.synth_backend)
    source = args.synth_input
    if not source.exists():
        raise SystemExit(f"synth input not found: {source}")
    plan = build_synth_plan(
        source, generator, max_snippets=args.synth_max_snippets
    )
    output = args.synth_output
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "w", encoding="utf-8") as handle:
        for record in plan:
            handle.write(json.dumps(record, ensure_ascii=False) + "\n")
    print(
        f"[synth] schema={SYNTH_PLAN_SCHEMA_VERSION} backend={generator.name} "
        f"dry_run={args.dry_run} records_planned={len(plan)} -> {output}"
    )
    return output


def parse_args():
    parser = argparse.ArgumentParser(
        description="NL query augmentation for MRR improvement"
    )
    parser.add_argument(
        "--csn", type=Path, default=SCRIPT_DIR / "csn_runtime_format.jsonl"
    )
    parser.add_argument(
        "--codexglue", type=Path, default=SCRIPT_DIR / "codexglue_train.jsonl"
    )
    parser.add_argument(
        "--output", type=Path, default=SCRIPT_DIR / "nl_augmented_pairs.jsonl"
    )
    parser.add_argument("--stats", action="store_true", help="Print statistics")
    parser.add_argument(
        "--max-per-source", type=int, default=50000, help="Max pairs per source file"
    )
    # LLM synthetic query stage (P2)
    parser.add_argument(
        "--synth-queries",
        action="store_true",
        help="Run the LLM synthetic-query planning stage instead of the "
        "rule-based augmentation flow.",
    )
    parser.add_argument(
        "--synth-backend",
        default="stub",
        help="Synthesis backend (only 'stub' is wired for dry-run/CI).",
    )
    parser.add_argument(
        "--synth-input",
        type=Path,
        default=SCRIPT_DIR / "synthetic_nl_pairs.jsonl",
        help="Source JSONL of {query, positive} rows to plan synthesis over.",
    )
    parser.add_argument(
        "--synth-output",
        type=Path,
        default=SCRIPT_DIR / "output" / "nl-synth-plan.jsonl",
        help="Where to write the synthesis plan JSONL.",
    )
    parser.add_argument(
        "--synth-max-snippets",
        type=int,
        default=200,
        help="Cap snippets planned (0 = all).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Plan-only: emit the synthesis plan without any LLM/network call.",
    )
    return parser.parse_args()


def main():
    args = parse_args()

    if args.synth_queries:
        run_synth_stage(args)
        if args.dry_run:
            return
        # Non-dry-run real generation is gated separately (user-approved
        # backend); the stub backend produces no queries to merge here.
        return

    all_augmented = []
    stats = Counter()

    sources = []
    if args.csn.exists():
        sources.append(("CSN", args.csn))
    if args.codexglue.exists():
        sources.append(("CodexGLUE", args.codexglue))

    for source_name, path in sources:
        print(f"\n=== Processing {source_name}: {path} ===")
        source_stats = Counter()
        pairs = augment_from_docstrings(path, source_stats)

        if len(pairs) > args.max_per_source:
            import random

            random.seed(42)
            random.shuffle(pairs)
            pairs = pairs[: args.max_per_source]

        all_augmented.extend(pairs)
        for k, v in source_stats.items():
            stats[f"{source_name}_{k}"] = v

        if args.stats:
            print(f"  Input pairs: {source_stats['total_input']}")
            print(f"  First sentence: {source_stats['first_sentence']}")
            print(f"  Imperative: {source_stats['imperative']}")
            print(f"  Identifier NL: {source_stats['identifier_nl']}")
            print(f"  Total augmented: {len(pairs)}")

    # Deduplicate across sources
    seen = set()
    deduped = []
    for pair in all_augmented:
        key = pair["query"].lower()
        if key not in seen:
            seen.add(key)
            deduped.append(pair)

    # Write output
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        for pair in deduped:
            f.write(json.dumps(pair, ensure_ascii=False) + "\n")

    print(f"\n=== Final Output ===")
    print(f"Total augmented pairs: {len(deduped)}")
    print(f"Written to: {args.output}")

    if args.stats:
        # Query length distribution
        lengths = [len(p["query"].split()) for p in deduped]
        import statistics as st

        print(f"\nQuery length stats:")
        print(f"  Mean: {st.mean(lengths):.1f} words")
        print(f"  Median: {st.median(lengths):.1f} words")
        print(
            f"  <= 6 words: {sum(1 for l in lengths if l <= 6)} ({sum(1 for l in lengths if l <= 6)/len(lengths)*100:.1f}%)"
        )
        print(
            f"  <= 10 words: {sum(1 for l in lengths if l <= 10)} ({sum(1 for l in lengths if l <= 10)/len(lengths)*100:.1f}%)"
        )

        # By augmentation type
        by_type = Counter(p["augmentation"] for p in deduped)
        print(f"\nBy augmentation type:")
        for t, c in by_type.most_common():
            print(f"  {t}: {c}")


if __name__ == "__main__":
    main()
