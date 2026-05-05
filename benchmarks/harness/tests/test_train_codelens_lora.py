import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "scripts" / "finetune" / "train_codelens_lora.py"


def load_script_module():
    spec = importlib.util.spec_from_file_location("train_codelens_lora_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.write_text(
        "".join(json.dumps(row) + "\n" for row in rows),
        encoding="utf-8",
    )


class TrainCodeLensLoraTests(unittest.TestCase):
    def test_load_pairs_filters_invalid_rows_and_caps(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            data_path = Path(tmpdir) / "train.jsonl"
            write_jsonl(
                data_path,
                [
                    {"query": "find symbol search", "positive": "fn semantic_search"},
                    {"query": "", "positive": "missing query"},
                    {"query": "missing positive"},
                    {"query": "trace request", "positive": "fn trace_request_path"},
                ],
            )

            pairs = module.load_pairs(data_path, max_rows=1)

            self.assertEqual(len(pairs), 1)
            self.assertEqual(pairs[0].query, "find symbol search")
            self.assertEqual(pairs[0].positive, "fn semantic_search")

    def test_runtime_manifest_contains_lora_and_quantization_fields(self):
        module = load_script_module()
        args = SimpleNamespace(
            model_name="MiniLM-L12-CodeLens-LoRA-INT8",
            base_model="sentence-transformers/all-MiniLM-L12-v2",
            teacher_dir=str(REPO_ROOT / "crates" / "codelens-engine" / "models" / "codesearch"),
            teacher_label="MiniLM-L12-CodeSearchNet-INT8",
            train_data="train.jsonl",
            rank=16,
            alpha=32,
            dropout=0.05,
            target_modules=["query", "value"],
            output_dir="out",
            no_quantize=False,
        )

        manifest = module.build_runtime_manifest(
            args,
            quantized=True,
            train_stats={"rows": 10},
            validation_stats={"rows": 2},
        )

        self.assertEqual(manifest["adapter_type"], "lora")
        self.assertEqual(manifest["quantization"], "dynamic-int8")
        self.assertEqual(manifest["lora_rank"], 16)
        self.assertEqual(manifest["export_backend"], "onnx")
        self.assertEqual(manifest["teacher_model"], "MiniLM-L12-CodeSearchNet-INT8")
        self.assertIn("teacher_model_dir", manifest)
        self.assertIn("promotion_gate.py", " ".join(manifest["promotion_gate_command"]))

    def test_dry_run_writes_training_plan_without_ml_dependencies(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            train_path = root / "train.jsonl"
            validation_path = root / "validation.jsonl"
            output_dir = root / "out"
            write_jsonl(
                train_path,
                [
                    {"query": "rank context for a task", "positive": "get_ranked_context"},
                    {"query": "find callers", "positive": "get_callers"},
                ],
            )
            write_jsonl(
                validation_path,
                [{"query": "semantic lookup", "positive": "semantic_search"}],
            )

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--train-data",
                    str(train_path),
                    "--validation-data",
                    str(validation_path),
                    "--output-dir",
                    str(output_dir),
                    "--model-name",
                    "CodeLens-Test-LoRA",
                    "--teacher-dir",
                    str(REPO_ROOT / "crates" / "codelens-engine" / "models" / "codesearch"),
                    "--dry-run",
                ],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            plan = json.loads((output_dir / "training-plan.json").read_text())
            self.assertTrue(plan["dry_run"])
            self.assertEqual(plan["adapter_type"], "lora")
            self.assertEqual(plan["quantization"], "dynamic-int8")
            self.assertEqual(plan["teacher_model"], "MiniLM-L12-CodeSearchNet-INT8")
            self.assertEqual(plan["train_stats"]["rows"], 2)
            self.assertIn("promotion_gate.py", " ".join(plan["promotion_gate_command"]))
            self.assertFalse((output_dir / "onnx" / "model.onnx").exists())


if __name__ == "__main__":
    unittest.main()
