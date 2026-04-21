import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


HARNESS_DIR = Path(__file__).resolve().parents[1]
BENCH_DIR = HARNESS_DIR.parent


def load_script_module(module_name: str, path: Path):
    argv = sys.argv[:]
    path_head = sys.path[:]
    try:
        sys.argv = [str(path)]
        sys.path.insert(0, str(BENCH_DIR))
        spec = importlib.util.spec_from_file_location(module_name, path)
        module = importlib.util.module_from_spec(spec)
        assert spec and spec.loader
        sys.modules[module_name] = module
        spec.loader.exec_module(module)
        return module
    finally:
        sys.argv = argv
        sys.path[:] = path_head


EMBED_QUALITY = load_script_module(
    "embedding_quality_test",
    BENCH_DIR / "embedding-quality.py",
)


class EmbeddingQualityDatasetTests(unittest.TestCase):
    def test_resolve_expected_file_suffix_prefers_definition_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            mod_file = root / "crates" / "demo" / "src" / "symbols" / "mod.rs"
            api_file = root / "crates" / "demo" / "src" / "symbols" / "api.rs"
            api_file.parent.mkdir(parents=True, exist_ok=True)
            mod_file.write_text("pub use api::find_symbol_range;\n", encoding="utf-8")
            api_file.write_text(
                "pub fn find_symbol_range() -> usize { 1 }\n",
                encoding="utf-8",
            )

            locator = EMBED_QUALITY.SourceDefinitionLocator(root)
            resolved = locator.resolve_expected_file_suffix(
                "find_symbol_range",
                "crates/demo/src/symbols/mod.rs",
            )

        self.assertEqual(resolved, "crates/demo/src/symbols/api.rs")

    def test_canonicalize_dataset_rows_skips_unresolved_symbols(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            state_file = root / "crates" / "demo" / "src" / "state.rs"
            state_file.parent.mkdir(parents=True, exist_ok=True)
            state_file.write_text("pub struct AppState;\n", encoding="utf-8")

            rows = [
                {
                    "query": "missing symbol",
                    "expected_symbol": "ProjectOverride",
                    "expected_file_suffix": "crates/demo/src/state.rs",
                }
            ]
            canonical, adapted, skipped = EMBED_QUALITY.canonicalize_dataset_rows(
                rows, root
            )

        self.assertEqual(canonical, [])
        self.assertEqual(adapted, 0)
        self.assertEqual(skipped, ["missing symbol"])
