import importlib.util
import os
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]


def load_script_module(module_name: str, filename: str):
    path = REPO_ROOT / filename
    added_path = False
    if str(path.parent) not in sys.path:
        sys.path.insert(0, str(path.parent))
        added_path = True
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    try:
        spec.loader.exec_module(module)
    finally:
        if added_path:
            sys.path.remove(str(path.parent))
    return module


VERIFY_MODEL_ASSETS = load_script_module(
    "verify_model_assets_test",
    "scripts/verify-model-assets.py",
)


def write_required_assets(model_dir: Path) -> None:
    model_dir.mkdir(parents=True)
    for name in VERIFY_MODEL_ASSETS.REQUIRED_FILES:
        (model_dir / name).write_text("{}", encoding="utf-8")


class VerifyModelAssetsTests(unittest.TestCase):
    def test_accepts_release_codesearch_layout(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            write_required_assets(root / "models" / "codesearch")

            model_dir, attempts = VERIFY_MODEL_ASSETS.find_model_dir(root, "auto")

            self.assertEqual(model_dir, root / "models" / "codesearch")
            self.assertTrue(any(attempt["path"] == str(model_dir) for attempt in attempts))

    @unittest.skipUnless(hasattr(os, "symlink"), "symlink unavailable")
    def test_rejects_symlinked_required_asset(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            model_dir = root / "models" / "codesearch"
            write_required_assets(model_dir)
            real_model = model_dir / "model-real.onnx"
            real_model.write_text("{}", encoding="utf-8")
            (model_dir / "model.onnx").unlink()
            os.symlink(real_model, model_dir / "model.onnx")

            found, attempts = VERIFY_MODEL_ASSETS.find_model_dir(root, "auto")

            self.assertIsNone(found)
            selected_attempt = next(
                attempt for attempt in attempts if attempt["path"] == str(model_dir)
            )
            self.assertEqual(selected_attempt["missing"], [])
            self.assertEqual(selected_attempt["symlinked"], ["model.onnx"])


if __name__ == "__main__":
    unittest.main()
