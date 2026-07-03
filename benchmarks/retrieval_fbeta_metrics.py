#!/usr/bin/env python3
"""Weighted F-beta retrieval metrics (SWE-grep pattern).

Pure, stdlib-only, importable helpers so they can be unit-tested without a
running daemon. The entrypoint (``retrieval-fbeta.py``) imports these to score
live retrieval candidates against the existing file/symbol labels.

Design note: the benchmark datasets label each query with a single
``expected_symbol`` inside an ``expected_file_suffix`` and carry NO explicit
line-range column. We therefore derive the gold line for the strict line-level
metric from the authoritative definition line of that symbol (resolved live via
``find_symbol``), and treat file-level vs line-level as two granularities of the
same precision-first score:

- **file-level**  — a retrieved candidate counts as relevant when its file path
  ends with a gold ``expected_file_suffix`` (right file, any symbol). Lenient.
- **line-level**  — a retrieved candidate counts as relevant only when its file
  matches AND its definition line equals the gold line (exact location). Strict;
  this is where context pollution (right file, wrong lines) is penalized.

Beta < 1 weights precision over recall (beta=0.5 => precision is 2x as
important), matching SWE-grep's penalty for returning noisy context.
"""

from __future__ import annotations


def weighted_f_beta(precision: float, recall: float, beta: float = 0.5) -> float:
    """Weighted F-beta. beta<1 favors precision, beta>1 favors recall.

    Returns a value in [0.0, 1.0]. Degenerate denominator (both weighted
    precision and recall zero) yields 0.0 rather than dividing by zero.
    """
    if beta <= 0:
        raise ValueError(f"beta must be positive, got {beta}")
    if not (0.0 <= precision <= 1.0):
        raise ValueError(f"precision out of range: {precision}")
    if not (0.0 <= recall <= 1.0):
        raise ValueError(f"recall out of range: {recall}")
    b2 = beta * beta
    denom = b2 * precision + recall
    if denom == 0.0:
        return 0.0
    score = (1.0 + b2) * precision * recall / denom
    # Numerically clamp to guard against float drift outside the unit interval.
    return min(1.0, max(0.0, score))


def prf(retrieved: set, gold: set, beta: float = 0.5) -> dict:
    """Precision / recall / F-beta over exact-match sets.

    Conventions (all outputs are in [0, 1]):
    - empty ``retrieved`` and empty ``gold``      -> precision=recall=1
    - empty ``retrieved``, non-empty ``gold``     -> precision=0, recall=0
    - non-empty ``retrieved``, empty ``gold``     -> precision=0, recall=1
    """
    tp = len(retrieved & gold)
    if retrieved:
        precision = tp / len(retrieved)
    else:
        precision = 1.0 if not gold else 0.0
    recall = tp / len(gold) if gold else 1.0
    return {
        "precision": precision,
        "recall": recall,
        "f_beta": weighted_f_beta(precision, recall, beta),
        "tp": tp,
        "retrieved": len(retrieved),
        "gold": len(gold),
    }


def suffix_match_prf(
    retrieved_files: set, gold_suffixes: set, beta: float = 0.5
) -> dict:
    """File-level PRF where a retrieved path is relevant iff it ends with a
    gold suffix. Recall counts distinct gold suffixes matched by any path."""
    retrieved_files = {str(f) for f in retrieved_files if f}
    gold_suffixes = {str(g) for g in gold_suffixes if g}
    relevant_retrieved = {
        f for f in retrieved_files if any(f.endswith(g) for g in gold_suffixes)
    }
    matched_gold = {
        g for g in gold_suffixes if any(f.endswith(g) for f in retrieved_files)
    }
    tp = len(relevant_retrieved)
    if retrieved_files:
        precision = tp / len(retrieved_files)
    else:
        precision = 1.0 if not gold_suffixes else 0.0
    recall = len(matched_gold) / len(gold_suffixes) if gold_suffixes else 1.0
    return {
        "precision": precision,
        "recall": recall,
        "f_beta": weighted_f_beta(precision, recall, beta),
        "tp": tp,
        "retrieved": len(retrieved_files),
        "gold": len(gold_suffixes),
    }


def line_pair_prf(
    retrieved_pairs: set, gold_pairs: set, beta: float = 0.5
) -> dict:
    """Line-level PRF over (file_suffix, line) pairs.

    A retrieved ``(file, line)`` is relevant iff there is a gold
    ``(suffix, line)`` with ``file.endswith(suffix)`` and equal line number.
    """
    retrieved_pairs = {(str(f), int(l)) for f, l in retrieved_pairs if f and l is not None}
    gold_pairs = {(str(g), int(gl)) for g, gl in gold_pairs if g and gl is not None}

    def is_relevant(pair) -> bool:
        f, l = pair
        return any(f.endswith(g) and l == gl for g, gl in gold_pairs)

    relevant_retrieved = {p for p in retrieved_pairs if is_relevant(p)}
    matched_gold = {
        (g, gl)
        for (g, gl) in gold_pairs
        if any(f.endswith(g) and l == gl for (f, l) in retrieved_pairs)
    }
    tp = len(relevant_retrieved)
    if retrieved_pairs:
        precision = tp / len(retrieved_pairs)
    else:
        precision = 1.0 if not gold_pairs else 0.0
    recall = len(matched_gold) / len(gold_pairs) if gold_pairs else 1.0
    return {
        "precision": precision,
        "recall": recall,
        "f_beta": weighted_f_beta(precision, recall, beta),
        "tp": tp,
        "retrieved": len(retrieved_pairs),
        "gold": len(gold_pairs),
    }


def mean(values: list) -> float:
    values = [v for v in values if v is not None]
    return sum(values) / len(values) if values else 0.0


def aggregate(per_query: list, beta: float = 0.5) -> dict:
    """Macro-average the per-query PRF dicts produced above.

    ``per_query`` items each carry a ``file`` PRF sub-dict, a ``reciprocal_rank``
    float, and a ``line`` sub-dict that is ``None`` when the gold line could not
    be resolved. Line-level queries with unresolved gold are excluded from the
    line aggregate (a vacuous perfect score would otherwise inflate recall).
    """
    line_rows = [q for q in per_query if q.get("line") is not None]
    return {
        "count": len(per_query),
        "line_count": len(line_rows),
        "mrr": mean([q["reciprocal_rank"] for q in per_query]),
        "file_precision": mean([q["file"]["precision"] for q in per_query]),
        "file_recall": mean([q["file"]["recall"] for q in per_query]),
        "file_f_beta": mean([q["file"]["f_beta"] for q in per_query]),
        "line_precision": mean([q["line"]["precision"] for q in line_rows]),
        "line_recall": mean([q["line"]["recall"] for q in line_rows]),
        "line_f_beta": mean([q["line"]["f_beta"] for q in line_rows]),
        "beta": beta,
    }
