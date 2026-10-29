import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[2] / "analysis-lite-latency.py"


def load():
    spec = importlib.util.spec_from_file_location("analysis_lite_latency_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class AnalysisLiteLatencyTests(unittest.TestCase):
    def test_target_defaults_to_refs_and_rejects_other_modes(self):
        module = load()
        self.assertEqual(module.parse_target("/r/a.ts@f"), ("/r/a.ts", "f", "refs"))
        self.assertEqual(module.parse_target("/r/a.ts@f@impact"), ("/r/a.ts", "f", "impact"))
        for bad in ("/r/a.ts", "@f", "/r/a.ts@f@callers", "/r/a.ts@f@refs@x"):
            with self.assertRaises(ValueError, msg=bad):
                module.parse_target(bad)

    def test_normalize_drops_only_volatile_fields(self):
        module = load()
        payload = {
            "status": "ok",
            "snapshot": {"generation": 7, "files": 3},
            "freshness": {"watcher_running": True},
            "results": {"references": [{"generation": 1, "line": 2}]},
        }
        self.assertEqual(
            module.normalize(payload),
            {"status": "ok", "snapshot": {"files": 3}, "results": {"references": [{"line": 2}]}},
        )

    def test_compare_reports_only_differing_targets(self):
        module = load()
        runs = [
            {"binary": "old", "rows": [{"target": "a", "result": {"x": 1}}, {"target": "b", "result": {"y": 1}}]},
            {"binary": "new", "rows": [{"target": "a", "result": {"x": 1}}, {"target": "b", "result": {"y": 2}}]},
        ]
        self.assertEqual(module.compare(runs), [{"binary": "new", "target": "b"}])

    def test_synthetic_bundle_nests_callbacks_and_targets_a_real_export(self):
        module = load()
        with tempfile.TemporaryDirectory() as root:
            targets = module.write_synthetic_project(root, depth=3, modules=2)
            bundle = (Path(root) / "evidence" / "bundle.js").read_text(encoding="utf-8")
            self.assertEqual(bundle.count("f(() => {"), 6)
            self.assertEqual(bundle.count("});"), 6)
            self.assertEqual(len(targets), 2)
            file, name, _ = module.parse_target(targets[0])
            self.assertIn(f"export function {name}", Path(file).read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
