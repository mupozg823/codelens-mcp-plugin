#!/usr/bin/env python3
"""Select the best retained fine-tune checkpoint by benchmark stack."""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_BINARY = ROOT / "target" / "release" / "codelens-mcp"


@dataclass(frozen=True)
class CandidateSpec:
    label: str
    model_dir: Path
    phase: str
    step: int | None


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--training-output-dir", required=True)
    parser.add_argument("--candidate-manifest", required=True)
    parser.add_argument("--project", default=str(ROOT))
    parser.add_argument(
        "--binary",
        default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)),
    )
    parser.add_argument("--output-dir", default="")
    parser.add_argument(
        "--candidate-label",
        action="append",
        default=[],
        help="Optional explicit candidate labels to evaluate (repeatable).",
    )
    parser.add_argument(
        "--keep-exported-candidates",
        action="store_true",
        help="Keep per-candidate ONNX exports after selection completes.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="List discovered candidates and the promotion-gate command without running benchmarks.",
    )
    return parser.parse_known_args()


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def checkpoint_step(path: Path) -> int | None:
    match = re.search(r"checkpoint-(\d+)$", path.name)
    if not match:
        return None
    return int(match.group(1))


def candidate_label(phase: str, checkpoint_dir: Path | None) -> str:
    if checkpoint_dir is None:
        return "final-model"
    step = checkpoint_step(checkpoint_dir)
    step_suffix = f"checkpoint-{step}" if step is not None else checkpoint_dir.name
    return f"{phase}-{step_suffix}"


def discover_candidates(training_output_dir: Path) -> list[CandidateSpec]:
    candidates: list[CandidateSpec] = []
    final_model_dir = training_output_dir / "model"
    if final_model_dir.exists():
        candidates.append(
            CandidateSpec(
                label="final-model",
                model_dir=final_model_dir,
                phase="final",
                step=None,
            )
        )

    phase_dirs = [
        ("stage2", training_output_dir / "checkpoints"),
        ("stage3-product-polish", training_output_dir / "product-polish-checkpoints"),
        ("stage4-semantic-polish", training_output_dir / "semantic-polish-checkpoints"),
    ]
    for phase_name, root in phase_dirs:
        if not root.exists():
            continue
        checkpoint_dirs = [
            path for path in root.glob("checkpoint-*") if path.is_dir()
        ]
        checkpoint_dirs.sort(key=lambda path: checkpoint_step(path) or -1)
        for checkpoint_dir in checkpoint_dirs:
            candidates.append(
                CandidateSpec(
                    label=candidate_label(phase_name, checkpoint_dir),
                    model_dir=checkpoint_dir,
                    phase=phase_name,
                    step=checkpoint_step(checkpoint_dir),
                )
            )
    return candidates


def ensure_unique_labels(candidates: list[CandidateSpec]) -> None:
    labels = [candidate.label for candidate in candidates]
    if len(labels) == len(set(labels)):
        return
    raise SystemExit(f"Discovered duplicate candidate labels: {labels}")


def export_candidate_onnx(model_dir: Path, export_dir: Path) -> Path:
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    if export_dir.exists():
        shutil.rmtree(export_dir)
    export_dir.mkdir(parents=True, exist_ok=True)
    model = ORTModelForFeatureExtraction.from_pretrained(str(model_dir), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_dir))
    model.save_pretrained(str(export_dir))
    tokenizer.save_pretrained(str(export_dir))
    return export_dir


def baseline_or_exported_onnx_dir(
    training_output_dir: Path,
    export_root: Path,
    candidate: CandidateSpec,
) -> Path:
    canonical_final_onnx = training_output_dir / "onnx"
    if candidate.label == "final-model" and (canonical_final_onnx / "model.onnx").exists():
        return canonical_final_onnx
    return export_candidate_onnx(candidate.model_dir, export_root / candidate.label)


