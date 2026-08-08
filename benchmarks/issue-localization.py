#!/usr/bin/env python3
"""Score CodeLens surfaces on CodeScout-style issue -> location localization.

Two surfaces are scored against the same labeled set:

* ``analyze_change_request`` -- called with ``task`` only. ``changed_files`` is
  never sent: it would hand the gold file set straight to the tool.
* ``search`` in ``mode=ranked`` -- called with ``query`` only.

Predictions are harvested into three granularities (file / container /
function) and compared with the hand-verified gold sets using the paper's
sample-averaged precision, recall, and F1, plus file-level hit@1 / hit@3.

Determinism contract: an empty dataset, or a run where no query produced a
single parsed prediction on any surface, exits 2. A report full of zeros is
never allowed to exit 0 -- a silent parser break must not read as a real
measurement.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import benchmark_runtime_common as runtime_common  # noqa: E402

DEFAULT_BASE_URL = "http://127.0.0.1:7838"
DEFAULT_PROJECT = str(SCRIPT_DIR.parent)
CLIENT_NAME = "claude-code"

PATH_RE = re.compile(r"(?:[\w.-]+/)+[\w.-]+\.(?:rs|py|toml|md|json|ts|tsx|js)")
FINDING_RE = re.compile(r"^\s*(?P<symbol>[\w:<>]+)\s*:\s*start in\s+(?P<file>\S+)")
CONTAINER_KINDS = {"struct", "enum", "trait", "impl", "mod", "module", "class", "union"}
FUNCTION_KINDS = {"function", "fn", "method", "func"}


class SurfaceError(RuntimeError):
    pass


def rel_path(raw: str, project: str) -> str | None:
    text = raw.strip().strip("`\"',()[]")
    if not text:
        return None
    if text.startswith(project):
        text = text[len(project) :].lstrip("/")
    if text.startswith("/"):
        return None
    if "." not in Path(text).name:
        return None
    return text


def harvest_paths(node, project: str, sink: set[str]) -> None:
    if isinstance(node, str):
        for match in PATH_RE.findall(node):
            candidate = rel_path(match, project)
            if candidate:
                sink.add(candidate)
    elif isinstance(node, dict):
        for value in node.values():
            harvest_paths(value, project, sink)
    elif isinstance(node, list):
        for value in node:
            harvest_paths(value, project, sink)


def bare_container(label: str) -> str:
    """``impl Trait for Type`` / ``mod tests`` -> the identifying name."""
    text = label.strip()
    if " for " in text:
        text = text.split(" for ", 1)[1]
    parts = text.split()
    return parts[-1] if parts else text


def normalise_gold(gold: dict) -> dict:
    files = set(gold.get("files", []))
    containers = set()
    for entry in gold.get("containers", []):
        path, _, label = entry.rpartition("::")
        containers.add(f"{path}::{bare_container(label)}")
    functions = set()
    for entry in gold.get("functions", []):
        parts = entry.split("::")
        functions.add(f"{parts[0]}::{parts[-1]}")
    return {"files": files, "containers": containers, "functions": functions}


def parse_analyze_change_request(payload: dict, project: str) -> dict:
    data = payload.get("data")
    data = data if isinstance(data, dict) else {}
    files: set[str] = set()
    containers: set[str] = set()
    functions: set[str] = set()
    ordered_files: list[str] = []

    for finding in data.get("top_findings", []) or []:
        if not isinstance(finding, str):
            continue
        match = FINDING_RE.match(finding)
        if not match:
            continue
        path = rel_path(match.group("file"), project)
        if not path:
            continue
        if path not in ordered_files:
            ordered_files.append(path)
        files.add(path)
        symbol = match.group("symbol").split("::")[-1]
        # The surface does not label symbol kind here, so a finding is counted
        # at both symbol granularities rather than guessed into one.
        functions.add(f"{path}::{symbol}")
        containers.add(f"{path}::{symbol}")

    harvested: set[str] = set()
    harvest_paths(data, project, harvested)
    for path in sorted(harvested):
        if path not in ordered_files:
            ordered_files.append(path)
    files |= harvested

    return {
        "files": files,
        "containers": containers,
        "functions": functions,
        "ranked_files": ordered_files,
    }


def parse_ranked(payload: dict, project: str) -> dict:
    body = payload.get("data_preview")
    if not isinstance(body, dict):
        body = payload.get("data")
    if not isinstance(body, dict):
        body = payload
    symbols = body.get("symbols")
    symbols = symbols if isinstance(symbols, list) else []

    files: set[str] = set()
    containers: set[str] = set()
    functions: set[str] = set()
    ordered_files: list[str] = []

    for symbol in symbols:
        if not isinstance(symbol, dict):
            continue
        path = rel_path(str(symbol.get("file", "")), project)
        if not path:
            continue
        if path not in ordered_files:
            ordered_files.append(path)
        files.add(path)
        name = str(symbol.get("name", "")).split("::")[-1].strip()
        if not name:
            continue
        kind = str(symbol.get("kind", "")).lower()
        if kind in CONTAINER_KINDS:
            containers.add(f"{path}::{name}")
        elif kind in FUNCTION_KINDS:
            functions.add(f"{path}::{name}")
        else:
            containers.add(f"{path}::{name}")
            functions.add(f"{path}::{name}")

    if not symbols:
        harvested: set[str] = set()
        harvest_paths(body, project, harvested)
        for path in sorted(harvested):
            if path not in ordered_files:
                ordered_files.append(path)
        files |= harvested

    return {
        "files": files,
        "containers": containers,
        "functions": functions,
        "ranked_files": ordered_files,
    }


SURFACES = {
    "analyze_change_request": {
        # analyze_change_request has no paging knob; page_size is ignored here.
        "arguments": lambda query, page_size: {"task": query},
        "tool": "analyze_change_request",
        "parser": parse_analyze_change_request,
    },
    "search_ranked": {
        "tool": "search",
        "arguments": lambda query, page_size: (
            {"mode": "ranked", "query": query, "page_size": page_size}
            if page_size
            else {"mode": "ranked", "query": query}
        ),
        "parser": parse_ranked,
    },
}


def prf(predicted: set[str], gold: set[str]) -> tuple[float, float, float]:
    if not gold:
        return (0.0, 0.0, 0.0)
    hit = len(predicted & gold)
    precision = hit / len(predicted) if predicted else 0.0
    recall = hit / len(gold)
    if precision + recall == 0:
        return (precision, recall, 0.0)
    return (precision, recall, 2 * precision * recall / (precision + recall))


def mean(values: list[float]) -> float:
    return round(sum(values) / len(values), 4) if values else 0.0


def miss_cause(missing: list[str], predicted: set[str], prediction: dict) -> str:
    """One-line, deterministic classification of a file-level miss.

    Deliberately shallow -- it separates a ranking loss from a retrieval loss so
    the deeper root-cause split stays a human call.
    """
    ranked = prediction["ranked_files"]
    top3 = set(ranked[:3])
    if any(path in predicted for path in missing):
        return "retrieved but outside the top-3 window"
    missing_dirs = {str(Path(path).parent) for path in missing}
    if any(str(Path(path).parent) in missing_dirs for path in top3):
        return "right module, wrong file"
    missing_crates = {path.split("/")[1] for path in missing if "/" in path}
    top_crates = {path.split("/")[1] for path in top3 if "/" in path}
    if missing_crates & top_crates:
        return "right crate, wrong module"
    return "different subsystem entirely"


def open_session(base_url: str, project: str, token_budget: int | None) -> str | None:
    session_id, response, _ = runtime_common.initialize_http_session(
        base_url, client_name=CLIENT_NAME, request_id=1
    )
    if "result" not in response:
        raise SurfaceError(f"initialize failed: {json.dumps(response)[:400]}")
    arguments: dict = {"project": project}
    if token_budget:
        arguments["token_budget"] = token_budget
    bootstrap = runtime_common.mcp_http_tool_call(
        base_url,
        "prepare_harness_session",
        arguments,
        request_id=2,
        session_id=session_id,
    )
    payload = runtime_common.extract_tool_payload(bootstrap)
    data = payload.get("data", {}) if isinstance(payload, dict) else {}
    bound = (data.get("project") or {}).get("project_name")
    if not bound:
        raise SurfaceError(f"project binding failed: {json.dumps(payload)[:400]}")
    return session_id


def extract_payload_channel(response) -> tuple[dict, str]:
    """Prefer ``structuredContent`` (full data) over the summarized text channel.

    The text channel samples every array to 3 items and omits ``page`` /
    ``next_cursor`` (``TEXT_CHANNEL_MAX_ARRAY_ITEMS`` in
    ``dispatch/response_support``), so scoring it measures the preview, not the
    retrieval. ``runtime_common.extract_tool_payload`` reads only the text
    channel and stays untouched — other benchmarks depend on its behavior.
    """
    result = response.get("result") if isinstance(response, dict) else None
    if isinstance(result, dict):
        structured = result.get("structuredContent")
        if isinstance(structured, dict) and structured:
            return structured, "structuredContent"
    payload = runtime_common.extract_tool_payload(response)
    return payload if isinstance(payload, dict) else {}, "text"


def run_query(
    base_url: str,
    session_id: str | None,
    surface: str,
    query: str,
    project: str,
    timeout_seconds: int,
    raw_dir: Path | None,
    query_id: str,
    page_size: int | None = None,
) -> dict:
    spec = SURFACES[surface]
    started = time.monotonic()
    response = runtime_common.mcp_http_tool_call(
        base_url,
        spec["tool"],
        spec["arguments"](query, page_size),
        request_id=100,
        session_id=session_id,
        timeout_seconds=timeout_seconds,
    )
    elapsed_ms = int((time.monotonic() - started) * 1000)
    payload, payload_channel = extract_payload_channel(response)
    if raw_dir is not None:
        raw_dir.mkdir(parents=True, exist_ok=True)
        (raw_dir / f"{query_id}.{surface}.json").write_text(
            json.dumps(payload, indent=1, ensure_ascii=False) + "\n", encoding="utf-8"
        )
    parsed = spec["parser"](payload, project)
    raw_chars = len(json.dumps(payload, ensure_ascii=False))
    parsed_total = (
        len(parsed["files"]) + len(parsed["containers"]) + len(parsed["functions"])
    )
    return {
        "surface": surface,
        "elapsed_ms": elapsed_ms,
        "raw_chars": raw_chars,
        "payload_channel": payload_channel,
        "success": bool(payload.get("success", False)),
        "truncated": bool(payload.get("truncated", False)),
        "compression_stage": payload.get("compression_stage"),
        "token_estimate": payload.get("token_estimate"),
        "parsed_total": parsed_total,
        # Instrument guard: a fat response that parses to nothing is a parser
        # bug, not a retrieval result (a prior parser break moved hit@1 by 11pp).
        "instrument_warning": raw_chars > 200 and parsed_total == 0,
        "files": sorted(parsed["files"]),
        "containers": sorted(parsed["containers"]),
        "functions": sorted(parsed["functions"]),
        "ranked_files": parsed["ranked_files"],
    }


def score_surface(entries: list[dict]) -> dict:
    buckets: dict[str, dict[str, list[float]]] = {
        level: {"precision": [], "recall": [], "f1": []}
        for level in ("files", "containers", "functions")
    }
    scored_counts = {level: 0 for level in buckets}
    hit1: list[float] = []
    hit3: list[float] = []
    for entry in entries:
        gold = entry["gold_normalised"]
        prediction = entry["prediction"]
        for level in buckets:
            if not gold[level]:
                continue
            scored_counts[level] += 1
            precision, recall, f1 = prf(set(prediction[level]), gold[level])
            buckets[level]["precision"].append(precision)
            buckets[level]["recall"].append(recall)
            buckets[level]["f1"].append(f1)
        ranked = prediction["ranked_files"]
        gold_files = gold["files"]
        if gold_files:
            hit1.append(1.0 if ranked[:1] and ranked[0] in gold_files else 0.0)
            hit3.append(1.0 if set(ranked[:3]) & gold_files else 0.0)
    result = {
        level: {
            "precision": mean(values["precision"]),
            "recall": mean(values["recall"]),
            "f1": mean(values["f1"]),
            "scored_queries": scored_counts[level],
        }
        for level, values in buckets.items()
    }
    result["file_hit_at_1"] = mean(hit1)
    result["file_hit_at_3"] = mean(hit3)
    result["queries"] = len(entries)
    return result


def render_markdown(report: dict) -> str:
    lines: list[str] = []
    lines.append("# Issue-localization baseline")
    lines.append("")
    lines.append(f"- Generated: `{report['generated_at']}`")
    lines.append(f"- Repo HEAD: `{report['head_sha']}`")
    lines.append(f"- Daemon: `{report['base_url']}` (clientInfo.name=`{CLIENT_NAME}`)")
    lines.append(
        f"- Dataset: {report['query_count']} queries "
        f"({report['class_counts'].get('verbatim', 0)} verbatim / "
        f"{report['class_counts'].get('descriptive', 0)} descriptive)"
    )
    lines.append("")
    lines.append("## Ground-truth rules")
    lines.append("")
    for rule in report["ground_truth_rules"]:
        lines.append(f"- {rule}")
    lines.append("")
    lines.append(
        "**HEAD-state deviation.** " + report["head_state_deviation"]
    )
    lines.append("")
    lines.append("## Scores")
    lines.append("")
    lines.append(
        "| surface | class | granularity | precision | recall | F1 | scored q |"
    )
    lines.append("| --- | --- | --- | --- | --- | --- | --- |")
    for surface, blocks in report["scores"].items():
        for class_name, block in blocks.items():
            for level in ("files", "containers", "functions"):
                cell = block[level]
                lines.append(
                    f"| {surface} | {class_name} | {level} | "
                    f"{cell['precision']:.3f} | {cell['recall']:.3f} | "
                    f"{cell['f1']:.3f} | {cell['scored_queries']} |"
                )
    lines.append("")
    lines.append("| surface | class | file hit@1 | file hit@3 | queries |")
    lines.append("| --- | --- | --- | --- | --- |")
    for surface, blocks in report["scores"].items():
        for class_name, block in blocks.items():
            lines.append(
                f"| {surface} | {class_name} | {block['file_hit_at_1']:.3f} | "
                f"{block['file_hit_at_3']:.3f} | {block['queries']} |"
            )
    lines.append("")
    lines.append("## Per-query misses")
    lines.append("")
    for surface, misses in report["misses"].items():
        lines.append(f"### {surface}")
        lines.append("")
        if not misses:
            lines.append("No file-level misses.")
            lines.append("")
            continue
        for miss in misses:
            lines.append(
                f"- `{miss['id']}` ({miss['class']}, `{miss['sha']}`) — "
                f"{miss['cause']}. Missing {miss['missing_files']}; "
                f"predicted top-3 {miss['predicted_top3']}"
            )
        lines.append("")
    lines.append("## Surface response shape")
    lines.append("")
    lines.append("| surface | truncated q | stage-5 q | median parsed items |")
    lines.append("| --- | --- | --- | --- |")
    for surface, shape in report["response_shape"].items():
        lines.append(
            f"| {surface} | {shape['truncated_queries']} | "
            f"{shape['stage5_queries']} | {shape['median_parsed_items']} |"
        )
    lines.append("")
    if report["instrument_warnings"]:
        lines.append("## Instrument warnings")
        lines.append("")
        for warning in report["instrument_warnings"]:
            lines.append(f"- {warning}")
        lines.append("")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL)
    parser.add_argument("--project", default=DEFAULT_PROJECT)
    parser.add_argument(
        "--surfaces", default=",".join(SURFACES), help="comma-separated surface list"
    )
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--token-budget", type=int, default=None)
    parser.add_argument(
        "--page-size",
        type=int,
        default=None,
        help="page_size forwarded to search mode=ranked (default: surface default page)",
    )
    parser.add_argument("--output-json", default=None)
    parser.add_argument("--output-md", default=None)
    parser.add_argument("--dump-raw", default=None, help="directory for raw payloads")
    args = parser.parse_args()

    dataset = json.loads(Path(args.dataset).read_text(encoding="utf-8"))
    queries = dataset.get("queries", [])
    if not queries:
        print("dataset carries no queries", file=sys.stderr)
        return 2

    surfaces = [s.strip() for s in args.surfaces.split(",") if s.strip()]
    unknown = [s for s in surfaces if s not in SURFACES]
    if unknown:
        print(f"unknown surfaces: {unknown}", file=sys.stderr)
        return 2

    project = str(Path(args.project).resolve())
    raw_dir = Path(args.dump_raw) if args.dump_raw else None

    try:
        session_id = open_session(args.base_url, project, args.token_budget)
    except Exception as error:  # noqa: BLE001 - surfaced verbatim, never worked around
        print(f"daemon session failed: {type(error).__name__}: {error}", file=sys.stderr)
        return 2

    per_surface: dict[str, list[dict]] = {surface: [] for surface in surfaces}
    instrument_warnings: list[str] = []
    call_failures: list[str] = []

    for item in queries:
        gold_normalised = normalise_gold(item["gold"])
        for surface in surfaces:
            try:
                outcome = run_query(
                    args.base_url,
                    session_id,
                    surface,
                    item["query"],
                    project,
                    args.timeout,
                    raw_dir,
                    item["id"],
                    args.page_size,
                )
            except Exception as error:  # noqa: BLE001
                call_failures.append(f"{item['id']}/{surface}: {type(error).__name__}")
                continue
            if outcome["instrument_warning"]:
                instrument_warnings.append(
                    f"{item['id']}/{surface}: {outcome['raw_chars']} raw chars "
                    f"parsed to 0 predictions"
                )
            per_surface[surface].append(
                {
                    "id": item["id"],
                    "sha": item["sha"],
                    "class": item["class"],
                    "gold_normalised": gold_normalised,
                    "prediction": outcome,
                }
            )

    parsed_any = any(
        entry["prediction"]["parsed_total"] > 0
        for entries in per_surface.values()
        for entry in entries
    )
    if not parsed_any:
        print(
            "every query parsed to 0 predictions on every surface — "
            "instrument failure, refusing to emit an all-zero baseline",
            file=sys.stderr,
        )
        return 2

    scores: dict[str, dict] = {}
    misses: dict[str, list[dict]] = {}
    response_shape: dict[str, dict] = {}
    for surface, entries in per_surface.items():
        scores[surface] = {"all": score_surface(entries)}
        for class_name in ("verbatim", "descriptive"):
            subset = [e for e in entries if e["class"] == class_name]
            if subset:
                scores[surface][class_name] = score_surface(subset)
        surface_misses = []
        for entry in entries:
            gold_files = entry["gold_normalised"]["files"]
            predicted = set(entry["prediction"]["files"])
            missing = sorted(gold_files - predicted)
            if missing:
                surface_misses.append(
                    {
                        "id": entry["id"],
                        "sha": entry["sha"],
                        "class": entry["class"],
                        "missing_files": missing,
                        "predicted_top3": entry["prediction"]["ranked_files"][:3],
                        "cause": miss_cause(missing, predicted, entry["prediction"]),
                    }
                )
        misses[surface] = surface_misses
        parsed_counts = sorted(e["prediction"]["parsed_total"] for e in entries)
        median = parsed_counts[len(parsed_counts) // 2] if parsed_counts else 0
        response_shape[surface] = {
            "truncated_queries": sum(
                1 for e in entries if e["prediction"]["truncated"]
            ),
            "stage5_queries": sum(
                1 for e in entries if e["prediction"]["compression_stage"] == 5
            ),
            "structured_channel_queries": sum(
                1
                for e in entries
                if e["prediction"]["payload_channel"] == "structuredContent"
            ),
            "median_parsed_items": median,
        }

    class_counts: dict[str, int] = {}
    for item in queries:
        class_counts[item["class"]] = class_counts.get(item["class"], 0) + 1

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "base_url": args.base_url,
        "project": project,
        "head_sha": dataset.get("head_sha"),
        "client_name": CLIENT_NAME,
        "token_budget_override": args.token_budget,
        "page_size_override": args.page_size,
        "query_count": len(queries),
        "class_counts": class_counts,
        "ground_truth_rules": dataset.get("ground_truth_rules", []),
        "head_state_deviation": dataset.get("head_state_deviation", ""),
        "scores": scores,
        "misses": misses,
        "response_shape": response_shape,
        "instrument_warnings": instrument_warnings,
        "call_failures": call_failures,
        "per_query": {
            surface: [
                {
                    "id": e["id"],
                    "class": e["class"],
                    "predicted_files": e["prediction"]["files"],
                    "predicted_functions": e["prediction"]["functions"],
                    "gold_files": sorted(e["gold_normalised"]["files"]),
                    "gold_functions": sorted(e["gold_normalised"]["functions"]),
                    "elapsed_ms": e["prediction"]["elapsed_ms"],
                    "compression_stage": e["prediction"]["compression_stage"],
                }
                for e in entries
            ]
            for surface, entries in per_surface.items()
        },
    }

    json_path = args.output_json or str(
        SCRIPT_DIR / "issue-localization-baseline.json"
    )
    md_path = args.output_md or str(SCRIPT_DIR / "issue-localization-baseline.md")
    Path(json_path).write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    Path(md_path).write_text(render_markdown(report), encoding="utf-8")

    for surface in surfaces:
        block = scores[surface]["all"]
        print(
            f"{surface}: file F1={block['files']['f1']:.3f} "
            f"container F1={block['containers']['f1']:.3f} "
            f"function F1={block['functions']['f1']:.3f} "
            f"hit@1={block['file_hit_at_1']:.3f} hit@3={block['file_hit_at_3']:.3f}"
        )
    if call_failures:
        print(f"call failures: {call_failures}", file=sys.stderr)
    print(f"wrote {json_path}")
    print(f"wrote {md_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
