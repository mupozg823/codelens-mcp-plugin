import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


BENCHMARKS_DIR = Path(__file__).resolve().parents[2]
SCRIPT = BENCHMARKS_DIR / "existing-model-bakeoff.py"


def load_script_module():
    if str(BENCHMARKS_DIR) not in sys.path:
        sys.path.insert(0, str(BENCHMARKS_DIR))
    spec = importlib.util.spec_from_file_location("existing_model_bakeoff_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class ExistingModelBakeoffTests(unittest.TestCase):
    def write_model_assets(self, model_dir: Path):
        module = load_script_module()
        model_dir.mkdir(parents=True)
        for asset in module.REQUIRED_MODEL_ASSETS:
            (model_dir / asset).write_text("{}", encoding="utf-8")

    def test_discover_default_candidates_skips_incomplete_models(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            complete = root / "crates" / "codelens-engine" / "models" / "codesearch"
            incomplete = root / "scripts" / "finetune" / "output" / "v6-internet" / "onnx"
            self.write_model_assets(complete)
            incomplete.mkdir(parents=True)
            (incomplete / "model.onnx").write_text("onnx", encoding="utf-8")

            candidates = module.discover_default_candidates(root)

            self.assertEqual([candidate.label for candidate in candidates], ["bundled"])
            self.assertEqual(candidates[0].model_dir, complete)

    def test_parse_candidate_spec_requires_label_and_path(self):
        module = load_script_module()

        candidate = module.parse_candidate_spec("v8=scripts/finetune/output/v8-final/onnx")

        self.assertEqual(candidate.label, "v8")
        self.assertEqual(candidate.model_dir, Path("scripts/finetune/output/v8-final/onnx"))
        with self.assertRaises(SystemExit):
            module.parse_candidate_spec("missing-separator")

    def test_summarize_report_extracts_leaderboard_fields(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            report_path = Path(tmpdir) / "report.json"
            report_path.write_text(
                json.dumps(
                    {
                        "dataset_size": 2,
                        "runtime_model": {"sha256": "abc", "size_bytes": 123},
                        "hybrid_uplift": {"mrr_delta": 0.1},
                        "methods": [
                            {"method": "semantic_search", "mrr": 0.4, "acc1": 0.3},
                            {
                                "method": "get_ranked_context_no_semantic",
                                "mrr": 0.5,
                                "acc1": 0.4,
                            },
                            {
                                "method": "get_ranked_context",
                                "mrr": 0.6,
                                "acc1": 0.5,
                                "acc3": 0.7,
                                "acc5": 0.8,
                                "avg_elapsed_ms": 42,
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )

            summary = module.summarize_report(
                SimpleNamespace(label="candidate", model_dir=Path("/model")),
                report_path,
            )

            self.assertEqual(summary["label"], "candidate")
            self.assertEqual(summary["hybrid_mrr"], 0.6)
            self.assertEqual(summary["semantic_mrr"], 0.4)
            self.assertEqual(summary["lexical_mrr"], 0.5)
            self.assertEqual(summary["runtime_model"]["sha256"], "abc")

    def test_embedding_quality_command_omits_custom_embed_model_by_default(self):
        module = load_script_module()
        args = SimpleNamespace(
            project_path=".",
            binary="target/debug/codelens-mcp",
            dataset="benchmarks/embedding-quality-dataset-self.json",
            preset="balanced",
            max_results=10,
            ranked_context_max_tokens=50000,
            isolated_copy=False,
            set_embed_model_label=False,
        )

        cmd = module.embedding_quality_command(
            args,
            module.Candidate(label="v8-final", model_dir=Path("/model")),
            Path("/tmp/report"),
        )

        self.assertNotIn("--embed-model", cmd)

    def test_parse_args_isolates_project_by_default(self):
        module = load_script_module()

        args = module.parse_args([])

        self.assertTrue(args.isolated_copy)


if __name__ == "__main__":
    unittest.main()