def build_gate_command(
    *,
    project: str,
    binary: str,
    output_dir: Path,
    manifest: Path,
    candidates: list[CandidateSpec],
    candidate_onnx_dirs: dict[str, Path],
    passthrough_args: list[str],
) -> list[str]:
    cmd = [
        "python3",
        str(SCRIPT_DIR / "promotion_gate.py"),
        "--project",
        project,
        "--binary",
        binary,
        "--output-dir",
        str(output_dir),
    ]
    for candidate in candidates:
        cmd.extend(["--candidate-onnx-dir", str(candidate_onnx_dirs[candidate.label])])
        cmd.extend(["--candidate-label", candidate.label])
        cmd.extend(["--candidate-manifest", str(manifest)])
    cmd.extend(passthrough_args)
    return cmd


def backup_tree(path: Path, *, tag: str) -> Path | None:
    if not path.exists():
        return None
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    backup_path = path.parent / f"{path.name}-{tag}-{timestamp}"
    shutil.move(str(path), str(backup_path))
    return backup_path


def replace_tree(
    source: Path,
    destination: Path,
    *,
    tag: str,
    backup: bool = True,
) -> Path | None:
    backup_path = backup_tree(destination, tag=tag) if backup else None
    tmp_destination = destination.parent / f".{destination.name}.tmp"
    if tmp_destination.exists():
        shutil.rmtree(tmp_destination)
    shutil.copytree(source, tmp_destination)
    tmp_destination.replace(destination)
    return backup_path


def materialize_clean_model(model_dir: Path, output_model_dir: Path) -> None:
    from sentence_transformers import SentenceTransformer

    tmp_dir = output_model_dir.parent / f".{output_model_dir.name}.materialize"
    if tmp_dir.exists():
        shutil.rmtree(tmp_dir)
    model = SentenceTransformer(str(model_dir))
    model.save(str(tmp_dir))
    replace_tree(
        tmp_dir,
        output_model_dir,
        tag="pre-benchmark-selection",
        backup=False,
    )
    shutil.rmtree(tmp_dir, ignore_errors=True)


