import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "scripts" / "finetune" / "prepare_bundled_teacher_candidate.py"


def load_script_module():
    spec = importlib.util.spec_from_file_location(
        "prepare_bundled_teacher_candidate_test",
        SCRIPT,
    )
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class PrepareBundledTeacherCandidateTests(unittest.TestCase):
    def write_model_assets(self, model_dir: Path):
        module = load_script_module()
        model_dir.mkdir(parents=True)
        for name in module.REQUIRED_MODEL_ASSETS:
            (model_dir / name).write_text("{}", encoding="utf-8")

    def test_resolve_model_dir_accepts_root_containing_codesearch(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            model_dir = root / "codesearch"
            self.write_model_assets(model_dir)

            resolved = module.resolve_model_dir(root)

            self.assertEqual(resolved, model_dir.resolve())

    def test_prepare_candidate_copies_assets_and_writes_manifest(self):
        module = load_script_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            teacher = root / "teacher"
            output = root / "candidate"
            self.write_model_assets(teacher)

            manifest_path = module.prepare_candidate(
                teacher,
                output,
                label="bundled-teacher-noop",
            )

            onnx_dir = output / "onnx"
            self.assertTrue((onnx_dir / "model.onnx").exists())
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["candidate_type"], "bundled_teacher_noop")
            self.assertEqual(manifest["teacher_model_dir"], str(teacher.resolve()))
            self.assertIn("promotion_gate.py", " ".join(manifest["promotion_gate_command"]))


if __name__ == "__main__":
    unittest.main()
