import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT_DIR = REPO_ROOT / "scripts" / "finetune"


def load_script_module():
    if str(REPO_ROOT / "benchmarks") not in sys.path:
        sys.path.insert(0, str(REPO_ROOT / "benchmarks"))
    spec = importlib.util.spec_from_file_location(
        "promotion_gate_test",
        SCRIPT_DIR / "promotion_gate.py",
    )
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class PromotionGateTests(unittest.TestCase):
    def test_stage_candidate_model_uses_model_dir_without_embed_model_override(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            source = Path(tmpdir) / "candidate"
            source.mkdir()
            (source / "model.onnx").write_text("onnx", encoding="utf-8")

            cleanup_root, env = module.stage_candidate_model(
                source,
                "MiniLM-L12-CodeLens-LoRA-INT8",
            )
            try:
                self.assertEqual(env["CODELENS_MODEL_DIR"], str(cleanup_root))
                self.assertNotIn("CODELENS_EMBED_MODEL", env)
                self.assertTrue((cleanup_root / "codesearch" / "model.onnx").exists())
            finally:
                shutil.rmtree(cleanup_root, ignore_errors=True)

    def test_retrieval_benchmark_uses_compact_stdout(self):
        module = load_script_module()
        captured = {}

        def fake_run(cmd, *, env):
            captured["cmd"] = cmd
            captured["env"] = env
            return object()

        def fake_require_success(result, label):
            captured["label"] = label

        module.run = fake_run
        module.require_success = fake_require_success

        module.run_retrieval_benchmark(
            ".",
            "target/debug/codelens-mcp",
            Path("/tmp/report.json"),
            Path("/tmp/report.md"),
            env={"CODELENS_MODEL_DIR": "/tmp/model"},
        )

        stdout_index = captured["cmd"].index("--stdout")
        self.assertEqual(captured["cmd"][stdout_index + 1], "summary")
        self.assertEqual(captured["label"], "embedding-quality.py")


if __name__ == "__main__":
    unittest.main()
