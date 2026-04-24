import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


BENCHMARKS_DIR = Path(__file__).resolve().parents[2]


def load_script_module(module_name: str, filename: str):
    path = BENCHMARKS_DIR / filename
    added_path = False
    if str(BENCHMARKS_DIR) not in sys.path:
        sys.path.insert(0, str(BENCHMARKS_DIR))
        added_path = True
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    try:
        spec.loader.exec_module(module)
    finally:
        if added_path:
            sys.path.remove(str(BENCHMARKS_DIR))
    return module


RELEASE_MATRIX = load_script_module(
    "release_quality_matrix_test",
    "release-quality-matrix.py",
)
EVIDENCE_CONTRACT = load_script_module(
    "evidence_contract_test",
    "evidence-contract.py",
)


class ReleaseQualityMatrixTests(unittest.TestCase):
    def test_evidence_contract_validator_accepts_v1_shape(self):
        evidence = {
            "schema_version": "codelens-evidence-v1",
            "domain": "retrieval",
            "active_backend": "hybrid",
            "confidence": 0.91,
            "confidence_basis": "hybrid_semantic_sparse",
            "degraded_reason": None,
            "signals": {"preferred_lane": "hybrid_semantic_sparse"},
        }

        self.assertEqual(
            EVIDENCE_CONTRACT.validate_evidence(evidence, expected_domain="retrieval"),
            [],
        )

    def test_evidence_contract_validator_rejects_missing_fields(self):
        errors = EVIDENCE_CONTRACT.validate_evidence(
            {
                "schema_version": "codelens-evidence-v1",
                "domain": "retrieval",
                "confidence": 0.5,
            },
            expected_domain="retrieval",
        )

        self.assertIn("active_backend must be one of known backend labels", errors)
        self.assertIn("signals must be an object", errors)

    def test_evidence_contract_suite_is_candidate_only(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            command_specs, compare_spec = RELEASE_MATRIX.suite_commands(
                "evidence_contract",
                project=Path("/repo"),
                baseline_binary=Path("/tmp/base"),
                candidate_binary=Path("/tmp/candidate"),
                output_dir=Path(tmpdir),
                preset="balanced",
                http_iterations=1,
            )

        self.assertEqual(compare_spec, {"type": "single", "compare": "evidence_contract"})
        self.assertEqual(len(command_specs), 1)
        self.assertEqual(command_specs[0]["label"], "candidate")
        self.assertIn("evidence-contract.py", command_specs[0]["argv"][1])
        self.assertIn("--binary", command_specs[0]["argv"])

    def test_gate_fails_on_evidence_contract_or_default_tool_count_growth(self):
        failures = RELEASE_MATRIX.gate_results(
            {
                "evidence_contract": {
                    "ok": False,
                    "failure_count": 2,
                },
                "http_surface": {
                    "comparisons": [
                        {
                            "supported": True,
                            "baseline": {
                                "scenario": "surface_tools_list",
                                "tool_count_p50": 8,
                            },
                            "candidate": {
                                "scenario": "surface_tools_list",
                                "tool_count_p50": 10,
                            },
                        }
                    ]
                },
            }
        )

        self.assertIn("evidence_contract failed with 2 failures", failures)
        self.assertIn("http_surface default tools/list tool_count increased", failures)


if __name__ == "__main__":
    unittest.main()