def main():
    args, passthrough_args = parse_args()
    training_output_dir = Path(args.training_output_dir).expanduser().resolve()
    manifest = Path(args.candidate_manifest).expanduser().resolve()
    project = str(Path(args.project).expanduser().resolve())
    binary = str(Path(args.binary).expanduser().resolve())
    output_dir = (
        Path(args.output_dir).expanduser().resolve()
        if args.output_dir
        else (training_output_dir / "benchmark-selection")
    )
    export_root = output_dir / "candidate-onnx"
    gate_output_dir = output_dir / "promotion-gate"

    candidates = discover_candidates(training_output_dir)
    ensure_unique_labels(candidates)
    if args.candidate_label:
        requested = set(args.candidate_label)
        candidates = [candidate for candidate in candidates if candidate.label in requested]
        missing = sorted(requested - {candidate.label for candidate in candidates})
        if missing:
            raise SystemExit(f"Unknown candidate labels: {missing}")
    if not candidates:
        raise SystemExit(f"No checkpoint candidates found under {training_output_dir}")

    candidate_onnx_dirs = {
        candidate.label: (
            (training_output_dir / "onnx")
            if candidate.label == "final-model"
            and (training_output_dir / "onnx" / "model.onnx").exists()
            else (export_root / candidate.label)
        )
        for candidate in candidates
    }
    gate_cmd = build_gate_command(
        project=project,
        binary=binary,
        output_dir=gate_output_dir,
        manifest=manifest,
        candidates=candidates,
        candidate_onnx_dirs=candidate_onnx_dirs,
        passthrough_args=passthrough_args,
    )

    if args.dry_run:
        print(
            json.dumps(
                {
                    "training_output_dir": str(training_output_dir),
                    "candidate_manifest": str(manifest),
                    "candidates": [
                        {
                            "label": candidate.label,
                            "phase": candidate.phase,
                            "step": candidate.step,
                            "model_dir": str(candidate.model_dir),
                            "planned_onnx_dir": str(candidate_onnx_dirs[candidate.label]),
                        }
                        for candidate in candidates
                    ],
                    "promotion_gate_command": gate_cmd,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return

    output_dir.mkdir(parents=True, exist_ok=True)
    export_root.mkdir(parents=True, exist_ok=True)

    exported_onnx_dirs: dict[str, Path] = {}
    for candidate in candidates:
        exported_onnx_dirs[candidate.label] = baseline_or_exported_onnx_dir(
            training_output_dir,
            export_root,
            candidate,
        )

    gate_cmd = build_gate_command(
        project=project,
        binary=binary,
        output_dir=gate_output_dir,
        manifest=manifest,
        candidates=candidates,
        candidate_onnx_dirs=exported_onnx_dirs,
        passthrough_args=passthrough_args,
    )
    gate_result = run(gate_cmd)
    report_path = gate_output_dir / "promotion-gate-report.json"
    if not report_path.exists():
        raise SystemExit(
            "promotion_gate.py did not produce a report\n"
            f"stdout:\n{gate_result.stdout}\n\n"
            f"stderr:\n{gate_result.stderr}"
        )

    gate_report = json.loads(report_path.read_text(encoding="utf-8"))
    selected_label = gate_report.get("selected_candidate_label")
    selected_candidate = next(
        (candidate for candidate in candidates if candidate.label == selected_label),
        None,
    )

    selection_report = {
        "schema_version": "codelens-checkpoint-selection-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "training_output_dir": str(training_output_dir),
        "candidate_manifest": str(manifest),
        "promotion_gate_command": [shlex.quote(part) for part in gate_cmd],
        "promotion_gate_exit_code": gate_result.returncode,
        "promotion_gate_report": str(report_path),
        "candidates": [
            {
                "label": candidate.label,
                "phase": candidate.phase,
                "step": candidate.step,
                "model_dir": str(candidate.model_dir),
                "onnx_dir": str(exported_onnx_dirs[candidate.label]),
            }
            for candidate in candidates
        ],
        "selected_candidate_label": selected_label,
        "selected_model_dir": str(selected_candidate.model_dir) if selected_candidate else None,
        "selected_onnx_dir": (
            str(exported_onnx_dirs[selected_label]) if selected_label else None
        ),
        "materialized_model_dir": None,
        "materialized_onnx_dir": None,
        "backups": {
            "model": None,
            "onnx": None,
        },
    }

    if selected_candidate is None:
        selection_path = output_dir / "checkpoint-selection-report.json"
        selection_path.write_text(
            json.dumps(selection_report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(json.dumps(selection_report, ensure_ascii=False, indent=2))
        if not args.keep_exported_candidates and export_root.exists():
            shutil.rmtree(export_root, ignore_errors=True)
        raise SystemExit(gate_result.returncode or 1)

    canonical_model_dir = training_output_dir / "model"
    canonical_onnx_dir = training_output_dir / "onnx"
    if selected_candidate.model_dir.resolve() != canonical_model_dir.resolve():
        model_backup = backup_tree(
            canonical_model_dir,
            tag="pre-benchmark-selection",
        )
        if model_backup is not None:
            selection_report["backups"]["model"] = str(model_backup)
        materialize_clean_model(selected_candidate.model_dir, canonical_model_dir)
    selected_onnx_dir = exported_onnx_dirs[selected_candidate.label]
    if selected_onnx_dir.resolve() != canonical_onnx_dir.resolve():
        onnx_backup = replace_tree(
            selected_onnx_dir,
            canonical_onnx_dir,
            tag="pre-benchmark-selection",
        )
        if onnx_backup is not None:
            selection_report["backups"]["onnx"] = str(onnx_backup)

    selection_report["materialized_model_dir"] = str(canonical_model_dir)
    selection_report["materialized_onnx_dir"] = str(canonical_onnx_dir)
    selection_path = output_dir / "checkpoint-selection-report.json"
    selection_path.write_text(
        json.dumps(selection_report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(selection_report, ensure_ascii=False, indent=2))

    if not args.keep_exported_candidates and export_root.exists():
        shutil.rmtree(export_root, ignore_errors=True)


if __name__ == "__main__":
    main()
